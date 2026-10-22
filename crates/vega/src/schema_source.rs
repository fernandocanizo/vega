//! Where the schema comes from: the cache, or a fresh introspection fetch.

use std::path::Path;

use anyhow::Context;
use vega_core::cache::{self, CacheEntry};
use vega_core::client;
use vega_core::config::Settings;

/// Use the cache unless `refresh` is set or no usable cache exists. A failed fetch
/// returns an error and leaves any existing cache file untouched.
pub fn load_or_fetch(
    settings: &Settings,
    cache_file: &Path,
    refresh: bool,
) -> anyhow::Result<CacheEntry> {
    if !refresh {
        match cache::load(cache_file) {
            Ok(Some(entry)) => return Ok(entry),
            Ok(None) => {}
            Err(e) => eprintln!("warning: ignoring unusable cache ({e}); fetching a fresh schema"),
        }
    }
    let entry = fetch_entry(settings)?;
    cache::store(cache_file, &entry).context("could not write the schema cache")?;
    Ok(entry)
}

pub fn fetch_entry(settings: &Settings) -> anyhow::Result<CacheEntry> {
    let response = client::fetch_introspection(&settings.uri, settings.api_key.as_deref())
        .with_context(|| format!("fetching the schema from {}", settings.uri))?;
    Ok(CacheEntry {
        endpoint: settings.uri.clone(),
        fetched_at: cache::now_secs(),
        response,
    })
}
