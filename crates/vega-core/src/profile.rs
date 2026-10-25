//! Saved server profiles. The non-secret part (endpoint, kind of credentials, API key
//! header name) lives in `config.toml`; the secret lives in the OS keyring.

use std::{fs, io, path::Path};

use serde::{Deserialize, Serialize};
use toml_edit::{value, DocumentMut, Item, Table};

use crate::{
    config::{ConfigError, FileConfig, Settings},
    headers::{self, Header, DEFAULT_KEY_HEADER},
    secrets::SecretStore,
};

/// How a profile authenticates; it decides what its keyring secret holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// No credentials, no keyring entry.
    #[serde(rename = "none")]
    NoAuth,
    /// The secret is the API key value.
    #[serde(rename = "api_key")]
    ApiKey,
    /// The secret is `Name: value` lines (the `VEGA_GRAPHQL_HEADERS` format).
    #[serde(rename = "headers")]
    Headers,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::NoAuth => "none",
            Kind::ApiKey => "api_key",
            Kind::Headers => "headers",
        }
    }
}

/// The part of a profile that is safe to write to `config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub uri: String,
    pub kind: Kind,
    /// Header name of an `api_key` profile (a name is not a secret).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_header: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("a profile name may only contain letters, digits, '-' and '_'")]
    InvalidName,
    #[error("no profile named `{name}` (saved: {known})")]
    Unknown { name: String, known: String },
    #[error("the stored credentials are not valid: save the profile again")]
    BadCredentials,
    #[error(transparent)]
    Secret(#[from] crate::secrets::SecretError),
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
    #[error(
        "could not save profile `{name}` and could not restore its keyring entry `vega/{name}`: {source}"
    )]
    Orphan {
        name: String,
        source: Box<crate::config::ConfigError>,
    },
}

/// Names are used as a TOML key and as the keyring user, so they stay plain.
pub fn validate_name(name: &str) -> Result<(), ProfileError> {
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if plain {
        Ok(())
    } else {
        Err(ProfileError::InvalidName)
    }
}

impl Profile {
    /// Splits settings into the part for `config.toml` and the secret for the keyring.
    pub fn from_settings(settings: &Settings) -> (Profile, Option<String>) {
        let uri = settings.uri.clone();
        match settings.headers.as_slice() {
            [] => (
                Profile {
                    uri,
                    kind: Kind::NoAuth,
                    key_header: None,
                },
                None,
            ),
            [key] if settings.key_only => (
                Profile {
                    uri,
                    kind: Kind::ApiKey,
                    key_header: Some(key.name().to_string()),
                },
                Some(key.value().to_string()),
            ),
            all => (
                Profile {
                    uri,
                    kind: Kind::Headers,
                    key_header: None,
                },
                Some(headers::to_lines(all)),
            ),
        }
    }

    /// Rebuilds the settings from the keyring secret. Without a secret there are no
    /// headers.
    pub fn settings(&self, secret: Option<&str>) -> Result<Settings, ProfileError> {
        let mut settings = Settings {
            uri: self.uri.clone(),
            headers: Vec::new(),
            key_only: false,
        };
        let Some(secret) = secret else {
            return Ok(settings);
        };
        match self.kind {
            Kind::NoAuth => {}
            Kind::ApiKey => {
                let name = self.key_header.as_deref().unwrap_or(DEFAULT_KEY_HEADER);
                let header = Header::new(name, secret).map_err(|_| ProfileError::BadCredentials)?;
                settings.headers = vec![header];
                settings.key_only = true;
            }
            Kind::Headers => {
                settings.headers =
                    headers::parse_lines(secret).map_err(|_| ProfileError::BadCredentials)?;
            }
        }
        Ok(settings)
    }
}

/// A profile ready to use.
#[derive(Debug)]
pub struct Connected {
    pub settings: Settings,
    /// The profile needs credentials but the keyring has none for it.
    pub missing_credentials: bool,
}

