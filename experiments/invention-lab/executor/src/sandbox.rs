//! How a method process is started, and what it can reach.
//!
//! `Mode::Namespaces` (needs root): new network, mount and PID namespaces; the
//! root filesystem is remounted read-only; fresh private tmpfs over `/home`,
//! `/root`, `/tmp`, `/var/tmp`, `/dev/shm` and `/run`, so the evaluator, its
//! sources, the slot state, the staged script and other runs aren't visible and
//! nothing written survives the run; the method runs as `nobody` with
//! `no_new_privs` and gets a copy of its package's source files in `/tmp/work`. Killing the
//! namespace's first process ends every process in it.
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
//! `Mode::None` runs the method directly: no isolation at all, so model and
//! network use can't be bounded and are reported unknown.
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Namespaces,
    None,
}

/// Commands the sandboxed shell runs as root before dropping to `nobody`.
const SETUP: &str = r#"set -e
mount --bind / /
mount -o remount,bind,ro /
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
                "root_filesystem": "read-only bind",
                "hidden": ["/home", "/root", "/tmp", "/var/tmp", "/dev/shm", "/run"],
                "pid": "new namespace, killed as a whole at the wall-time limit",
                "limits": {"address_space_bytes": 1073741824u64, "nproc_user": 64, "file_size_bytes": 67108864u64, "open_files": 256},
                "not_enforced": ["reads of the rest of the read-only system", "syscall filter"],
                "setup": SETUP,
            }),
            Self::None => serde_json::json!({"mode": "none", "isolation": "none"}),
        }
    }

    /// Model calls and tokens are measured as zero only when the method has no
    /// network and no model is provided inside the sandbox.
    pub fn bounds_model_use(self) -> bool {
        self == Self::Namespaces
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
