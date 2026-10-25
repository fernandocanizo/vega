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
