//! How a method process is started, and what it can reach.
//!
//! `Mode::Namespaces` (needs root): new network, mount and PID namespaces;
//! every existing mount is remounted read-only where the kernel allows it (a
//! mount that refuses keeps its mode, and the setup doesn't fail on it); fresh
//! private tmpfs over `/home`, `/root`, `/tmp`, `/var/tmp`, `/dev/shm` and
//! `/run`, so the evaluator, its sources, the slot state, the staged files and
//! other runs aren't visible there; the method runs as `nobody` with
//! `no_new_privs` and gets a copy of its package's source files in `/tmp/work`.
//! Only stdin, stdout and stderr cross into the namespace: every other
//! descriptor the executor holds is marked close-on-exec before `unshare` starts.
//! Linux close_range(CLOSE_RANGE_CLOEXEC) must succeed; failure refuses startup.
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

/// What stopping the launcher established. Reaping it later cannot upgrade
/// an unconfirmed stop into evidence that its PID namespace ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopOutcome {
    NamespaceInitStopped,
    LauncherExitedSuccessfully,
    Unconfirmed,
}

impl StopOutcome {
    pub fn confirms_namespace_end(self) -> bool {
        self != Self::Unconfirmed
    }
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
                "descriptors": "stdin, stdout and stderr only; Linux close_range(3, UINT_MAX, CLOSE_RANGE_CLOEXEC) must succeed before exec; no partial descriptor-limit fallback",
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
                // Only stdio crosses into the namespace. A descriptor this
                // process inherited without close-on-exec (a directory, a
                // socket) would otherwise reach the method and let it fchdir
                // out of the chroot or talk to the host.
                // SAFETY: the closure only makes async-signal-safe syscalls.
                unsafe {
                    use std::os::unix::process::CommandExt;
                    c.pre_exec(cloexec_above_stdio);
                }
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
    /// Attempts to stop a started method. In `Namespaces` mode it targets the
    /// PID namespace's first process (the `unshare` child). Whole-tree evidence
    /// requires a confirming outcome and a subsequent successful launcher reap;
    /// the launcher-only fallback provides no such assurance.
    /// Killing `unshare` itself would not end the namespace: the setpriv
    /// credential change clears `--kill-child`'s parent-death signal and the
    /// namespace would outlive it. In `None` mode only the method process is
    /// killed; its descendants are not tracked.
    pub fn stop(self, child: &mut Child) -> StopOutcome {
        if self == Self::Namespaces {
            let id = child.id();
            // No first process yet means `unshare` hasn't forked, or the
            // namespace is already ending and `unshare` is about to exit.
            for _ in 0..200 {
                let inits = namespace_inits(id);
                let mut stopped = !inits.is_empty();
                for pid in inits {
                    // Signal directly: spawning a `kill` program can fail (no
                    // processes or descriptors left, a stripped PATH), and a
                    // failed stop would leave the caller waiting forever.
                    stopped &= sigkill(pid);
                }
                if stopped {
                    return StopOutcome::NamespaceInitStopped;
                }
                match child.try_wait() {
                    Ok(Some(status)) if status.success() => {
                        return StopOutcome::LauncherExitedSuccessfully;
                    }
                    Ok(None) => {}
                    Ok(Some(_)) | Err(_) => return StopOutcome::Unconfirmed,
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            // Nothing could be signalled: stop `unshare` itself so the caller's
            // wait is bounded. The namespace may then outlive it (see above).
        }
        let _ = child.kill();
        StopOutcome::Unconfirmed
    }
}

/// `unshare`'s children: the namespace's first process. Falls back to a /proc
/// scan where the kernel lacks `/proc/<pid>/task/<pid>/children`.
fn namespace_inits(id: u32) -> Vec<i32> {
    let parse = |text: &str| -> Vec<i32> {
        text.split_whitespace()
            .filter_map(|p| p.parse::<i32>().ok())
            .filter(|p| *p > 0)
            .collect()
    };
    if let Ok(text) = std::fs::read_to_string(format!("/proc/{id}/task/{id}/children")) {
        return parse(&text);
    }
    let mut found = vec![];
    for entry in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        let status = std::fs::read_to_string(entry.path().join("status")).unwrap_or_default();
        let ppid = status
            .lines()
            .find_map(|l| l.strip_prefix("PPid:"))
            .and_then(|v| v.trim().parse::<u32>().ok());
        if ppid == Some(id) && pid > 0 {
            found.push(pid);
        }
    }
    found
}

/// Marks every descriptor above 2 close-on-exec, in the child between fork and
/// exec. Marking rather than closing keeps the spawn error pipe working.
/// A current resource limit is not an upper bound on descriptors already open:
/// the launcher may have lowered it. Refuse startup if the all-range operation
/// is unavailable rather than silently leave descriptors outside a partial walk.
fn cloexec_above_stdio() -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: raw syscalls on integers; no allocation (async-signal-safe).
        let rc = unsafe {
            libc::syscall(
                libc::SYS_close_range,
                3u32,
                u32::MAX,
                libc::CLOSE_RANGE_CLOEXEC,
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(std::io::ErrorKind::Unsupported.into())
    }
}

/// SIGKILL `pid`; true if it was delivered or the process is already gone.
fn sigkill(pid: i32) -> bool {
    // SAFETY: kill(2) takes plain integers and has no memory effects.
    let rc = unsafe { libc::kill(pid, libc::SIGKILL) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

#[cfg(test)]
mod stop_tests {
    #[test]
    fn signalling_a_missing_process_counts_as_stopped() {
        // pid_max is far below i32::MAX - 1, so this pid can't exist.
        assert!(super::sigkill(i32::MAX - 1));
    }
}
