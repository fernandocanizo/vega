//! Connection settings: CLI flag, then environment, then `config.toml`.
//! Header values are read from the environment only and are never persisted.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::{
    headers::{self, Header, HeaderError, DEFAULT_KEY_HEADER},
    profile::Profile,
};

#[derive(Debug, Default, Deserialize)]
pub struct FileConfig {
    pub uri: Option<String>,
    /// Saved servers. The credentials of a profile are in the OS keyring, not here.
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
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
    /// The endpoint; empty when none is configured yet (only the TUI allows that).
    pub uri: String,
    /// Sent with every request. Values are secrets (`Header`'s `Debug` hides them).
    pub headers: Vec<Header>,
    /// True when `headers` is exactly the single API key header, which the connection
    /// modal shows on its API Key form.
    pub key_only: bool,
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

/// Like `resolve_settings`, but a missing endpoint is not an error: `uri` is empty.
pub fn resolve_settings_partial(
    flag_uri: Option<String>,
    env_uri: Option<String>,
    auth: AuthInput,
    file: FileConfig,
) -> Result<Settings, ConfigError> {
    let uri = [flag_uri, env_uri, file.uri]
        .into_iter()
        .flatten()
        .map(|uri| uri.trim().to_string())
        .find(|uri| !uri.is_empty())
        .unwrap_or_default();
    let mut headers = Vec::new();
    let mut key_only = false;
    if let Some(key) = non_empty(auth.key) {
        let name = non_empty(auth.key_header).unwrap_or_else(|| DEFAULT_KEY_HEADER.to_string());
        let header = Header::new(name, key).map_err(|_| ConfigError::InvalidKeyHeader)?;
        headers.push(header);
        key_only = true;
    }
    if let Some(text) = auth.headers {
        let extra = headers::parse_lines(&text)?;
        if !extra.is_empty() {
            key_only = false;
            headers = headers::merge(headers, extra);
        }
    }
    Ok(Settings {
        uri,
        headers,
        key_only,
    })
}

pub fn resolve_settings(
    flag_uri: Option<String>,
    env_uri: Option<String>,
    auth: AuthInput,
    file: FileConfig,
) -> Result<Settings, ConfigError> {
    let settings = resolve_settings_partial(flag_uri, env_uri, auth, file)?;
    if settings.uri.is_empty() {
        return Err(ConfigError::MissingUri);
    }
    Ok(settings)
}

/// The outcome of layering a profile under the flags and the environment.
#[derive(Debug, PartialEq)]
pub struct WithProfile {
    pub settings: Settings,
    /// The profile's credentials were left out because the endpoint in use is not the
    /// profile's own.
    pub credentials_withheld: bool,
}

/// Whether two URIs share scheme, host and port (default ports are normalized).
fn same_origin(a: &str, b: &str) -> bool {
    match (reqwest::Url::parse(a), reqwest::Url::parse(b)) {
        (Ok(a), Ok(b)) => a.origin() == b.origin(),
        _ => false,
    }
}

/// Layers a saved profile under the flags and the environment: the profile's endpoint
/// applies when no flag or variable names one. Its credentials apply only when the
/// endpoint in use has the profile's origin, and the environment's credentials (those
/// replace them entirely) are never affected.
pub fn with_profile(settings: Settings, profile: Settings, env_credentials: bool) -> WithProfile {
    let uri = if settings.uri.is_empty() {
        profile.uri.clone()
    } else {
        settings.uri
    };
    if env_credentials {
        return WithProfile {
            settings: Settings {
                uri,
                headers: settings.headers,
                key_only: settings.key_only,
            },
            credentials_withheld: false,
        };
    }
    if same_origin(&uri, &profile.uri) {
        WithProfile {
            settings: Settings {
                uri,
                headers: profile.headers,
                key_only: profile.key_only,
            },
            credentials_withheld: false,
        }
    } else {
        WithProfile {
            settings: Settings {
                uri,
                headers: Vec::new(),
                key_only: false,
            },
            credentials_withheld: !profile.headers.is_empty(),
        }
    }
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
            key_only: true,
        };
        let text = format!("{settings:?}");
        assert!(!text.contains("super-secret"), "{text}");
        assert!(text.contains("***"), "{text}");
    }

    fn file(uri: &str) -> FileConfig {
        FileConfig {
            uri: Some(uri.to_string()),
            ..Default::default()
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
    fn key_only_is_true_for_the_key_alone_and_false_otherwise() {
        let key_only = resolve_settings(
            s("u"),
            None,
            auth(Some("k"), None, None),
            FileConfig::default(),
        )
        .unwrap();
        assert!(key_only.key_only);

        let with_headers = resolve_settings(
            s("u"),
            None,
            auth(Some("k"), None, Some("X-Tenant: t")),
            FileConfig::default(),
        )
        .unwrap();
        assert!(!with_headers.key_only);

        let headers_only = resolve_settings(
            s("u"),
            None,
            auth(None, None, Some("X-Tenant: t")),
            FileConfig::default(),
        )
        .unwrap();
        assert!(!headers_only.key_only);

        let nothing =
            resolve_settings(s("u"), None, auth(None, None, None), FileConfig::default()).unwrap();
        assert!(!nothing.key_only);
        assert!(nothing.headers.is_empty());
    }

    #[test]
    fn an_empty_headers_variable_keeps_the_key_only() {
        let settings = resolve_settings(
            s("u"),
            None,
            auth(Some("k"), None, Some("\n")),
            FileConfig::default(),
        )
        .unwrap();
        assert!(settings.key_only);
    }

    #[test]
    fn partial_settings_allow_a_missing_uri_and_keep_the_headers() {
        let settings = resolve_settings_partial(
            None,
            None,
            auth(Some("k"), None, None),
            FileConfig::default(),
        )
        .unwrap();
        assert_eq!(settings.uri, "");
        assert_eq!(settings.headers, vec![header("X-API-Key", "k")]);
        assert!(settings.key_only);
    }

    #[test]
    fn partial_settings_still_reject_malformed_headers() {
        let err = resolve_settings_partial(
            None,
            None,
            auth(None, None, Some("no colon here")),
            FileConfig::default(),
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::Headers(_)));
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
    fn the_endpoint_is_trimmed_and_a_blank_one_counts_as_unset() {
        let settings = resolve_settings(
            s("  "),
            s(" http://env/graphql \n"),
            key(""),
            FileConfig::default(),
        )
        .unwrap();
        assert_eq!(settings.uri, "http://env/graphql");
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

    fn profile_settings(kind_only: bool, headers: Vec<Header>) -> Settings {
        Settings {
            uri: "https://p/graphql".to_string(),
            headers,
            key_only: kind_only,
        }
    }

    #[test]
    fn a_profile_fills_the_endpoint_and_credentials_the_flags_leave_open() {
        let flags =
            resolve_settings_partial(None, None, AuthInput::default(), FileConfig::default())
                .unwrap();
        let profile = profile_settings(true, vec![header("X-Token", "k")]);
        let merged = with_profile(flags, profile.clone(), false);
        assert_eq!(merged.settings, profile);
        assert!(!merged.credentials_withheld);
    }

    fn flag_uri(uri: &str) -> Settings {
        resolve_settings_partial(s(uri), None, AuthInput::default(), FileConfig::default()).unwrap()
    }

    fn merged_with_flag(uri: &str, profile_uri: &str) -> WithProfile {
        let mut profile = profile_settings(true, vec![header("X-Token", "k")]);
        profile.uri = profile_uri.to_string();
        with_profile(flag_uri(uri), profile, false)
    }

    #[test]
    fn the_uri_flag_beats_the_profile_endpoint() {
        let merged = merged_with_flag("https://p/other", "https://p/graphql");
        assert_eq!(merged.settings.uri, "https://p/other");
        assert_eq!(merged.settings.headers, vec![header("X-Token", "k")]);
        assert!(merged.settings.key_only);
        assert!(!merged.credentials_withheld);
    }

    #[test]
    fn credentials_are_withheld_from_another_origin() {
        for flag in [
            "https://elsewhere/graphql",
            "http://p/graphql",
            "https://p:8443/graphql",
            "not a uri",
        ] {
            let merged = merged_with_flag(flag, "https://p/graphql");
            assert_eq!(merged.settings.uri, flag);
            assert!(merged.settings.headers.is_empty(), "{flag}");
            assert!(!merged.settings.key_only, "{flag}");
            assert!(merged.credentials_withheld, "{flag}");
        }
    }

    #[test]
    fn default_ports_count_as_the_same_origin() {
        let merged = merged_with_flag("https://p:443/x", "https://p/graphql");
        assert_eq!(merged.settings.headers, vec![header("X-Token", "k")]);
    }

    #[test]
    fn an_unparseable_profile_uri_withholds_the_credentials() {
        let merged = merged_with_flag("https://p/graphql", "nonsense");
        assert!(merged.settings.headers.is_empty());
        assert!(merged.credentials_withheld);
    }

    #[test]
    fn a_profile_without_credentials_withholds_nothing() {
        let mut profile = profile_settings(false, vec![]);
        profile.uri = "https://p/graphql".to_string();
        let merged = with_profile(flag_uri("https://elsewhere/graphql"), profile, false);
        assert!(!merged.credentials_withheld);
    }

    #[test]
    fn environment_credentials_replace_the_profile_ones() {
        let env = resolve_settings_partial(
            None,
            None,
            auth(Some("env-key"), None, None),
            FileConfig::default(),
        )
        .unwrap();
        let profile = profile_settings(false, vec![header("Authorization", "Bearer p")]);
        let merged = with_profile(env.clone(), profile, true);
        assert_eq!(merged.settings.uri, "https://p/graphql");
        assert_eq!(merged.settings.headers, env.headers);
        assert!(merged.settings.key_only);
        assert!(!merged.credentials_withheld);
    }

    #[test]
    fn environment_credentials_survive_another_endpoint() {
        let env = resolve_settings_partial(
            s("https://elsewhere/graphql"),
            None,
            auth(Some("env-key"), None, None),
            FileConfig::default(),
        )
        .unwrap();
        let profile = profile_settings(false, vec![header("Authorization", "Bearer p")]);
        let merged = with_profile(env.clone(), profile, true);
        assert_eq!(merged.settings, env);
        assert!(!merged.credentials_withheld);
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
