//! How a method process is started, and what it can reach.
//!
//! `Mode::Namespaces` (needs root): new network, IPC, mount and PID namespaces,
//! then a chroot into a minimal method root built for this run and nothing
//! else. The root holds, read-only: the interpreter and its standard library
//! (any `site-packages`/`dist-packages` inside it masked empty), `sh`,
//! `prlimit`, `setpriv`, the shared libraries those and the stdlib extension
//! modules link (plus `/etc/ld.so.cache`), and `/dev/{null,zero,random,urandom}`.
//! The only writable place is a private tmpfs `/tmp`, where `/tmp/work` holds a
//! copy of the package's source files. There is no `/proc`, `/etc` (beyond the
//! loader cache), `/home`, `/run`, `/var`, host `/tmp` or other mount in it.
//! Setup refuses to start if any socket file is inside the root. Only stdin,
//! stdout and stderr cross into the namespace: every other descriptor the
//! executor holds is marked close-on-exec before `unshare` starts. This requires
//! Linux close_range(CLOSE_RANGE_CLOEXEC); any failure refuses startup. The method
//! runs as `nobody` with `no_new_privs`, so it can't leave the chroot. Killing
//! the namespace's first process ends every process in it. [`Mode::stop`]
//! kills that process directly: `unshare --kill-child` alone is not enough,
//! because the privilege drop clears the parent-death signal it relies on.
//!
//! Resource limits (prlimit, applied before the privilege drop): 1 GiB address
//! space, 64 processes for the `nobody` user, 64 MiB per written file, 256 open
//! files; CPU time is bounded by the wall-time kill.
//!
//! **Model usage.** With no network (a fresh network namespace, which also
//! scopes abstract Unix sockets), no socket file and no other IPC namespace
//! peer, and no files beyond the interpreter image and the method's own
//! source, no model or model service is reachable, so model calls and tokens
//! are recorded as measured zero in this mode. That is a statement about
//! external models; whatever the method computes itself is its own work.
//!
//! Not enforced: a syscall filter. The process limit is per user, so
//! concurrent runs share it. A method can still compute its own physics inside
//! its wall time; `simulation_steps` meters evaluator calls only.
//!
//! `Mode::None` runs the method directly with no isolation at all, and model
//! usage stays unknown.
use std::path::Path;
use std::process::{Child, Command, Stdio};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Namespaces,
    None,
}

/// Commands the sandboxed shell runs as root: build the method root, then
/// chroot, limit and drop to `nobody`.
const SETUP: &str = r#"set -e
R="$LAB_STAGE/root"
mkdir -p "$R"
mount -t tmpfs -o size=8m,mode=755 none "$R"
for l in /bin /sbin /lib /lib64; do
  if [ -L "$l" ]; then ln -s "$(readlink "$l")" "$R$l"; fi
