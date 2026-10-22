//! On-disk schema cache, one file per endpoint.

use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::schema::IntrospectionResponse;

/// A cache older than this (strictly) is stale: 24 hours.
pub const STALE_AFTER_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("cache I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cache file {path} is corrupt: {source}")]
    Corrupt {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("cannot serialize cache entry: {0}")]
    Serialize(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub endpoint: String,
    /// Unix seconds, stored inside the file because mtime changes on copy.
    pub fetched_at: u64,
    pub response: IntrospectionResponse,
}

impl CacheEntry {
    pub fn age_secs(&self, now: u64) -> u64 {
        now.saturating_sub(self.fetched_at)
    }
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn is_stale(age_secs: u64) -> bool {
    age_secs > STALE_AFTER_SECS
}

/// Coarse human-readable age: `<1m`, `5m`, `3h`, `2d`.
pub fn format_age(age_secs: u64) -> String {
    match age_secs {
        0..=59 => "<1m".to_string(),
        60..=3_599 => format!("{}m", age_secs / 60),
        3_600..=86_399 => format!("{}h", age_secs / 3_600),
        _ => format!("{}d", age_secs / 86_400),
    }
}

/// FNV-1a, 64 bit. Hand-rolled because `DefaultHasher` is not stable across Rust versions.
fn fnv1a(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// `<dir>/<readable-endpoint>-<hash>.json`. The API key is never part of this.
pub fn cache_path(dir: &Path, endpoint: &str) -> PathBuf {
    let readable: String = endpoint
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(48)
        .collect();
    dir.join(format!("{readable}-{:016x}.json", fnv1a(endpoint)))
}

/// `Ok(None)` when no cache file exists yet.
pub fn load(path: &Path) -> Result<Option<CacheEntry>, CacheError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(CacheError::Io {
                path: path.to_path_buf(),
                source,
            })
        }
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|source| CacheError::Corrupt {
            path: path.to_path_buf(),
            source,
        })
}

/// Writes to a temp file and renames, so a crash never leaves a half-written cache.
pub fn store(path: &Path, entry: &CacheEntry) -> Result<(), CacheError> {
    let io_err = |source: io::Error| CacheError::Io {
        path: path.to_path_buf(),
        source,
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io_err)?;
    }
    let json = serde_json::to_string(entry)?;
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    fs::write(&tmp, json).map_err(io_err)?;
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(io_err(e));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::fixture_response;

    fn entry(fetched_at: u64) -> CacheEntry {
        CacheEntry {
            endpoint: "https://nexus.example.com/graphql".into(),
            fetched_at,
            response: fixture_response(),
        }
    }

    #[test]
    fn store_leaves_no_temp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        store(&dir.path().join("c.json"), &entry(1)).unwrap();
        let names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["c.json".to_string()]);
    }

    #[test]
    fn store_then_load_round_trips_and_creates_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("vega").join("c.json");
        store(&path, &entry(1_000)).unwrap();
        let loaded = load(&path).unwrap().unwrap();
        assert_eq!(loaded.fetched_at, 1_000);
        assert_eq!(loaded.endpoint, "https://nexus.example.com/graphql");
        assert_eq!(loaded.response.data.schema.types.len(), 10);
    }

    #[test]
    fn load_of_a_missing_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&dir.path().join("absent.json")).unwrap().is_none());
    }

    #[test]
    fn load_of_garbage_is_a_corrupt_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(matches!(load(&path), Err(CacheError::Corrupt { .. })));
    }

    #[test]
    fn store_replaces_the_previous_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        store(&path, &entry(1)).unwrap();
        store(&path, &entry(2)).unwrap();
        assert_eq!(load(&path).unwrap().unwrap().fetched_at, 2);
    }

    #[test]
    fn cache_path_is_stable_per_endpoint_and_filesystem_safe() {
        let dir = Path::new("/c");
        let a = cache_path(dir, "https://a.example.com/graphql");
        assert_eq!(a, cache_path(dir, "https://a.example.com/graphql"));
        assert_ne!(a, cache_path(dir, "https://b.example.com/graphql"));
        assert_eq!(a.parent(), Some(dir));
        let name = a.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("https___a_example_com_graphql-"));
        assert!(name.ends_with(".json"));
        assert!(!name.contains('/'));
    }

    #[test]
    fn fnv1a_of_empty_input_is_the_offset_basis() {
        assert_eq!(fnv1a(""), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn age_and_staleness() {
        let e = entry(1_000);
        assert_eq!(e.age_secs(1_060), 60);
        assert_eq!(e.age_secs(500), 0);
        assert!(!is_stale(STALE_AFTER_SECS));
        assert!(is_stale(STALE_AFTER_SECS + 1));
    }

    #[test]
    fn format_age_buckets() {
        assert_eq!(format_age(0), "<1m");
        assert_eq!(format_age(59), "<1m");
        assert_eq!(format_age(60), "1m");
        assert_eq!(format_age(59 * 60), "59m");
        assert_eq!(format_age(3_600), "1h");
        assert_eq!(format_age(3 * 3_600 + 59), "3h");
        assert_eq!(format_age(86_400), "1d");
        assert_eq!(format_age(2 * 86_400 + 7_200), "2d");
    }
}
