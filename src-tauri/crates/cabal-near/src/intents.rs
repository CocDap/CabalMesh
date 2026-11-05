//! Intent messages for the NEAR Intents verifier.

use crate::keys::{AccountId, NearKeypair};
use crate::nep413::MultiPayload;
use chrono::DateTime;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use std::collections::BTreeMap;

/// Versioned nonce prefix, from `defuse` v0.4.5.
const NONCE_MAGIC: [u8; 4] = [0x56, 0x28, 0xf6, 0xc6];

/// Signed balance changes keyed by `nep141:<contract>`. Sorted, so both sides
/// of a trade produce the same bytes. Amounts serialize as decimal strings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenDiff(BTreeMap<String, i128>);

impl TokenDiff {
    #[must_use]
    pub fn get(&self, token: &str) -> Option<&i128> {
        self.0.get(token)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &i128)> {
        self.0.iter()
    }
}

impl FromIterator<(String, i128)> for TokenDiff {
    fn from_iter<T: IntoIterator<Item = (String, i128)>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl Serialize for TokenDiff {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (token, amount) in &self.0 {
            map.serialize_entry(token, &amount.to_string())?;
        }
        map.end()
    }
}

/// `magic ‖ 0x00 ‖ salt ‖ deadline_ns: i64 LE ‖ random`.
///
/// A deadline past what `i64` nanoseconds can hold saturates; that is the year
/// 2262, and the verifier rejects a nonce whose deadline precedes the message's.
#[must_use]
pub fn versioned_nonce(salt: [u8; 4], deadline_unix_ms: u64, random: [u8; 15]) -> [u8; 32] {
    let deadline_ns = i64::try_from(u128::from(deadline_unix_ms) * 1_000_000).unwrap_or(i64::MAX);
    let mut nonce = [0u8; 32];
    nonce[..4].copy_from_slice(&NONCE_MAGIC);
    nonce[5..9].copy_from_slice(&salt);
    nonce[9..17].copy_from_slice(&deadline_ns.to_le_bytes());
    nonce[17..].copy_from_slice(&random);
    nonce
}

/// `YYYY-MM-DDTHH:MM:SS.mmmZ`.
fn render_deadline(deadline_unix_ms: u64) -> String {
    i64::try_from(deadline_unix_ms)
        .ok()
        .and_then(DateTime::from_timestamp_millis)
        .unwrap_or(DateTime::UNIX_EPOCH)
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

/// Key order is part of the signed bytes: `signer_id`, `deadline`, `intents`.
#[derive(Serialize)]
struct IntentMessage<'a, I: Serialize> {
    signer_id: &'a str,
    deadline: String,
    intents: [I; 1],
}

#[derive(Serialize)]
#[serde(tag = "intent", rename_all = "snake_case")]
enum Intent<'a> {
    TokenDiff {
        diff: &'a TokenDiff,
    },
    FtWithdraw {
        token: &'a str,
        receiver_id: &'a str,
        amount: String,
    },
}

fn signed(
    signer: &NearKeypair,
    verifier: &AccountId,
    intent: Intent<'_>,
    deadline_unix_ms: u64,
    nonce: [u8; 32],
) -> MultiPayload {
    let account = signer.account_id();
    let message = IntentMessage {
        signer_id: account.as_str(),
        deadline: render_deadline(deadline_unix_ms),
        intents: [intent],
    };
    // Plain structs of strings: serializing cannot fail.
    let message = serde_json::to_string(&message).unwrap_or_default();
    MultiPayload::sign(signer, message, nonce, verifier.as_str())
}

/// A signed `token_diff` intent.
#[must_use]
pub fn token_diff_payload(
    signer: &NearKeypair,
    verifier: &AccountId,
    diff: &TokenDiff,
    deadline_unix_ms: u64,
    nonce: [u8; 32],
) -> MultiPayload {
    signed(signer, verifier, Intent::TokenDiff { diff }, deadline_unix_ms, nonce)
}

/// A signed `ft_withdraw` intent back to the signer's own account.
#[must_use]
pub fn ft_withdraw_payload(
    signer: &NearKeypair,
    verifier: &AccountId,
    token: &str,
    amount: u128,
    deadline_unix_ms: u64,
    nonce: [u8; 32],
) -> MultiPayload {
    let account = signer.account_id();
    let intent = Intent::FtWithdraw {
        token,
        receiver_id: account.as_str(),
        amount: amount.to_string(),
    };
    signed(signer, verifier, intent, deadline_unix_ms, nonce)
}
