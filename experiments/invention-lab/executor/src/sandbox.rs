//! How a method process is started, and what it can reach.
//!
//! `Mode::Namespaces` (needs root): new network, mount and PID namespaces;
//! every existing mount is remounted read-only where the kernel allows it (a
//! mount that refuses keeps its mode, and the setup doesn't fail on it); fresh
//! private tmpfs over `/home`, `/root`, `/tmp`, `/var/tmp`, `/dev/shm` and
//! `/run`, so the evaluator, its sources, the slot state, the staged files and
//! other runs aren't visible there; the method runs as `nobody` with
//! `no_new_privs` and gets a copy of its package's source files in `/tmp/work`.
//! Writes land in those private tmpfs mounts, which vanish at exit, unless a
//! mount refused the read-only remount. Killing the namespace's first process
//! ends every process in it. [`Mode::stop`] kills that process directly:
//! `unshare --kill-child` alone is not enough, because the privilege drop
//! clears the parent-death signal it relies on.
//!
//! Resource limits (prlimit, applied before the privilege drop): 1 GiB address
//! space, 64 processes for the `nobody` user, 64 MiB per written file, 256 open
//! files; CPU time is bounded by the wall-time kill.
//!
//! Not enforced: read access to the rest of the (read-only) system (`/usr`,
//! `/etc`, other mounts such as `/var` subtrees not listed) and any syscall
//! filter. The process limit is per user, so concurrent runs share it. A method can
//! still compute its own physics inside its wall time; `simulation_steps`
//! meters evaluator calls only.
//!
//! Model calls and tokens are recorded as unknown in every mode: the network
//! namespace doesn't exclude filesystem Unix sockets or a model reachable on a
//! readable path, so "no network" isn't proof of "no model".
//!
//! `Mode::None` runs the method directly with no isolation at all.
use std::path::Path;
use std::process::{Child, Command, Stdio};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Namespaces,
    None,
}

/// Commands the sandboxed shell runs as root before dropping to `nobody`.
const SETUP: &str = r#"set -e
mount --bind / /
# Every existing mount, not just /, becomes read-only (best effort per mount;
# kernel pseudo-filesystems that refuse are left as they are).
awk '{print $5}' /proc/self/mountinfo | sort -u | while read -r m; do
  mount -o remount,bind,ro "$m" 2>/dev/null || true
done
mount -t tmpfs -o size=64m,mode=755 none /home
mount -t tmpfs -o size=1m,mode=700 none /root
mount -t tmpfs -o size=256m,mode=1777 none /tmp
mount -t tmpfs -o size=16m,mode=1777 none /var/tmp
mount -t tmpfs -o size=16m,mode=1777 none /dev/shm
mkdir -p /tmp/work
cp -R "$LAB_STAGE/files/." /tmp/work/
chmod -R a+rX /tmp/work
mount -t tmpfs -o size=1m,mode=755 none /run
cd /tmp/work
exec prlimit --as=1073741824 --nproc=64 --fsize=67108864 --nofile=256 -- \
  setpriv --reuid 65534 --regid 65534 --clear-groups --no-new-privs -- "$LAB_INTERPRETER" "/tmp/work/$LAB_ENTRY"
"#;

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Namespaces => "namespaces",
            Self::None => "none",
        }
    }

    pub fn describe(self) -> serde_json::Value {
        match self {
            Self::Namespaces => serde_json::json!({
                "mode": "namespaces",
                "network": "none (new network namespace)",
                "user": "nobody, no_new_privs",
                "mounts": "every existing mount remounted read-only where the kernel allows it",
                "hidden": ["/home", "/root", "/tmp", "/var/tmp", "/dev/shm", "/run"],
                "pid": "new namespace, killed as a whole at the wall-time limit",
                "limits": {"address_space_bytes": 1073741824u64, "nproc_user": 64, "file_size_bytes": 67108864u64, "open_files": 256},
                "not_enforced": ["reads of the rest of the read-only system", "syscall filter", "filesystem Unix sockets", "mounts that refuse a read-only remount"],
                "model_usage": "unknown",
                "setup": SETUP,
            }),
            Self::None => serde_json::json!({"mode": "none", "isolation": "none"}),
        }
    }

    /// Whether model calls and tokens can be recorded as measured zero. Never:
    /// no network is not the same as no model. Filesystem Unix sockets and any
    /// local model or service on readable paths are not excluded, so model
    /// usage stays unknown in every mode.
    pub fn bounds_model_use(self) -> bool {
        false
    }

    /// `stage/files` holds the package's source files, outside the directories
    /// the sandbox hides; `entry` is the package's validated relative entrypoint.
    pub fn command(self, interpreter: &str, entry: &str, stage: &Path) -> Command {
        let mut c = match self {
            Self::Namespaces => {
                let mut c = Command::new("unshare");
                c.args([
                    "--net",
                    "--mount",
                    "--pid",
                    "--fork",
                    "--mount-proc",
                    "--propagation",
                    "private",
                    "--kill-child=SIGKILL",
                    "--",
                    "sh",
                    "-c",
                    SETUP,
                ])
                .env_clear()
                .env("PATH", "/usr/local/bin:/usr/bin:/bin")
                .env("LAB_STAGE", stage)
                .env("LAB_ENTRY", entry)
                .env("LAB_INTERPRETER", interpreter);
                c
            }
            Self::None => {
                let mut c = Command::new(interpreter);
                c.arg(stage.join("files").join(entry))
                    .current_dir(stage.join("files"))
                    .env_clear()
                    .env("PATH", "/usr/local/bin:/usr/bin:/bin");
                c
            }
        };
        c.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        c
    }
}

impl Mode {
    /// Stops a started method. In `Namespaces` mode this SIGKILLs the PID
    /// namespace's first process (the `unshare` child), which ends every
    /// process in the namespace; `unshare` then exits only after the kernel
    /// has torn the namespace down, so waiting on `child` afterwards waits for
    /// the whole tree. Killing `unshare` itself would not: the setpriv
    /// credential change clears `--kill-child`'s parent-death signal and the
    /// namespace would outlive it. In `None` mode only the method process is
    /// killed; its descendants are not tracked.
    pub fn stop(self, child: &mut Child) {
        if self == Self::Namespaces {
            let id = child.id();
            // No first process yet means `unshare` hasn't forked, or the
            // namespace is already ending and `unshare` is about to exit.
            for _ in 0..200 {
                let inits = std::fs::read_to_string(format!("/proc/{id}/task/{id}/children"))
                    .unwrap_or_default();
                let mut found = false;
                for pid in inits
                    .split_whitespace()
                    .filter(|p| p.parse::<u32>().is_ok())
                {
                    found = true;
                    let _ = Command::new("kill")
                        .args(["-KILL", pid])
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                }
                if found || !matches!(child.try_wait(), Ok(None)) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        let _ = child.kill();
    }
}