/// Looks the profile up and reads its secret from `store`.
pub fn connect(
    file: &FileConfig,
    store: &dyn SecretStore,
    name: &str,
) -> Result<Connected, ProfileError> {
    let Some(profile) = file.profiles.get(name) else {
        let known = if file.profiles.is_empty() {
            "none".to_string()
        } else {
            file.profiles.keys().cloned().collect::<Vec<_>>().join(", ")
        };
        return Err(ProfileError::Unknown {
            name: name.to_string(),
            known,
        });
    };
    let secret = match profile.kind {
        Kind::NoAuth => None,
        _ => store.get(name)?,
    };
    let missing_credentials = profile.kind != Kind::NoAuth && secret.is_none();
    Ok(Connected {
        settings: profile.settings(secret.as_deref())?,
        missing_credentials,
    })
}

/// Saves `settings` as profile `name`: the secret to `store` first, then the rest to
/// `<dir>/config.toml`. If the config write fails the keyring is put back as it was.
pub fn save(
    dir: &Path,
    store: &dyn SecretStore,
    name: &str,
    settings: &Settings,
) -> Result<(), ProfileError> {
    validate_name(name)?;
    let (profile, secret) = Profile::from_settings(settings);
    let previous = store.get(name)?;
    match &secret {
        Some(secret) => store.set(name, secret)?,
        None => store.delete(name)?,
    }
    let written = edit_config(dir, |profiles| {
        let mut table = Table::new();
        table["uri"] = value(profile.uri.as_str());
        table["kind"] = value(profile.kind.as_str());
        if let Some(header) = &profile.key_header {
            table["key_header"] = value(header.as_str());
        }
        profiles.insert(name, Item::Table(table));
    });
    if let Err(error) = written {
        let restored = match previous {
            Some(previous) => store.set(name, &previous),
            None => store.delete(name),
        };
        return Err(match restored {
            Ok(()) => error.into(),
            Err(_) => ProfileError::Orphan {
                name: name.to_string(),
                source: Box::new(error),
            },
        });
    }
    Ok(())
}

/// Removes the secret, then the config entry. A profile that is not there is fine.
pub fn delete(dir: &Path, store: &dyn SecretStore, name: &str) -> Result<(), ProfileError> {
    store.delete(name)?;
    edit_config(dir, |profiles| {
        profiles.remove(name);
    })?;
    Ok(())
}

