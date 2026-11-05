//! Implicit-account keys: the account id is the hex of the ed25519 public key.

use ed25519_dalek::{Signer, SigningKey};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::fmt;
use zeroize::Zeroize;

/// An account id that follows NEAR's rules: 2 to 64 characters, lowercase
/// alphanumeric parts separated by a single `.`, `-` or `_`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AccountId(String);

/// A string that is not a valid NEAR account id.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid NEAR account id")]
pub struct InvalidAccountId;

impl AccountId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<AccountId> for String {
    fn from(id: AccountId) -> Self {
        id.0
    }
}

impl TryFrom<&str> for AccountId {
    type Error = InvalidAccountId;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if !(2..=64).contains(&value.len()) {
            return Err(InvalidAccountId);
        }
        let mut previous_was_separator = true;
        for byte in value.bytes() {
            match byte {
                b'a'..=b'z' | b'0'..=b'9' => previous_was_separator = false,
                b'.' | b'-' | b'_' if !previous_was_separator => previous_was_separator = true,
                _ => return Err(InvalidAccountId),
            }
        }
        if previous_was_separator {
            return Err(InvalidAccountId);
        }
        Ok(Self(value.to_owned()))
    }
}

impl TryFrom<String> for AccountId {
    type Error = InvalidAccountId;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(value.as_str())
    }
}

/// A device's NEAR key. Zeroized on drop by `SigningKey` itself.
#[derive(Clone)]
pub struct NearKeypair(SigningKey);

impl NearKeypair {
    /// A new random key.
    #[must_use]
    pub fn generate() -> Self {
        let mut secret = [0u8; 32];
        rand::rng().fill_bytes(&mut secret);
        let keypair = Self::from_secret_bytes(&secret);
        secret.zeroize();
        keypair
    }

    #[must_use]
    pub fn from_secret_bytes(secret: &[u8; 32]) -> Self {
        Self(SigningKey::from_bytes(secret))
    }

    #[must_use]
    pub fn secret_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    #[must_use]
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.0.verifying_key().to_bytes()
    }

    /// The implicit account: lowercase hex of the 32-byte public key.
    #[must_use]
    pub fn account_id(&self) -> AccountId {
        AccountId(hex::encode(self.public_key_bytes()))
    }

    /// `ed25519:<base58>`.
    #[must_use]
    pub fn public_key_string(&self) -> String {
        format!("ed25519:{}", bs58::encode(self.public_key_bytes()).into_string())
    }

    pub(crate) fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.0.sign(message).to_bytes()
    }
}

impl fmt::Debug for NearKeypair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("NearKeypair").field(&self.account_id().as_str()).finish()
    }
}
