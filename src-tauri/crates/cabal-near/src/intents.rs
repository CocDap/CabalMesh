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
    intents: Vec<I>,
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
    Transfer {
        receiver_id: &'a str,
        tokens: BTreeMap<&'a str, String>,
    },
}

/// What a trader pays the rewards contract for whichever gateway submits.
///
/// Paid to the contract rather than a gateway, so it can be signed offline
/// without knowing who will submit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayFee {
    pub to: AccountId,
    pub token: String,
    pub amount: u128,
}

impl GatewayFee {
    fn intent(&self) -> Intent<'_> {
        Intent::Transfer {
            receiver_id: self.to.as_str(),
            tokens: BTreeMap::from([(self.token.as_str(), self.amount.to_string())]),
        }
    }

    /// Whether this payload signed exactly this fee.
    #[must_use]
    pub fn paid_by(&self, payload: &MultiPayload) -> bool {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(&payload.payload.message)
        else {
            return false;
        };
        let Some(expected) = serde_json::to_value(self.intent()).ok() else {
            return false;
        };
        message["intents"]
            .as_array()
            .is_some_and(|intents| intents.contains(&expected))
    }
}

fn signed(
    signer: &NearKeypair,
    verifier: &AccountId,
    intents: Vec<Intent<'_>>,
    deadline_unix_ms: u64,
    nonce: [u8; 32],
) -> MultiPayload {
    let account = signer.account_id();
    let message = IntentMessage {
        signer_id: account.as_str(),
        deadline: render_deadline(deadline_unix_ms),
        intents,
    };
    // Plain structs of strings: serializing cannot fail.
    let message = serde_json::to_string(&message).unwrap_or_default();
    MultiPayload::sign(signer, message, nonce, verifier.as_str())
}

/// A signed `token_diff` intent, followed by the fee when there is one.
///
/// The diff comes first: the seller may hold none of the fee token until the
/// trade pays it.
#[must_use]
pub fn token_diff_payload(
    signer: &NearKeypair,
    verifier: &AccountId,
    diff: &TokenDiff,
    fee: Option<&GatewayFee>,
    deadline_unix_ms: u64,
    nonce: [u8; 32],
) -> MultiPayload {
    let mut intents = vec![Intent::TokenDiff { diff }];
    intents.extend(fee.map(GatewayFee::intent));
    signed(signer, verifier, intents, deadline_unix_ms, nonce)
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
    signed(signer, verifier, vec![intent], deadline_unix_ms, nonce)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fee() -> GatewayFee {
        GatewayFee {
            to: AccountId::try_from("rewards.test").unwrap(),
            token: "nep141:usdc.test".into(),
            amount: 10_000,
        }
    }

    fn signed_with(fee: Option<&GatewayFee>) -> MultiPayload {
        let verifier = AccountId::try_from("verifier.test").unwrap();
        let diff: TokenDiff = [("nep141:usdc.test".to_owned(), -1)].into_iter().collect();
        token_diff_payload(
            &NearKeypair::from_secret_bytes(&[3; 32]),
            &verifier,
            &diff,
            fee,
            1_000,
            [0; 32],
        )
    }

    #[test]
    fn a_fee_is_a_transfer_signed_after_the_diff() {
        let payload = signed_with(Some(&fee()));
        let message: serde_json::Value = serde_json::from_str(&payload.payload.message).unwrap();
        assert_eq!(message["intents"][0]["intent"], "token_diff");
        assert_eq!(
            message["intents"][1],
            serde_json::json!({
                "intent": "transfer",
                "receiver_id": "rewards.test",
                "tokens": { "nep141:usdc.test": "10000" },
            })
        );
    }

    #[test]
    fn with_no_fee_the_message_holds_only_the_diff() {
        let payload = signed_with(None);
        let message: serde_json::Value = serde_json::from_str(&payload.payload.message).unwrap();
        assert_eq!(message["intents"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_payload_pays_a_fee_only_if_it_signed_that_fee() {
        assert!(fee().paid_by(&signed_with(Some(&fee()))));
        assert!(!fee().paid_by(&signed_with(None)));
        let other = GatewayFee { amount: 1, ..fee() };
        assert!(!fee().paid_by(&signed_with(Some(&other))));
    }
}
