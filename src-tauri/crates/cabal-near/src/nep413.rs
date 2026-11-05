//! NEP-413 off-chain message signing and the verifier's `MultiPayload` shape.

use crate::keys::NearKeypair;
use base64::{engine::general_purpose::STANDARD, Engine};
use borsh::BorshSerialize;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// `2^31 + 413`, the NEP-413 domain tag.
const NEP413_TAG: u32 = 2_147_484_061;

const ED25519_PREFIX: &str = "ed25519:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nep413Payload {
    pub message: String,
    /// Base64 of 32 bytes.
    pub nonce: String,
    pub recipient: String,
}

/// One signed intent, exactly as `execute_intents` takes it.
///
/// `standard` is a plain field, so a payload carrying any other value parses
/// here and is refused by [`verify`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiPayload {
    pub standard: String,
    pub payload: Nep413Payload,
    pub public_key: String,
    pub signature: String,
}

/// A payload whose contents cannot be decoded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PayloadError {
    #[error("nonce is not base64 of 32 bytes")]
    Nonce,
    #[error("message is not an intent message")]
    Message,
}

#[derive(BorshSerialize)]
struct BorshPayload<'a> {
    message: &'a str,
    nonce: [u8; 32],
    recipient: &'a str,
    callback_url: Option<&'a str>,
}

fn hash_of(message: &str, nonce: [u8; 32], recipient: &str) -> [u8; 32] {
    let body = BorshPayload { message, nonce, recipient, callback_url: None };
    let mut hasher = Sha256::new();
    hasher.update(NEP413_TAG.to_le_bytes());
    // Writing to a Vec cannot fail.
    hasher.update(borsh::to_vec(&body).unwrap_or_default());
    hasher.finalize().into()
}

impl MultiPayload {
    pub(crate) fn sign(signer: &NearKeypair, message: String, nonce: [u8; 32], recipient: &str) -> Self {
        let signature = signer.sign(&hash_of(&message, nonce, recipient));
        Self {
            standard: "nep413".to_owned(),
            payload: Nep413Payload {
                message,
                nonce: STANDARD.encode(nonce),
                recipient: recipient.to_owned(),
            },
            public_key: signer.public_key_string(),
            signature: format!("{ED25519_PREFIX}{}", bs58::encode(signature).into_string()),
        }
    }

    /// The NEP-413 hash the signature covers.
    ///
    /// # Errors
    ///
    /// [`PayloadError::Nonce`] if the nonce is not base64 of 32 bytes.
    pub fn hash(&self) -> Result<[u8; 32], PayloadError> {
        let nonce: [u8; 32] = STANDARD
            .decode(&self.payload.nonce)
            .map_err(|_| PayloadError::Nonce)?
            .try_into()
            .map_err(|_| PayloadError::Nonce)?;
        Ok(hash_of(&self.payload.message, nonce, &self.payload.recipient))
    }

    /// The `signer_id` the signed message names.
    ///
    /// # Errors
    ///
    /// [`PayloadError::Message`] if the message is not an intent message.
    pub fn signer_id(&self) -> Result<String, PayloadError> {
        let message: serde_json::Value =
            serde_json::from_str(&self.payload.message).map_err(|_| PayloadError::Message)?;
        message
            .get("signer_id")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or(PayloadError::Message)
    }
}

fn decode_prefixed<const N: usize>(value: &str) -> Option<[u8; N]> {
    let encoded = value.strip_prefix(ED25519_PREFIX)?;
    bs58::decode(encoded).into_vec().ok()?.try_into().ok()
}

/// Checks the signature only. The verifier still decides whether the intent
/// is acceptable; a gateway uses this so it does not relay forged payloads.
#[must_use]
pub fn verify(payload: &MultiPayload) -> bool {
    if payload.standard != "nep413" {
        return false;
    }
    let Ok(hash) = payload.hash() else { return false };
    let Some(public_key) = decode_prefixed::<32>(&payload.public_key) else { return false };
    let Some(signature) = decode_prefixed::<64>(&payload.signature) else { return false };
    let Ok(key) = VerifyingKey::from_bytes(&public_key) else { return false };
    key.verify_strict(&hash, &Signature::from_bytes(&signature)).is_ok()
}