done
# Recreate an absolute path inside the root: symlinks as symlinks, files and
# directories as read-only bind mounts.
add() {
  d=$(cd "$(dirname "$1")" && pwd -P)
  p="$d/$(basename "$1")"
  if [ -L "$p" ]; then
    t=$(readlink "$p")
    mkdir -p "$R$d"
    if [ ! -e "$R$p" ] && [ ! -L "$R$p" ]; then ln -s "$t" "$R$p"; fi
    case "$t" in /*) add "$t" ;; *) add "$d/$t" ;; esac
  elif [ ! -e "$R$p" ]; then
    if [ -d "$p" ]; then mkdir -p "$R$p"; else mkdir -p "$R$d"; : > "$R$p"; fi
    mount --bind "$p" "$R$p"
    mount -o remount,bind,ro "$R$p"
  fi
}
libs() { ldd "$1" 2>/dev/null | awk '{for (i = 1; i <= NF; i++) if ($i ~ /^\//) print $i}'; }
INTERP=$(readlink -f "$(command -v "$LAB_INTERPRETER")")
STDLIB=$("$INTERP" -I -S -c 'import sysconfig; print(sysconfig.get_paths()["stdlib"])')
SH=$(command -v sh); PRLIMIT=$(command -v prlimit); SETPRIV=$(command -v setpriv)
for f in "$INTERP" "$SH" "$PRLIMIT" "$SETPRIV"; do
  add "$f"
  for l in $(libs "$f"); do add "$l"; done
done
add "$STDLIB"
for so in "$STDLIB"/lib-dynload/*.so; do
  for l in $(libs "$so"); do add "$l"; done
done
for sp in "$R$STDLIB/site-packages" "$R$STDLIB/dist-packages"; do
  if [ -d "$sp" ]; then mount -t tmpfs -o size=4k,mode=555 none "$sp"; fi
done
if [ -e /etc/ld.so.cache ]; then add /etc/ld.so.cache; fi
mkdir -p "$R/dev" "$R/tmp"
for n in null zero random urandom; do
  : > "$R/dev/$n"
  mount --bind "/dev/$n" "$R/dev/$n"
done
mount -t tmpfs -o size=256m,mode=1777 none "$R/tmp"
mkdir "$R/tmp/work"
cp -R "$LAB_STAGE/files/." "$R/tmp/work/"
chmod -R a+rX "$R/tmp/work"
if [ -n "$(find "$R" -type s 2>/dev/null)" ]; then
  echo "a socket file is inside the method root" >&2
  exit 1
fi
mount -o remount,ro "$R"
ENTRY="$LAB_ENTRY"
unset LAB_STAGE LAB_ENTRY LAB_INTERPRETER
exec chroot "$R" "$SH" -c 'cd /tmp/work && exec "$0" --as=1073741824 --nproc=64 --fsize=67108864 --nofile=256 -- "$1" --reuid 65534 --regid 65534 --clear-groups --no-new-privs -- "$2" "/tmp/work/$3"' \
  "$PRLIMIT" "$SETPRIV" "$INTERP" "$ENTRY"
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
                "namespaces": ["net", "ipc", "mount", "pid"],
                "root": "chroot into a per-run minimal root: interpreter, its stdlib (site/dist-packages masked), sh, prlimit, setpriv, their shared libraries, the loader cache and /dev/{null,zero,random,urandom}, all read-only; private tmpfs /tmp with the package files in /tmp/work; no socket files",
                "user": "nobody, no_new_privs",
                "descriptors": "stdin, stdout and stderr only; Linux close_range(3, UINT_MAX, CLOSE_RANGE_CLOEXEC) must succeed before exec; no partial descriptor-limit fallback",
                "pid": "new namespace, killed as a whole at the wall-time limit",
                "limits": {"address_space_bytes": 1073741824u64, "nproc_user": 64, "file_size_bytes": 67108864u64, "open_files": 256},
                "not_enforced": ["syscall filter"],
                "model_usage": "measured zero: no network, socket file, IPC peer or file beyond the interpreter image and the method source",
                "setup": SETUP,
            }),
            Self::None => serde_json::json!({"mode": "none", "isolation": "none"}),
        }
    }

    /// Whether model calls and tokens can be recorded as measured zero: only
    /// inside the minimal method root, where no model or service is reachable.
    pub fn bounds_model_use(self) -> bool {
        matches!(self, Self::Namespaces)
    }

    /// `stage/files` holds the package's source files, outside the directories
    /// the sandbox hides; `entry` is the package's validated relative entrypoint.
    pub fn command(self, interpreter: &str, entry: &str, stage: &Path) -> Command {
        let mut c = match self {
            Self::Namespaces => {
                let mut c = Command::new("unshare");
                c.args([
                    "--net",
                    "--ipc",
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
                .env("PATH", "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin")
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
                let mut signalled = false;
                for pid in namespace_inits(id) {
                    // Signal directly: spawning a `kill` program can fail (no
                    // processes or descriptors left, a stripped PATH), and a
                    // failed stop would leave the caller waiting forever.
                    signalled |= sigkill(pid);
                }
                if signalled || !matches!(child.try_wait(), Ok(None)) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            // Nothing could be signalled: stop `unshare` itself so the caller's
            // wait is bounded. The namespace may then outlive it (see above).
        }
        let _ = child.kill();
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
