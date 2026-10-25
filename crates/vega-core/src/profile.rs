//! Saved server profiles. The non-secret part (endpoint, kind of credentials, API key
//! header name) lives in `config.toml`; the secret lives in the OS keyring.

use serde::{Deserialize, Serialize};

use crate::{
    config::Settings,
    headers::{self, Header, DEFAULT_KEY_HEADER},
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

#[cfg(test)]
mod tests {
    use super::*;

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
