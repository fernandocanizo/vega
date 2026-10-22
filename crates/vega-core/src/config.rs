//! Connection settings: CLI flag, then environment, then `config.toml`.
//! The API key is read from the environment only and is never persisted.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub struct FileConfig {
    pub uri: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Settings {
    pub uri: String,
    pub api_key: Option<String>,
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Settings")
            .field("uri", &self.uri)
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error(
        "no GraphQL endpoint configured: pass --uri, set VEGA_GRAPHQL_URI, or add `uri` to config.toml"
    )]
    MissingUri,
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid config file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.is_empty())
}

pub fn resolve_settings(
    flag_uri: Option<String>,
    env_uri: Option<String>,
    env_key: Option<String>,
    file: FileConfig,
) -> Result<Settings, ConfigError> {
    let uri = non_empty(flag_uri)
        .or_else(|| non_empty(env_uri))
        .or_else(|| non_empty(file.uri))
        .ok_or(ConfigError::MissingUri)?;
    Ok(Settings {
        uri,
        api_key: non_empty(env_key),
    })
}

/// Reads `<config_dir>/config.toml`; a missing file is not an error.
pub fn load_file_config(config_dir: &Path) -> Result<FileConfig, ConfigError> {
    let path = config_dir.join("config.toml");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(FileConfig::default()),
        Err(source) => return Err(ConfigError::Io { path, source }),
    };
    toml::from_str(&text).map_err(|source| ConfigError::Parse { path, source })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_hides_the_api_key() {
        let settings = Settings {
            uri: "https://x.example.com/graphql".into(),
            api_key: Some("super-secret".into()),
        };
        let text = format!("{settings:?}");
        assert!(!text.contains("super-secret"), "{text}");
        assert!(text.contains("Some(\"***\")"), "{text}");
    }

    fn file(uri: &str) -> FileConfig {
        FileConfig {
            uri: Some(uri.to_string()),
        }
    }

    fn s(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    #[test]
    fn flag_beats_env_beats_file() {
        let all = resolve_settings(s("flag"), s("env"), None, file("file")).unwrap();
        assert_eq!(all.uri, "flag");
        let env_file = resolve_settings(None, s("env"), None, file("file")).unwrap();
        assert_eq!(env_file.uri, "env");
        let only_file = resolve_settings(None, None, None, file("file")).unwrap();
        assert_eq!(only_file.uri, "file");
    }

    #[test]
    fn empty_values_count_as_unset() {
        let settings = resolve_settings(s(""), s("env"), s(""), FileConfig::default()).unwrap();
        assert_eq!(settings.uri, "env");
        assert_eq!(settings.api_key, None);
    }

    #[test]
    fn missing_uri_is_an_error_naming_the_ways_to_set_it() {
        let err = resolve_settings(None, None, None, FileConfig::default()).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("--uri"));
        assert!(message.contains("VEGA_GRAPHQL_URI"));
        assert!(message.contains("config.toml"));
    }

    #[test]
    fn api_key_comes_from_the_environment_value() {
        let settings = resolve_settings(s("u"), None, s("secret"), FileConfig::default()).unwrap();
        assert_eq!(settings.api_key.as_deref(), Some("secret"));
    }

    #[test]
    fn missing_config_file_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_file_config(dir.path()).unwrap().uri, None);
    }

    #[test]
    fn config_file_uri_is_read() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "uri = \"https://x/graphql\"\n",
        )
        .unwrap();
        assert_eq!(
            load_file_config(dir.path()).unwrap().uri.as_deref(),
            Some("https://x/graphql")
        );
    }

    #[test]
    fn invalid_config_file_is_a_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "uri = [").unwrap();
        assert!(matches!(
            load_file_config(dir.path()),
            Err(ConfigError::Parse { .. })
        ));
    }
}
