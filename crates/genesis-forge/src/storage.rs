use genesis_verification::decode_json;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::future::Future;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const MAX_JSON_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_EXTRACTOR_BYTES: usize = 128 * 1024;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

fn private_create(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn sync_parent(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| "cannot sync checkpoint directory; checkpoint durability is uncertain".into())
}

fn regular_file(path: &Path) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| "cannot inspect input file".to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("input must be a regular file, not a symlink or device".into());
    }
    Ok(())
}

pub fn read_bytes(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    regular_file(path)?;
    let file = File::open(path).map_err(|_| "cannot open input file".to_string())?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read input file".to_string())?;
    if bytes.len() > limit {
        return Err(format!("input exceeds {limit} byte limit"));
    }
    Ok(bytes)
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    decode_json(&read_bytes(path, MAX_JSON_BYTES)?)
        .map_err(|error| format!("invalid JSON/schema: {error}"))
}

struct BoundedWriter {
    bytes: Vec<u8>,
}
impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_JSON_BYTES - self.bytes.len() {
            return Err(std::io::Error::other("JSON output exceeds 8 MiB limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn encode_json(value: &impl Serialize) -> Result<Vec<u8>, String> {
    encode(value, false)
}

pub fn encode_compact_json(value: &impl Serialize) -> Result<Vec<u8>, String> {
    encode(value, true)
}

fn encode(value: &impl Serialize, compact: bool) -> Result<Vec<u8>, String> {
    let mut writer = BoundedWriter { bytes: Vec::new() };
    if compact {
        serde_json::to_writer(&mut writer, value)
    } else {
        serde_json::to_writer_pretty(&mut writer, value)
    }
    .map_err(|_| "cannot encode JSON within 8 MiB output limit".to_string())?;
    writer
        .write_all(b"\n")
        .map_err(|_| "JSON output exceeds 8 MiB limit".to_string())?;
    Ok(writer.bytes)
}

/// Owns the cooperative exclusive lock until the provider action and its final
/// checkpoint save complete. A crash deliberately leaves the lock for review.
pub struct CheckpointLock {
    path: PathBuf,
    lock_path: PathBuf,
    lock_file: File,
}

impl CheckpointLock {
    pub fn acquire(path: &Path) -> Result<Self, String> {
        let name = path
            .file_name()
            .ok_or_else(|| "checkpoint must name a file".to_string())?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let parent = parent
            .canonicalize()
            .map_err(|_| "checkpoint directory must already exist".to_string())?;
        let path = parent.join(name);
        let mut lock_name = OsString::from(name);
        lock_name.push(".lock");
        let lock_path = parent.join(lock_name);
        let lock_file = private_create(&lock_path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                "checkpoint lock already exists; another process may be active. After a crash, confirm that process has stopped, inspect the checkpoint, then remove the .lock file manually; never replay a pending POST".to_string()
            } else { "cannot create private checkpoint lock".into() }
        })?;
        let mut guard = Self {
            path,
            lock_path,
            lock_file,
        };
        writeln!(guard.lock_file, "pid={}", std::process::id())
            .map_err(|_| "cannot write checkpoint lock".to_string())?;
        guard
            .lock_file
            .sync_all()
            .map_err(|_| "cannot sync checkpoint lock".to_string())?;
        sync_parent(&parent)?;
        Ok(guard)
    }

    pub fn load<T: DeserializeOwned>(&self) -> Result<T, String> {
        read_json(&self.path)
    }

    pub fn create(&self, value: &impl Serialize) -> Result<(), String> {
        self.save(&encode_json(value)?, false)
    }

    pub fn replace(&self, value: &impl Serialize) -> Result<(), String> {
        self.save(&encode_json(value)?, true)
    }

    pub fn create_compact(&self, value: &impl Serialize) -> Result<(), String> {
        self.save(&encode_compact_json(value)?, false)
    }

    pub fn replace_compact(&self, value: &impl Serialize) -> Result<(), String> {
        self.save(&encode_compact_json(value)?, true)
    }

    fn save(&self, bytes: &[u8], replace: bool) -> Result<(), String> {
        if replace {
            regular_file(&self.path)?;
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "checkpoint parent missing".to_string())?;
        let temp_path = parent.join(format!(
            ".genesis-forge-{}-{}.tmp",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = private_create(&temp_path)
            .map_err(|_| "cannot create checkpoint temporary file".to_string())?;
        let mut temp = TempFile {
            path: temp_path.clone(),
        };
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| "cannot durably write checkpoint".to_string())?;
        if replace {
            fs::rename(&temp_path, &self.path)
                .map_err(|_| "cannot atomically replace checkpoint".to_string())?;
        } else {
            // hard_link is atomic and cannot replace a preexisting destination;
            // unlike exists()+rename(), prepare cannot clobber a racing writer.
            fs::hard_link(&temp_path, &self.path).map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    "checkpoint already exists; prepare never overwrites it".to_string()
                } else {
                    "cannot atomically create checkpoint".to_string()
                }
            })?;
            fs::remove_file(&temp_path)
                .map_err(|_| "checkpoint created but cannot remove temporary link".to_string())?;
        }
        temp.path = PathBuf::new();
        sync_parent(parent)
    }

    /// The effect closure cannot run until the pending intent has been written,
    /// fsynced, atomically replaced, and the parent directory fsynced.
    pub async fn persist_before_effect<T, F, Fut>(
        &self,
        value: &impl Serialize,
        effect: F,
    ) -> Result<T, String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = T>,
    {
        self.replace(value)?;
        Ok(effect().await)
    }

    /// Acquisition keeps two linked histories. Reserve 2 MiB of compact JSON
    /// capacity for a bounded provider completion before sending any request.
    pub async fn persist_compact_before_effect<T, F, Fut>(
        &self,
        value: &impl Serialize,
        effect: F,
    ) -> Result<T, String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = T>,
    {
        let bytes = encode_compact_json(value)?;
        if bytes.len() > MAX_JSON_BYTES - 2 * 1024 * 1024 {
            return Err(
                "acquisition checkpoint has insufficient completion capacity; no request was sent"
                    .into(),
            );
        }
        self.save(&bytes, true)?;
        Ok(effect().await)
    }
}

