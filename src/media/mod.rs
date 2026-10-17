//! Manager-side media store (05 A8, 08): single-use, TTL-bounded capability artifacts.
//!
//! The store owns the bytes between a capture and one fetch. The id is the credential (128
//! bits, 32 lowercase hex); the store is memory-only for the URL lifetime — a manager restart
//! drops every entry (later fetches answer `404 media_expired`) — and `audit.store_clips`
//! adds a persisted copy under `<data>/media/<session>/<id>` that never resurrects the URL.
//!
//! Minting is an in-process API only (`pub(crate)` callers: the capture tools, tests). There
//! is no RPC method and no CLI verb that mints an id (A8's "no public mint surface").

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::config::{CoreConfig, Paths};
use crate::ipc::ErrorEnvelope;

/// `media.ttl`'s pinned default (04/05); the live config value wins when readable.
pub const DEFAULT_TTL: Duration = Duration::from_secs(600);
/// 08: the persisted-artifact cap is per session, oldest-first eviction.
pub const SESSION_CAP_BYTES: u64 = 128 * 1024 * 1024;

struct Entry {
    content_type: String,
    created: Instant,
    bytes: Arc<Vec<u8>>,
}

/// One fetch's worth of bytes, already claimed from the store.
pub struct MediaHandle {
    pub content_type: String,
    pub bytes: Arc<Vec<u8>>,
}

/// The store: memory-only URL entries + optional persisted copies per session.
pub struct MediaStore {
    entries: Mutex<HashMap<String, Entry>>,
    /// `<data root>/media` (05/08).
    root: PathBuf,
    /// A fixed TTL (tests); production reads `media.ttl` live from `config`.
    fixed_ttl: Option<Duration>,
    config: Option<PathBuf>,
    cap_bytes: u64,
}

impl MediaStore {
    /// The production store for an instance: `<data>/media`, `media.ttl` read live.
    pub fn for_instance(paths: &Paths) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            root: paths.media_dir(),
            fixed_ttl: None,
            config: Some(paths.core_config()),
            cap_bytes: SESSION_CAP_BYTES,
        }
    }

    /// A store for tests: fixed root, fixed TTL.
    pub fn with_ttl(root: impl Into<PathBuf>, ttl: Duration) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            root: root.into(),
            fixed_ttl: Some(ttl),
            config: None,
            cap_bytes: SESSION_CAP_BYTES,
        }
    }

    /// Test-only small cap, so eviction is exercised without 128 MiB of fixtures.
    pub fn with_limits(root: impl Into<PathBuf>, ttl: Duration, cap_bytes: u64) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            root: root.into(),
            fixed_ttl: Some(ttl),
            config: None,
            cap_bytes,
        }
    }

    /// Mint one artifact. `store_clips` (the scope's `audit.store_clips`) writes the same
    /// bytes to the session's persisted directory — one artifact, never a second capture.
    pub fn mint(
        &self,
        session_id: &str,
        content_type: &str,
        bytes: Vec<u8>,
        store_clips: bool,
    ) -> String {
        self.purge_expired();
        let id = crate::config::generate_token();
        if store_clips {
            let _ = self.persist(session_id, &id, &bytes);
            self.evict_session(session_id);
        }
        let mut entries = self.entries.lock().expect("media store lock");
        entries.insert(
            id.clone(),
            Entry {
                content_type: content_type.to_string(),
                created: Instant::now(),
                bytes: Arc::new(bytes),
            },
        );
        id
    }

    /// Persist a screenshot's bytes with **no** URL entry (05 §Capture): `store_screenshots`
    /// keeps the artifact for the retention window, and a screenshot never becomes a
    /// capability. Same per-session cap and eviction as clips.
    pub fn persist_screenshot(&self, session_id: &str, bytes: &[u8]) {
        let id = crate::config::generate_token();
        let _ = self.persist(session_id, &id, bytes);
        self.evict_session(session_id);
    }

    /// Claim one artifact for a fetch. Claim-on-open (pinned): the first caller wins the id;
    /// a consumed id — complete transfer or not — answers exactly like an expired one.
    pub fn open(&self, id: &str) -> Option<MediaHandle> {
        let ttl = self.ttl();
        let mut entries = self.entries.lock().expect("media store lock");
        let entry = entries.get(id)?;
        if entry.created.elapsed() > ttl {
            entries.remove(id);
            return None;
        }
        let entry = entries.remove(id)?;
        Some(MediaHandle {
            content_type: entry.content_type,
            bytes: entry.bytes,
        })
    }

    /// The live TTL: the fixed test value, else `media.ttl` from the core config, else the
    /// pinned default (a config read failure never extends an artifact's life).
    fn ttl(&self) -> Duration {
        self.fixed_ttl
            .or_else(|| {
                self.config
                    .as_deref()
                    .and_then(|path| CoreConfig::load(path).ok())
                    .map(|config| Duration::from_secs(config.media.ttl.secs))
            })
            .unwrap_or(DEFAULT_TTL)
    }

    fn purge_expired(&self) {
        let ttl = self.ttl();
        let mut entries = self.entries.lock().expect("media store lock");
        entries.retain(|_, entry| entry.created.elapsed() <= ttl);
    }

    /// The session's persisted directory: `<data>/media/<session-id>/`.
    fn session_dir(&self, session_id: &str) -> PathBuf {
        self.root.join(session_id)
    }

    fn persist(&self, session_id: &str, id: &str, bytes: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let dir = self.session_dir(session_id);
        std::fs::create_dir_all(&dir)?;
        let _ = std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700));
        let path = dir.join(id);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(bytes)
    }

    /// 08's cap: per session, oldest-first. A failing listing simply skips eviction — the cap
    /// is a bound on growth, not a correctness rule for a fetch.
    fn evict_session(&self, session_id: &str) {
        let dir = self.session_dir(session_id);
        let Ok(read_dir) = std::fs::read_dir(&dir) else {
            return;
        };
        let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
        for entry in read_dir.flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            let modified = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
            files.push((modified, metadata.len(), entry.path()));
        }
        let mut total: u64 = files.iter().map(|(_, len, _)| *len).sum();
        if total <= self.cap_bytes {
            return;
        }
        files.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.2.cmp(&b.2)));
        for (_, len, path) in files {
            if total <= self.cap_bytes {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(len);
            }
        }
    }

    /// Session close/reap: the persisted directory follows the session's life (08).
    pub fn drop_session(&self, session_id: &str) {
        let _ = std::fs::remove_dir_all(self.session_dir(session_id));
    }
}

