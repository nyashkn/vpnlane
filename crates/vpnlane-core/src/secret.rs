use std::fmt;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::Error;

/// Provider access token. Zeroized on drop; `Debug` never prints it.
#[derive(Clone)]
pub struct Token(Zeroizing<String>);

impl Token {
    /// Validates and wraps raw token text (surrounding whitespace is trimmed).
    ///
    /// Only ASCII alphanumerics are accepted: the token ends up in an HTTP
    /// header, so anything else is either a paste error or an injection.
    pub fn new(raw: &str) -> Result<Self, Error> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(Error::TokenMalformed("empty"));
        }
        if !raw.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(Error::TokenMalformed("only letters and digits are allowed"));
        }
        Ok(Self(Zeroizing::new(raw.to_owned())))
    }

    /// The secret itself. Call only where it leaves the process (HTTP auth).
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

/// WireGuard private key (base64). Zeroized on drop; `Debug` never prints it.
#[derive(Clone)]
pub struct PrivateKey(Zeroizing<String>);

impl PrivateKey {
    /// Wraps a base64 key as returned by the provider.
    pub fn new(base64: impl Into<String>) -> Self {
        Self(Zeroizing::new(base64.into()))
    }

    /// The key text, for rendering into a config.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PrivateKey(<redacted>)")
    }
}

/// WireGuard public key (base64). Not secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicKey(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_trims_and_validates() {
        assert_eq!(Token::new("  abc123\n").unwrap().expose(), "abc123");
        assert!(Token::new("   ").is_err());
        assert!(Token::new("abc\r\nX-Evil: 1").is_err());
    }

    #[test]
    fn debug_redacts_secrets() {
        let t = Token::new("deadbeef").unwrap();
        let k = PrivateKey::new("c2VjcmV0");
        assert!(!format!("{t:?}{k:?}").contains("deadbeef"));
        assert!(!format!("{k:?}").contains("c2VjcmV0"));
    }
}