struct TempFile {
    path: PathBuf,
}
impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.path.as_os_str().is_empty() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl Drop for CheckpointLock {
    fn drop(&mut self) {
        // Do not remove a replacement lock if an operator removed ours while we
        // were still running. The lock is cooperative, not a tenant boundary.
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let owned = self
                .lock_file
                .metadata()
                .ok()
                .zip(fs::symlink_metadata(&self.lock_path).ok())
                .is_some_and(|(ours, current)| {
                    ours.dev() == current.dev() && ours.ino() == current.ino()
                });
            if !owned {
                return;
            }
        }
        if fs::remove_file(&self.lock_path).is_ok() {
            if let Some(parent) = self.lock_path.parent() {
                let _ = sync_parent(parent);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::sync::atomic::AtomicBool;

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "genesis-forge-storage-{}-{}",
                std::process::id(),
                NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn checkpoint(&self) -> PathBuf {
            self.0.join("session.json")
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn pending_intent_is_readable_and_lock_exclusive_before_effect_runs() {
        let directory = Directory::new();
        let path = directory.checkpoint();
        let guard = CheckpointLock::acquire(&path).unwrap();
        guard.create(&json!({"pending": null})).unwrap();
        let pending = json!({"pending": {"kind": "create_actor", "sequence": 1}});
        let result = guard
            .persist_before_effect(&pending, || async {
                let saved: Value = read_json(&path).unwrap();
                assert_eq!(saved, pending);
                assert!(CheckpointLock::acquire(&path).is_err());
                assert!(directory.0.join("session.json.lock").exists());
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    assert_eq!(
                        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                        0o600
                    );
                    assert_eq!(
                        fs::metadata(directory.0.join("session.json.lock"))
                            .unwrap()
                            .permissions()
                            .mode()
                            & 0o777,
                        0o600
                    );
                }
                "effect completed"
            })
            .await
            .unwrap();
        assert_eq!(result, "effect completed");
        guard.replace(&json!({"completed": true})).unwrap();
        drop(guard);
        assert!(!directory.0.join("session.json.lock").exists());
    }

    #[tokio::test]
    async fn oversized_checkpoint_never_runs_effect_or_replaces_previous_state() {
        let directory = Directory::new();
        let guard = CheckpointLock::acquire(&directory.checkpoint()).unwrap();
        let previous = json!({"phase": "prepared"});
        guard.create(&previous).unwrap();
        let called = AtomicBool::new(false);
        let oversized = "x".repeat(MAX_JSON_BYTES);
        let result = guard
            .persist_before_effect(&oversized, || async {
                called.store(true, Ordering::SeqCst);
            })
            .await;
        assert!(result.is_err());
        assert!(!called.load(Ordering::SeqCst));
        assert_eq!(guard.load::<Value>().unwrap(), previous);
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 2);
    }

    #[tokio::test]
    async fn acquisition_reserves_completion_capacity_before_persisting_or_sending() {
        let directory = Directory::new();
        let guard = CheckpointLock::acquire(&directory.checkpoint()).unwrap();
        let previous = json!({"phase": "prepared"});
        guard.create_compact(&previous).unwrap();
        let called = AtomicBool::new(false);
        // This fits the ordinary checkpoint limit but leaves insufficient space
        // for the bounded provider response after a request has been accepted.
        let too_full = json!({"pending": "x".repeat(6 * 1024 * 1024)});
        assert!(guard
            .persist_compact_before_effect(&too_full, || async {
                called.store(true, Ordering::SeqCst);
            })
            .await
            .is_err());
        assert!(!called.load(Ordering::SeqCst));
        assert_eq!(guard.load::<Value>().unwrap(), previous);
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 2);
    }

    #[tokio::test]
    async fn compact_acquisition_intent_is_durable_and_locked_before_effect() {
        let directory = Directory::new();
        let path = directory.checkpoint();
        let guard = CheckpointLock::acquire(&path).unwrap();
        guard.create_compact(&json!({"pending": null})).unwrap();
        let pending = json!({"pending": {"provider": "model", "attempt": 1}});
        guard
            .persist_compact_before_effect(&pending, || async {
                assert_eq!(read_json::<Value>(&path).unwrap(), pending);
                assert!(CheckpointLock::acquire(&path).is_err());
                assert_eq!(
                    fs::read(&path).unwrap(),
                    encode_compact_json(&pending).unwrap()
                );
            })
            .await
            .unwrap();
        guard.replace_compact(&json!({"completed": true})).unwrap();
        assert_eq!(guard.load::<Value>().unwrap(), json!({"completed": true}));
    }
}
