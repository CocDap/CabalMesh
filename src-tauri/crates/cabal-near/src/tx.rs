//! Hand-encoded NEAR transactions: `Transaction::V0` with `FunctionCall` actions.

use crate::keys::{AccountId, NearKeypair};
use base64::{engine::general_purpose::STANDARD, Engine};
use borsh::BorshSerialize;
use sha2::{Digest, Sha256};

/// Measured 30.58 Tgas in the spike. Unused gas is refunded.
pub const GAS_FT_TRANSFER_CALL: u64 = 100_000_000_000_000;

/// Measured 14.80 Tgas for a pair and 32.88 Tgas for a withdrawal in the spike.
pub const GAS_EXECUTE_INTENTS: u64 = 300_000_000_000_000;

/// Borsh enum index of `ed25519`, for both public keys and signatures.
const ED25519: u8 = 0;

/// Borsh enum index of `Action::FunctionCall`.
const ACTION_FUNCTION_CALL: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionCall {
    pub method_name: String,
    pub args: Vec<u8>,
    pub gas: u64,
    pub deposit: u128,
}

#[derive(BorshSerialize)]
struct Action<'a> {
    kind: u8,
    method_name: &'a str,
    args: &'a [u8],
    gas: u64,
    deposit: u128,
}

#[derive(BorshSerialize)]
struct Transaction<'a> {
    signer_id: &'a str,
    key_kind: u8,
    public_key: [u8; 32],
    nonce: u64,
    receiver_id: &'a str,
    block_hash: [u8; 32],
    actions: Vec<Action<'a>>,
}

/// Base64 of a borsh `SignedTransaction`, ready for RPC `send_tx`.
#[must_use]
pub fn sign_function_calls(
    signer: &NearKeypair,
    receiver: &AccountId,
    nonce: u64,
    block_hash: [u8; 32],
    calls: &[FunctionCall],
) -> String {
    let account = signer.account_id();
    let tx = Transaction {
        signer_id: account.as_str(),
        key_kind: ED25519,
        public_key: signer.public_key_bytes(),
        nonce,
        receiver_id: receiver.as_str(),
        block_hash,
        actions: calls
            .iter()
            .map(|call| Action {
                kind: ACTION_FUNCTION_CALL,
                method_name: &call.method_name,
                args: &call.args,
                gas: call.gas,
                deposit: call.deposit,
            })
            .collect(),
    };
    // Writing to a Vec cannot fail.
    let mut encoded = borsh::to_vec(&tx).unwrap_or_default();
    let signature = signer.sign(&Sha256::digest(&encoded));
    encoded.push(ED25519);
    encoded.extend_from_slice(&signature);
    STANDARD.encode(encoded)
}
