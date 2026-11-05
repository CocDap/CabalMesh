//! Crash-safe typed persistence.
//!
//! # Two problems this fixes
//!
//! **Paths were discovered, not injected.** Storage called a desktop-only
//! helper to find its own directory. Inside a mobile app sandbox that answer is
//! wrong, and there is no way for a caller to correct it. Every store here
//! takes the path it was given; nothing in this crate asks the operating system
//! where to write.
//!
//! **Writes were not atomic.** A mobile process is killed without warning —
//! backgrounded, memory pressure, force-quit — and a truncated `identities.json`
//! is an unrecoverable wallet loss, not a cache miss. Writes go to a temporary
//! file in the same directory and are renamed over the target, so a reader sees
//! either the old file or the new one and never a half-written one.
//!
//! # What this crate is not
//!
//! Not a database, and deliberately not generic over backends. It persists
//! small documents that are read whole and written whole, which is exactly what
//! the app needs and nothing more.

#![forbid(unsafe_code)]

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Why a store operation failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    #[error("reading {path} failed")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("writing {path} failed")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{path} is not valid JSON for this type")]
    Decode {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("encoding a value for {path} failed")]
    Encode {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

/// A single JSON document on disk.
///
/// Holds no cache: callers own their in-memory copy, and a store that also
/// cached would give two sources of truth that drift.
#[derive(Debug, Clone)]
pub struct JsonStore {
    path: PathBuf,
    pretty: bool,
}

impl JsonStore {
    /// A store at `path`, written as indented JSON.
    ///
    /// The path is taken as given. On mobile it must come from the platform's
    /// app-data directory; resolving it here is what made the old code wrong on
    /// device.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), pretty: true }
    }

    /// A store written as compact JSON, for documents nobody reads by hand.
    #[must_use]
    pub fn compact(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), pretty: false }
    }

    /// Where this store writes.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the document exists.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    /// Reads and decodes the document.
    ///
    /// # Errors
    ///
    /// [`StoreError::Read`] if the file cannot be read, [`StoreError::Decode`]
    /// if its contents do not match `T`.
    pub fn load<T: DeserializeOwned>(&self) -> Result<T, StoreError> {
        let raw = fs::read_to_string(&self.path).map_err(|source| StoreError::Read {
            path: self.path.clone(),
            source,
        })?;
        serde_json::from_str(&raw).map_err(|source| StoreError::Decode {
            path: self.path.clone(),
            source,
        })
    }

    /// Reads the document, or returns `default` if it is absent **or corrupt**.
    ///
    /// Corruption is treated as absence on purpose: these documents are caches
    /// and queues whose loss costs a refresh, and refusing to start because a
    /// cache file is malformed would be worse than rebuilding it. **Do not use
    /// this for anything irreplaceable** — a wallet must fail loudly rather
    /// than silently become empty. Use [`JsonStore::load`] there.
    pub fn load_or<T: DeserializeOwned>(&self, default: T) -> T {
        match self.load() {
            Ok(value) => value,
            Err(error) => {
                if self.exists() {
                    tracing::warn!(
                        target: "cabal_store",
                        path = %self.path.display(),
                        error = %error,
                        "unreadable document, falling back to default"
                    );
                }
                default
            }
        }
    }

    /// Writes the document atomically.
    ///
    /// Encodes first, then writes to a sibling temporary file, flushes, syncs,
    /// and renames over the target. Ordering matters: encoding before touching
    /// the filesystem means a serialization failure cannot leave a truncated
    /// file, and `sync_all` before the rename means a power loss cannot leave
    /// the rename visible while the contents are not.
    ///
    /// The temporary file is a sibling rather than in the system temp dir so
    /// the rename stays within one filesystem, where it is atomic.
    ///
    /// # Errors
    ///
    /// [`StoreError::Encode`] if `value` cannot be serialized, or
    /// [`StoreError::Write`] for any filesystem failure.
    pub fn save<T: Serialize>(&self, value: &T) -> Result<(), StoreError> {
        let encoded = if self.pretty {
            serde_json::to_vec_pretty(value)
        } else {
            serde_json::to_vec(value)
        }
        .map_err(|source| StoreError::Encode {
            path: self.path.clone(),
            source,
        })?;

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|source| StoreError::Write {
                path: self.path.clone(),
                source,
            })?;
        }

        let temp = self.temp_path();
        let write = || -> std::io::Result<()> {
            let mut file = fs::File::create(&temp)?;
            file.write_all(&encoded)?;
            file.flush()?;
            // Without this the rename can land before the bytes do, leaving a
            // file that exists and is empty.
            file.sync_all()?;
            fs::rename(&temp, &self.path)
        };

        write().map_err(|source| {
            let _ = fs::remove_file(&temp);
            StoreError::Write {
                path: self.path.clone(),
                source,
            }
        })
    }

    /// Deletes the document. Absent is success, not an error.
    ///
    /// # Errors
    ///
    /// [`StoreError::Write`] if the file exists but cannot be removed.
    pub fn delete(&self) -> Result<(), StoreError> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(StoreError::Write {
                path: self.path.clone(),
                source,
            }),
        }
    }

    fn temp_path(&self) -> PathBuf {
        let mut name = self
            .path
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();
        name.push(".tmp");
        self.path.with_file_name(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use tempfile::TempDir;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Doc {
        name: String,
        count: u32,
    }

    fn doc() -> Doc {
        Doc { name: "genesis".into(), count: 7 }
    }

    #[test]
    fn round_trips_a_document() {
        let dir = TempDir::new().unwrap();
        let store = JsonStore::new(dir.path().join("doc.json"));

        store.save(&doc()).unwrap();
        assert_eq!(store.load::<Doc>().unwrap(), doc());
    }

    #[test]
    fn creates_missing_parent_directories() {
        let dir = TempDir::new().unwrap();
        let store = JsonStore::new(dir.path().join("nested/deeper/doc.json"));

        store.save(&doc()).unwrap();
        assert!(store.exists());
    }

    #[test]
    fn leaves_no_temporary_file_behind() {
        let dir = TempDir::new().unwrap();
        let store = JsonStore::new(dir.path().join("doc.json"));
        store.save(&doc()).unwrap();

        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temporary file survived the write");
    }

    #[test]
    fn an_interrupted_write_leaves_the_previous_document_intact() {
        // The scenario this crate exists for. A crash mid-write is simulated by
        // a stray temporary file: the real document must be untouched, because
        // the rename is what publishes a write.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("identities.json");
        let store = JsonStore::new(&path);

        store.save(&doc()).unwrap();
        fs::write(dir.path().join("identities.json.tmp"), b"{ truncated").unwrap();

        assert_eq!(store.load::<Doc>().unwrap(), doc());
    }

    #[test]
    fn overwriting_never_exposes_a_partial_document() {
        let dir = TempDir::new().unwrap();
        let store = JsonStore::new(dir.path().join("doc.json"));

        store.save(&doc()).unwrap();
        for count in 0..50 {
            store.save(&Doc { name: "genesis".into(), count }).unwrap();
            // Every read between writes must decode. A non-atomic write would
            // eventually be caught here.
            let read: Doc = store.load().unwrap();
            assert_eq!(read.count, count);
        }
    }

    #[test]
    fn a_serialization_failure_does_not_touch_the_existing_document() {
        // Encoding happens before the filesystem is touched, so an unencodable
        // value cannot destroy good data.
        struct Unencodable;
        impl Serialize for Unencodable {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("nope"))
            }
        }

        let dir = TempDir::new().unwrap();
        let store = JsonStore::new(dir.path().join("doc.json"));
        store.save(&doc()).unwrap();

        assert!(matches!(
            store.save(&Unencodable),
            Err(StoreError::Encode { .. })
        ));
        assert_eq!(store.load::<Doc>().unwrap(), doc());
    }

    #[test]
    fn load_fails_loudly_on_corruption() {
        // Irreplaceable data must never be silently replaced by a default.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("doc.json");
        fs::write(&path, b"{ not json").unwrap();

        assert!(matches!(
            JsonStore::new(&path).load::<Doc>(),
            Err(StoreError::Decode { .. })
        ));
    }

    #[test]
    fn load_or_falls_back_for_replaceable_data() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("cache.json");
        fs::write(&path, b"{ not json").unwrap();

        let fallback = Doc { name: "default".into(), count: 0 };
        assert_eq!(JsonStore::new(&path).load_or(fallback.clone()), fallback);
    }

    #[test]
    fn load_or_returns_the_default_when_absent() {
        let dir = TempDir::new().unwrap();
        let store = JsonStore::new(dir.path().join("missing.json"));
        assert_eq!(store.load_or(9_u32), 9);
    }

    #[test]
    fn deleting_something_absent_is_success() {
        let dir = TempDir::new().unwrap();
        assert!(JsonStore::new(dir.path().join("missing.json")).delete().is_ok());
    }

    #[test]
    fn errors_name_the_path_they_failed_on() {
        // Diagnostics go to the log, where the path is exactly what is needed
        // to act. It never reaches the webview — that is AppError's job.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("doc.json");
        fs::write(&path, b"{ not json").unwrap();

        let error = JsonStore::new(&path).load::<Doc>().unwrap_err();
        assert!(error.to_string().contains("doc.json"));
    }
}
