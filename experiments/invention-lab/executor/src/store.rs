//! Durable files for one slot. Each write goes to a temporary file, is synced,
//! renamed into place and the directory is synced, so a crash leaves either the
//! old or the new state. A runner holds a [`SlotClaim`] (an exclusive advisory
//! lock on `slot.lock`) from admission through finalization, so only one
//! execute or recover owns a slot at a time; the lock is released when the claim
//! is dropped or the process dies. This is a local store: it does not prevent
//! rollback by someone who can write the directory, and code that writes the
//! directory without a claim isn't stopped by it.
use genesis_core::development_method::ExactContentRef;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct Store {
    dir: PathBuf,
}

/// Exclusive ownership of one slot directory for as long as it is held.
pub struct SlotClaim {
    _lock: File,
}

impl SlotClaim {
    /// Takes the slot's lock without waiting; a held lock is `WouldBlock`.
    pub fn acquire(dir: &Path) -> std::io::Result<Self> {
        fs::create_dir_all(dir)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("slot.lock"))?;
        lock.try_lock().map_err(|e| match e {
            fs::TryLockError::WouldBlock => std::io::Error::from(std::io::ErrorKind::WouldBlock),
            fs::TryLockError::Error(e) => e,
        })?;
        Ok(Self { _lock: lock })
    }
}

/// Another runner holds this slot's claim.
pub fn is_busy(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::WouldBlock
}

impl Store {
    /// Creates the slot directory (mode 0700) if needed. Mutating a slot
    /// requires holding its [`SlotClaim`].
    pub fn open(dir: &Path) -> std::io::Result<Self> {
        fs::create_dir_all(dir.join("artifacts"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self { dir: dir.into() })
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    pub fn exists(&self, name: &str) -> bool {
        self.path(name).exists()
    }

    pub fn read(&self, name: &str) -> std::io::Result<Vec<u8>> {
        fs::read(self.path(name))
    }

    pub fn write(&self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        write_durable(&self.path(name), bytes)
    }

    /// Stores bytes under their reference's digest; refuses a digest mismatch.
    pub fn put_artifact(&self, reference: &ExactContentRef, bytes: &[u8]) -> std::io::Result<()> {
        if crate::refs::sha256_hex(bytes) != reference.sha256 {
            return Err(std::io::Error::other(
                "artifact bytes don't match their reference",
            ));
        }
        let path = self.path(&format!("artifacts/{}", reference.sha256));
        if path.exists() {
            return Ok(());
        }
        write_durable(&path, bytes)
    }
}

pub fn write_durable(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("no parent"))?;
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("state")
    ));
    {
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    File::open(dir)?.sync_all()
}
