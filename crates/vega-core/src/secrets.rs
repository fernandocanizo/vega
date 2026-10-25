//! Where profile secrets are kept.

#[derive(Debug, Clone, thiserror::Error)]
pub enum SecretError {
    #[error("no keyring is available on this system")]
    Unavailable,
    #[error("the keyring refused access (locked or denied)")]
    Denied,
    #[error("keyring error: {0}")]
    Backend(String),
}

/// Every Vega secret lives under this keyring service; the user is the profile name.
pub const SERVICE: &str = "vega";

pub trait SecretStore {
    /// `None` when there is no entry for `name`.
    fn get(&self, name: &str) -> Result<Option<String>, SecretError>;
    fn set(&self, name: &str, secret: &str) -> Result<(), SecretError>;
    /// A missing entry is not an error.
    fn delete(&self, name: &str) -> Result<(), SecretError>;
}

/// The operating system keyring (Secret Service, Keychain or Credential Manager).
pub struct KeyringStore;

fn map_error(error: keyring::Error) -> SecretError {
    match error {
        keyring::Error::NoDefaultStore => SecretError::Unavailable,
        keyring::Error::NoStorageAccess(_) => SecretError::Denied,
        // `keyring` errors describe the failure, never the secret.
        other => SecretError::Backend(other.to_string()),
    }
}

impl KeyringStore {
    fn entry(name: &str) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(SERVICE, name).map_err(map_error)
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, name: &str) -> Result<Option<String>, SecretError> {
        match Self::entry(name)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(map_error(error)),
        }
    }

    fn set(&self, name: &str, secret: &str) -> Result<(), SecretError> {
        Self::entry(name)?.set_password(secret).map_err(map_error)
    }

    fn delete(&self, name: &str) -> Result<(), SecretError> {
        match Self::entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(map_error(error)),
        }
    }
}

/// An in-memory store for tests, with switches to make writes fail.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore {
    secrets: std::sync::Mutex<std::collections::HashMap<String, String>>,
    fail_sets: bool,
    fail_deletes: bool,
}

#[cfg(test)]
impl MemoryStore {
    pub fn failing_sets() -> Self {
        Self {
            fail_sets: true,
            ..Self::default()
        }
    }

    pub fn failing_deletes() -> Self {
        Self {
            fail_deletes: true,
            ..Self::default()
        }
    }

    fn map(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<String, String>> {
        self.secrets.lock().unwrap_or_else(|p| p.into_inner())
    }
}

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn get(&self, name: &str) -> Result<Option<String>, SecretError> {
        Ok(self.map().get(name).cloned())
    }

    fn set(&self, name: &str, secret: &str) -> Result<(), SecretError> {
        if self.fail_sets {
            return Err(SecretError::Backend("injected".to_string()));
        }
        self.map().insert(name.to_string(), secret.to_string());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), SecretError> {
        if self.fail_deletes {
            return Err(SecretError::Backend("injected".to_string()));
        }
        self.map().remove(name);
        Ok(())
    }
}

/// A store for tests whose every call says there is no keyring.
#[cfg(test)]
pub struct UnavailableStore;

#[cfg(test)]
impl SecretStore for UnavailableStore {
    fn get(&self, _name: &str) -> Result<Option<String>, SecretError> {
        Err(SecretError::Unavailable)
    }

    fn set(&self, _name: &str, _secret: &str) -> Result<(), SecretError> {
        Err(SecretError::Unavailable)
    }

    fn delete(&self, _name: &str) -> Result<(), SecretError> {
        Err(SecretError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_memory_store_round_trips_and_forgets() {
        let store = MemoryStore::default();
        assert_eq!(store.get("a").unwrap(), None);
        store.set("a", "one").unwrap();
        assert_eq!(store.get("a").unwrap().as_deref(), Some("one"));
        store.set("a", "two").unwrap();
        assert_eq!(store.get("a").unwrap().as_deref(), Some("two"));
        store.delete("a").unwrap();
        assert_eq!(store.get("a").unwrap(), None);
        store.delete("a").unwrap(); // a missing entry is not an error
    }

    #[test]
    fn the_memory_store_can_fail_on_demand() {
        assert!(MemoryStore::failing_sets().set("a", "x").is_err());
        assert!(MemoryStore::failing_deletes().delete("a").is_err());
    }

    #[test]
    fn keyring_errors_map_without_echoing_data() {
        assert!(matches!(
            map_error(keyring::Error::NoDefaultStore),
            SecretError::Unavailable
        ));
        assert!(matches!(
            map_error(keyring::Error::BadEncoding(b"secret".to_vec())),
            SecretError::Backend(message) if !message.contains("secret")
        ));
    }

    /// Needs a running Secret Service / Keychain / Credential Manager:
    /// `cargo test -p vega-core -- --ignored keyring_round_trip`.
    #[test]
    #[ignore = "uses the real OS keyring"]
    fn keyring_round_trip() {
        let store = KeyringStore;
        let name = format!("vega-test-{}", std::process::id());
        store.set(&name, "s3cret").unwrap();
        assert_eq!(store.get(&name).unwrap().as_deref(), Some("s3cret"));
        store.delete(&name).unwrap();
        assert_eq!(store.get(&name).unwrap(), None);
    }
}
