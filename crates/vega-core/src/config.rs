//! Connection settings: CLI flag, then environment, then `config.toml`.
//! Header values are read from the environment only and are never persisted.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::headers::{self, Header, HeaderError, DEFAULT_KEY_HEADER};

#[derive(Debug, Default, Deserialize)]
pub struct FileConfig {
    pub uri: Option<String>,
}

/// Credentials as read from the environment and flags. It has no `Debug` on purpose.
#[derive(Default)]
pub struct AuthInput {
    /// `VEGA_GRAPHQL_KEY`.
    pub key: Option<String>,
    /// `--key-header`, else `VEGA_GRAPHQL_KEY_HEADER`.
    pub key_header: Option<String>,
    /// `VEGA_GRAPHQL_HEADERS`: one `Name: value` per line.
    pub headers: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub uri: String,
    /// Sent with every request. Values are secrets (`Header`'s `Debug` hides them).
    pub headers: Vec<Header>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error(
        "no GraphQL endpoint configured: pass --uri, set VEGA_GRAPHQL_URI, or add `uri` to config.toml"
    )]
    MissingUri,
    #[error("the key header name is not valid: fix --key-header or VEGA_GRAPHQL_KEY_HEADER")]
    InvalidKeyHeader,
    #[error(transparent)]
    Headers(#[from] HeaderError),
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
    auth: AuthInput,
    file: FileConfig,
) -> Result<Settings, ConfigError> {
    let uri = non_empty(flag_uri)
        .or_else(|| non_empty(env_uri))
        .or_else(|| non_empty(file.uri))
        .ok_or(ConfigError::MissingUri)?;
    let mut headers = Vec::new();
    if let Some(key) = non_empty(auth.key) {
        let name = non_empty(auth.key_header).unwrap_or_else(|| DEFAULT_KEY_HEADER.to_string());
        let header = Header::new(name, key).map_err(|_| ConfigError::InvalidKeyHeader)?;
        headers.push(header);
    }
    if let Some(text) = auth.headers {
        headers = headers::merge(headers, headers::parse_lines(&text)?);
    }
    Ok(Settings { uri, headers })
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
    fn debug_output_hides_the_header_values() {
        let settings = Settings {
            uri: "https://x.example.com/graphql".into(),
            headers: vec![Header::new("X-API-Key", "super-secret").unwrap()],
        };
        let text = format!("{settings:?}");
        assert!(!text.contains("super-secret"), "{text}");
        assert!(text.contains("***"), "{text}");
    }

    fn file(uri: &str) -> FileConfig {
        FileConfig {
            uri: Some(uri.to_string()),
        }
    }

    fn s(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    fn key(v: &str) -> AuthInput {
        AuthInput {
            key: s(v),
            ..AuthInput::default()
        }
    }

    fn auth(key: Option<&str>, key_header: Option<&str>, headers: Option<&str>) -> AuthInput {
        AuthInput {
            key: key.map(str::to_string),
            key_header: key_header.map(str::to_string),
            headers: headers.map(str::to_string),
        }
    }

    fn header(name: &str, value: &str) -> Header {
        Header::new(name, value).unwrap()
    }

    #[test]
    fn the_key_header_name_can_be_changed() {
        let settings = resolve_settings(
            s("u"),
            None,
            auth(Some("k"), Some("X-Token"), None),
            FileConfig::default(),
        )
        .unwrap();
        assert_eq!(settings.headers, vec![header("X-Token", "k")]);
    }

    #[test]
    fn an_empty_key_header_name_falls_back_to_the_default() {
        let settings = resolve_settings(
            s("u"),
            None,
            auth(Some("k"), Some(""), None),
            FileConfig::default(),
        )
        .unwrap();
        assert_eq!(settings.headers, vec![header("X-API-Key", "k")]);
    }

    #[test]
    fn an_invalid_key_header_name_is_an_error_that_hides_the_key() {
        let err = resolve_settings(
            s("u"),
            None,
            auth(Some("super-secret"), Some("Bad Name"), None),
            FileConfig::default(),
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::InvalidKeyHeader));
        assert!(!err.to_string().contains("super-secret"));
    }

    #[test]
    fn headers_from_the_environment_follow_the_key_and_win_on_the_same_name() {
        let settings = resolve_settings(
            s("u"),
            None,
            auth(Some("k"), None, Some("x-api-key: other\nX-Tenant: t")),
            FileConfig::default(),
        )
        .unwrap();
        assert_eq!(
            settings.headers,
            vec![header("x-api-key", "other"), header("X-Tenant", "t")]
        );
    }

    #[test]
    fn headers_work_without_a_key() {
        let settings = resolve_settings(
            s("u"),
            None,
            auth(None, None, Some("Authorization: Bearer abc")),
            FileConfig::default(),
        )
        .unwrap();
        assert_eq!(
            settings.headers,
            vec![header("Authorization", "Bearer abc")]
        );
    }

    #[test]
    fn a_malformed_headers_variable_is_an_error_without_the_value() {
        let err = resolve_settings(
            s("u"),
            None,
            auth(None, None, Some("Authorization Bearer tok-123")),
            FileConfig::default(),
        )
        .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("VEGA_GRAPHQL_HEADERS line 1"), "{message}");
        assert!(!message.contains("tok-123"), "{message}");
    }

    #[test]
    fn flag_beats_env_beats_file() {
        let all =
            resolve_settings(s("flag"), s("env"), AuthInput::default(), file("file")).unwrap();
        assert_eq!(all.uri, "flag");
        let env_file =
            resolve_settings(None, s("env"), AuthInput::default(), file("file")).unwrap();
        assert_eq!(env_file.uri, "env");
        let only_file = resolve_settings(None, None, AuthInput::default(), file("file")).unwrap();
        assert_eq!(only_file.uri, "file");
    }

    #[test]
    fn empty_values_count_as_unset() {
        let settings = resolve_settings(s(""), s("env"), key(""), FileConfig::default()).unwrap();
        assert_eq!(settings.uri, "env");
        assert!(settings.headers.is_empty());
    }

    #[test]
    fn missing_uri_is_an_error_naming_the_ways_to_set_it() {
        let err =
            resolve_settings(None, None, AuthInput::default(), FileConfig::default()).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("--uri"));
        assert!(message.contains("VEGA_GRAPHQL_URI"));
        assert!(message.contains("config.toml"));
    }

    #[test]
    fn the_key_becomes_the_default_header() {
        let settings =
            resolve_settings(s("u"), None, key("secret"), FileConfig::default()).unwrap();
        assert_eq!(
            settings.headers,
            vec![Header::new("X-API-Key", "secret").unwrap()]
        );
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
