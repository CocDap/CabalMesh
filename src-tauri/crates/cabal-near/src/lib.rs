//! NEAR keys, NEP-413 signing and NEAR Intents payloads.
//!
//! This crate performs no I/O. `tests/fixtures/` holds golden vectors captured
//! from the Tidepool testnet verifier; see `docs/near-intents-spike.md`.
//!
//! No `near-primitives`: it pulls the full nearcore type tree into an app whose
//! mobile size is tracked, for the one transaction shape needed here.

#![forbid(unsafe_code)]

mod events;
mod intents;
mod keys;
mod nep413;
mod quote;
mod tx;

pub use events::settlements_for;
pub use intents::{
    ft_withdraw_payload, token_diff_payload, versioned_nonce, GatewayFee, TokenDiff,
};
pub use keys::{AccountId, InvalidAccountId, NearKeypair};
pub use nep413::{verify, MultiPayload, Nep413Payload, PayloadError};
pub use quote::{buyer_diff, quote_amount, seller_diff, FeeRule};
pub use tx::{sign_function_calls, FunctionCall, GAS_EXECUTE_INTENTS, GAS_FT_TRANSFER_CALL};
