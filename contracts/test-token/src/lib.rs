//! Test only. The verifier credits whichever contract calls `ft_on_transfer`,
//! so this is all a sandbox test needs in place of a real token.

use near_sdk::json_types::U128;
use near_sdk::{env, near, serde_json, AccountId, Gas, NearToken, Promise};

#[near(contract_state)]
#[derive(Default)]
pub struct TestToken {}

#[near]
impl TestToken {
    pub fn credit(&mut self, verifier: AccountId, receiver: AccountId, amount: U128) -> Promise {
        let args = serde_json::json!({
            "sender_id": env::current_account_id(),
            "amount": amount,
            "msg": receiver,
        });
        Promise::new(verifier).function_call(
            "ft_on_transfer".to_owned(),
            args.to_string().into_bytes(),
            NearToken::from_yoctonear(0),
            Gas::from_tgas(100),
        )
    }

    /// The verifier may ask a token for its balance when one is withdrawn.
    pub fn ft_balance_of(&self, account_id: AccountId) -> U128 {
        let _ = account_id;
        U128(0)
    }
}