/// Applies `change` to the `[profiles]` table and writes the file back through a
/// temporary file, keeping comments and unrelated entries.
fn edit_config(dir: &Path, change: impl FnOnce(&mut Table)) -> Result<(), ConfigError> {
    let path = dir.join("config.toml");
    let io_error = |source: io::Error| ConfigError::Io {
        path: path.clone(),
        source,
    };
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(io_error(e)),
    };
    let mut document: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
        io_error(io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    })?;
    let profiles = document
        .entry("profiles")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| {
            io_error(io::Error::new(
                io::ErrorKind::InvalidData,
                "`profiles` is not a table",
            ))
        })?;
    // Print `[profiles.name]` without an empty `[profiles]` header above it.
    profiles.set_implicit(true);
    change(profiles);
    fs::create_dir_all(dir).map_err(io_error)?;
    let temporary = dir.join("config.toml.tmp");
    fs::write(&temporary, document.to_string()).map_err(io_error)?;
    fs::rename(&temporary, &path).map_err(io_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::load_file_config;
    use crate::secrets::MemoryStore;

    fn dir_with(config: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), config).unwrap();
        dir
    }

    #[test]
    fn a_saved_profile_connects_back_to_the_same_settings() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::default();
        let start = settings(vec![header("X-Token", "k-1")], true);
        save(dir.path(), &store, "staging", &start).unwrap();

        let file = load_file_config(dir.path()).unwrap();
        let connected = connect(&file, &store, "staging").unwrap();
        assert_eq!(connected.settings, start);
        assert!(!connected.missing_credentials);

        // The secret is in the store and nowhere in the config file.
        let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(!text.contains("k-1"), "{text}");
        assert_eq!(store.get("staging").unwrap().as_deref(), Some("k-1"));
    }

    #[test]
    fn saving_keeps_comments_and_other_entries() {
        let dir = dir_with("# my servers\nuri = \"https://top/graphql\"\n");
        let store = MemoryStore::default();
        save(dir.path(), &store, "a", &settings(vec![], false)).unwrap();
        save(dir.path(), &store, "b", &settings(vec![], false)).unwrap();
        let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(text.contains("# my servers"), "{text}");
        let file = load_file_config(dir.path()).unwrap();
        assert_eq!(file.uri.as_deref(), Some("https://top/graphql"));
        assert_eq!(file.profiles.keys().collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn saving_over_a_profile_replaces_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::default();
        save(
            dir.path(),
            &store,
            "a",
            &settings(vec![header("X-Token", "old")], true),
        )
        .unwrap();
        save(dir.path(), &store, "a", &settings(vec![], false)).unwrap();
        let file = load_file_config(dir.path()).unwrap();
        assert_eq!(file.profiles["a"].kind, Kind::NoAuth);
        assert_eq!(store.get("a").unwrap(), None);
    }

    #[test]
    fn a_failed_keyring_write_saves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::failing_sets();
        let start = settings(vec![header("X-Token", "k")], true);
        assert!(save(dir.path(), &store, "a", &start).is_err());
        assert!(!dir.path().join("config.toml").exists());
    }

    #[test]
    fn a_failed_config_write_restores_the_previous_secret() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where the file should be makes every config write fail.
        std::fs::create_dir(dir.path().join("config.toml")).unwrap();
        let store = MemoryStore::default();
        store.set("a", "old").unwrap();
        let start = settings(vec![header("X-Token", "new")], true);
        assert!(save(dir.path(), &store, "a", &start).is_err());
        assert_eq!(store.get("a").unwrap().as_deref(), Some("old"));
        // With no previous entry the new one is removed again.
        assert!(save(dir.path(), &store, "fresh", &start).is_err());
        assert_eq!(store.get("fresh").unwrap(), None);
    }

    #[test]
    fn an_invalid_name_is_rejected_before_anything_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::default();
        let error = save(dir.path(), &store, "bad name", &settings(vec![], false)).unwrap_err();
        assert!(matches!(error, ProfileError::InvalidName));
        assert!(!dir.path().join("config.toml").exists());
    }

    #[test]
    fn deleting_removes_the_entry_and_the_secret() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::default();
        save(
            dir.path(),
            &store,
            "a",
            &settings(vec![header("X-Token", "k")], true),
        )
        .unwrap();
        delete(dir.path(), &store, "a").unwrap();
        assert!(load_file_config(dir.path()).unwrap().profiles.is_empty());
        assert_eq!(store.get("a").unwrap(), None);
        delete(dir.path(), &store, "a").unwrap(); // already gone
    }

    #[test]
    fn a_failed_secret_delete_keeps_the_profile() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::failing_deletes();
        save(
            dir.path(),
            &store,
            "a",
            &settings(vec![header("X-Token", "k")], true),
        )
        .unwrap();
        assert!(delete(dir.path(), &store, "a").is_err());
        assert!(load_file_config(dir.path())
            .unwrap()
            .profiles
            .contains_key("a"));
    }

    #[test]
    fn an_unknown_profile_lists_the_saved_ones() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::default();
        save(dir.path(), &store, "a", &settings(vec![], false)).unwrap();
        save(dir.path(), &store, "b", &settings(vec![], false)).unwrap();
        let file = load_file_config(dir.path()).unwrap();
        let error = connect(&file, &store, "nope").unwrap_err();
        assert_eq!(error.to_string(), "no profile named `nope` (saved: a, b)");
        let none = crate::config::FileConfig::default();
        assert_eq!(
            connect(&none, &store, "x").unwrap_err().to_string(),
            "no profile named `x` (saved: none)"
        );
    }

    #[test]
    fn a_profile_without_its_keyring_entry_connects_with_no_headers() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::default();
        save(
            dir.path(),
            &store,
            "a",
            &settings(vec![header("X-Token", "k")], true),
        )
        .unwrap();
        store.delete("a").unwrap();
        let file = load_file_config(dir.path()).unwrap();
        let connected = connect(&file, &store, "a").unwrap();
        assert!(connected.settings.headers.is_empty());
        assert!(connected.missing_credentials);
    }

    fn header(name: &str, value: &str) -> Header {
        Header::new(name, value).unwrap()
    }

    fn settings(headers: Vec<Header>, key_only: bool) -> Settings {
        Settings {
            uri: "https://x/graphql".to_string(),
            headers,
            key_only,
        }
    }

    #[test]
    fn names_are_letters_digits_dash_and_underscore() {
        assert!(validate_name("staging-2_b").is_ok());
        for bad in ["", "a b", "a.b", "a/b", "é", "a\"b"] {
            assert!(
                matches!(validate_name(bad), Err(ProfileError::InvalidName)),
                "{bad}"
            );
        }
    }

    #[test]
    fn no_headers_is_a_profile_without_a_secret() {
        let start = settings(vec![], false);
        let (profile, secret) = Profile::from_settings(&start);
        assert_eq!(profile.kind, Kind::NoAuth);
        assert_eq!(secret, None);
        assert_eq!(profile.settings(None).unwrap(), start);
    }

    #[test]
    fn the_api_key_alone_keeps_its_header_name_on_disk_and_its_value_in_the_secret() {
        let start = settings(vec![header("X-Token", "k-1")], true);
        let (profile, secret) = Profile::from_settings(&start);
        assert_eq!(profile.kind, Kind::ApiKey);
        assert_eq!(profile.key_header.as_deref(), Some("X-Token"));
        assert_eq!(secret.as_deref(), Some("k-1"));
        assert_eq!(profile.settings(secret.as_deref()).unwrap(), start);
    }

    #[test]
    fn other_headers_go_whole_into_the_secret() {
        let start = settings(
            vec![header("Authorization", "Bearer t"), header("X-Org", "acme")],
            false,
        );
        let (profile, secret) = Profile::from_settings(&start);
        assert_eq!(profile.kind, Kind::Headers);
        assert_eq!(profile.key_header, None);
        assert_eq!(
            secret.as_deref(),
            Some("Authorization: Bearer t\nX-Org: acme")
        );
        assert_eq!(profile.settings(secret.as_deref()).unwrap(), start);
    }

    #[test]
    fn a_missing_secret_gives_no_headers() {
        let (profile, _) = Profile::from_settings(&settings(vec![header("X-Token", "k")], true));
        assert_eq!(profile.settings(None).unwrap(), settings(vec![], false));
    }

    #[test]
    fn a_broken_secret_is_an_error_that_does_not_echo_it() {
        let (profile, _) =
            Profile::from_settings(&settings(vec![header("A", "1"), header("B", "2")], false));
        let error = profile.settings(Some("no colon here")).unwrap_err();
        assert!(matches!(error, ProfileError::BadCredentials));
        assert!(!error.to_string().contains("no colon"));
    }

    #[test]
    fn profiles_are_read_from_the_config_table() {
        let file: crate::config::FileConfig = toml::from_str(
            "uri = \"https://top/graphql\"\n\
             [profiles.staging]\n\
             uri = \"https://s/graphql\"\n\
             kind = \"api_key\"\n\
             key_header = \"X-Token\"\n\
             [profiles.open]\n\
             uri = \"https://o/graphql\"\n\
             kind = \"none\"\n",
        )
        .unwrap();
        assert_eq!(file.uri.as_deref(), Some("https://top/graphql"));
        assert_eq!(file.profiles["staging"].kind, Kind::ApiKey);
        assert_eq!(
            file.profiles["staging"].key_header.as_deref(),
            Some("X-Token")
        );
        assert_eq!(file.profiles["open"].kind, Kind::NoAuth);
    }

    #[test]
    fn a_config_without_profiles_still_loads() {
        let file: crate::config::FileConfig =
            toml::from_str("uri = \"https://x/graphql\"\n").unwrap();
        assert!(file.profiles.is_empty());
    }
}