/// The service-side close hook: remove the session's persisted artifacts. Absent directory is
/// fine (nothing was persisted, or it was already reaped).
pub fn drop_session_dir(paths: &Paths, session_id: &str) {
    let _ = std::fs::remove_dir_all(paths.media_dir().join(session_id));
}

/// The 404 body the media endpoint answers with (and the relay passes through).
pub fn expired_envelope() -> Value {
    serde_json::to_value(ErrorEnvelope::media_expired()).expect("media envelope serializes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn store() -> (tempfile::TempDir, MediaStore) {
        let root = tempfile::tempdir().unwrap();
        let store = MediaStore::with_ttl(root.path().to_path_buf(), DEFAULT_TTL);
        (root, store)
    }

    #[test]
    fn mint_open_consumes_the_id() {
        let (_root, store) = store();
        let id = store.mint("session-1", "video/mp4", b"artifact".to_vec(), false);
        assert_eq!(id.len(), 32);
        assert!(
            id.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        let handle = store.open(&id).expect("first open wins");
        assert_eq!(&*handle.bytes, b"artifact");
        assert_eq!(handle.content_type, "video/mp4");
        assert!(
            store.open(&id).is_none(),
            "consumed ids answer like expired ones"
        );
    }

    #[test]
    fn ttl_expiry_hides_the_id() {
        let root = tempfile::tempdir().unwrap();
        let store = MediaStore::with_ttl(root.path().to_path_buf(), Duration::ZERO);
        let id = store.mint("session-1", "video/mp4", b"x".to_vec(), false);
        assert!(store.open(&id).is_none());
    }

    #[test]
    fn store_clips_persists_and_never_resurrects() {
        let (root, store) = store();
        let bytes = b"clip-bytes".to_vec();
        let id = store.mint("session-9", "video/mp4", bytes.clone(), true);
        let path = root.path().join("session-9").join(&id);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(store.open(&id).is_some(), "the URL still serves once");
        assert!(store.open(&id).is_none(), "and is gone after the fetch");
        assert!(path.exists(), "persistence never resurrects the URL");
        assert!(store.open(&id).is_none());
    }

    #[test]
    fn store_off_leaves_no_file() {
        let (root, store) = store();
        let id = store.mint("session-1", "video/mp4", b"x".to_vec(), false);
        assert!(!root.path().join("session-1").join(id).exists());
    }

    #[test]
    fn eviction_is_oldest_first_per_session() {
        let root = tempfile::tempdir().unwrap();
        let store = MediaStore::with_limits(root.path().to_path_buf(), DEFAULT_TTL, 10);
        let first = store.mint("session-1", "video/mp4", vec![b'a'; 6], true);
        std::thread::sleep(Duration::from_millis(20));
        let second = store.mint("session-1", "video/mp4", vec![b'b'; 6], true);
        // 6 + 6 > 10 → the oldest file is gone, the newest remains.
        assert!(!root.path().join("session-1").join(&first).exists());
        assert!(root.path().join("session-1").join(&second).exists());
    }

    #[test]
    fn drop_session_removes_persisted_artifacts() {
        let (root, store) = store();
        let id = store.mint("session-3", "video/mp4", b"x".to_vec(), true);
        assert!(root.path().join("session-3").join(&id).exists());
        store.drop_session("session-3");
        assert!(!root.path().join("session-3").exists());
    }

    #[test]
    fn ids_are_unique() {
        let (_root, store) = store();
        let a = store.mint("s", "t", b"1".to_vec(), false);
        let b = store.mint("s", "t", b"2".to_vec(), false);
        assert_ne!(a, b);
    }
}
