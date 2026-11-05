//! Pays the gateway that submits a matched NEAR Intents pair.
//!
//! Each trader signs a `transfer` of the fee to this contract next to their
//! `token_diff`. The fee goes to the contract rather than a gateway, so a trader
//! can sign offline without knowing who will submit. If the pair settles, the
//! contract pays both fees to whoever called [`Rewards::submit`].
//!
//! Funds never leave the verifier, and nobody can withdraw them except as a
//! payout to a submitter.
//!
//! # Why the fee check is strict
//!
//! A payout spends money the contract already holds, so it must be backed by a
//! fee this very pair really paid. The verifier ignores JSON fields it does not
//! know, and some it does know make a `transfer` refundable after it runs. A
//! check that only looked for a matching transfer could be fooled into paying
//! out of fees other traders paid, so anything but the exact shape the app signs
//! is refused.

use near_sdk::json_types::U128;
use near_sdk::serde::de::{self, Deserializer, MapAccess, Visitor};
use near_sdk::serde::Deserialize;
use near_sdk::serde_json::{self, Value};
use near_sdk::store::LookupMap;
use near_sdk::{
    env, near, require, AccountId, BorshStorageKey, Gas, NearToken, PanicOnDefault, Promise,
    PromiseError,
};
use std::collections::BTreeMap;
use std::fmt;

/// `execute_intents` burned about 16 Tgas with fees on testnet; this leaves room.
const GAS_EXECUTE_INTENTS: Gas = Gas::from_tgas(150);
const GAS_ON_EXECUTED: Gas = Gas::from_tgas(50);
const GAS_MT_TRANSFER: Gas = Gas::from_tgas(25);
const GAS_ON_PAID: Gas = Gas::from_tgas(10);
/// What `submit` needs to finish on its own account, on top of the calls it makes.
const GAS_SUBMIT_SELF: Gas = Gas::from_tgas(20);

/// A matched pair: one payload for each side.
const PAYLOADS_PER_PAIR: usize = 2;

#[derive(BorshStorageKey)]
#[near]
enum StorageKey {
    Pending,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Rewards {
    verifier: AccountId,
    fee_token: String,
    fee_per_side: u128,
    pending: LookupMap<AccountId, u128>,
    submissions: u64,
    paid: u128,
}

#[near(serializers = [json])]
#[derive(Debug, PartialEq, Eq)]
pub struct RewardStats {
    pub submissions: u64,
    pub paid: U128,
}

#[near(serializers = [json])]
#[derive(Debug, PartialEq, Eq)]
pub struct RewardConfig {
    pub verifier: AccountId,
    pub fee_token: String,
    pub fee_per_side: U128,
}

/// One signed side. Fields the verifier would act on must be spelled exactly so.
#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde", deny_unknown_fields)]
struct SignedSide {
    standard: String,
    payload: Nep413Payload,
    #[allow(dead_code)]
    public_key: String,
    #[allow(dead_code)]
    signature: String,
}

#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde", deny_unknown_fields)]
struct Nep413Payload {
    message: String,
    #[allow(dead_code)]
    nonce: String,
    recipient: String,
}

#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde", deny_unknown_fields)]
struct Message {
    signer_id: AccountId,
    #[allow(dead_code)]
    deadline: String,
    /// A tuple, so it is the two intents in this order or it does not parse.
    /// Not `Value`: that keeps the last of a duplicated key, where the verifier
    /// might read another.
    intents: (TokenDiffIntent, FeeIntent),
}

#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde", deny_unknown_fields)]
struct TokenDiffIntent {
    intent: String,
    #[allow(dead_code)]
    diff: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde", deny_unknown_fields)]
struct FeeIntent {
    intent: String,
    receiver_id: AccountId,
    tokens: OneToken,
}

/// A map of exactly one token. A map type would keep one of a duplicated key
/// silently, and the fee check must read the same amount the verifier does.
struct OneToken {
    id: String,
    amount: String,
}

impl<'de> Deserialize<'de> for OneToken {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OneTokenVisitor;

        impl<'de> Visitor<'de> for OneTokenVisitor {
            type Value = OneToken;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map of exactly one token")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<OneToken, A::Error> {
                let (id, amount) = map
                    .next_entry()?
                    .ok_or_else(|| de::Error::custom("no token"))?;
                if map.next_key::<String>()?.is_some() {
                    return Err(de::Error::custom("more than one token"));
                }
                Ok(OneToken { id, amount })
            }
        }

        deserializer.deserialize_map(OneTokenVisitor)
    }
}

#[near]
impl Rewards {
    #[init]
    #[private]
    pub fn new(verifier: AccountId, fee_token: String, fee_per_side: U128) -> Self {
        require!(fee_per_side.0 > 0, "fee_per_side must be above zero");
        Self {
            verifier,
            fee_token,
            fee_per_side: fee_per_side.0,
            pending: LookupMap::new(StorageKey::Pending),
            submissions: 0,
            paid: 0,
        }
    }

    /// Forwards a matched pair to the verifier and pays the caller if it settles.
    ///
    /// Refused here, before any forwarding, when a payload is not exactly what
    /// the app signs, so a gateway never spends gas on a pair that pays nothing.
    pub fn submit(&mut self, payloads: Vec<Value>) -> Promise {
        let needed = GAS_EXECUTE_INTENTS
            .saturating_add(GAS_ON_EXECUTED)
            .saturating_add(GAS_SUBMIT_SELF);
        // Otherwise the trade could stand with too little gas left to record it.
        require!(env::prepaid_gas() >= needed, "not enough gas attached");
        require!(
            payloads.len() == PAYLOADS_PER_PAIR,
            "a pair is two payloads"
        );
        for payload in &payloads {
            if let Err(reason) = self.check_payload(payload) {
                env::panic_str(reason);
            }
        }

        let args = serde_json::json!({ "signed": payloads });
        let amount = self.fee_per_side * PAYLOADS_PER_PAIR as u128;
        Promise::new(self.verifier.clone())
            .function_call(
                "execute_intents".to_owned(),
                args.to_string().into_bytes(),
                NearToken::from_yoctonear(0),
                GAS_EXECUTE_INTENTS,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_ON_EXECUTED)
                    .on_executed(env::predecessor_account_id(), U128(amount)),
            )
    }

    /// Pays the gateway if the verifier accepted the pair. A rejected pair took
    /// its fee transfers down with it, so there is nothing to pay.
    ///
    /// Panics on a rejected pair so the transaction reads as failed: a gateway
    /// must not report a settlement that did not happen. A failed payout does
    /// not fail the transaction, because the trade stands either way.
    #[private]
    pub fn on_executed(
        &mut self,
        gateway: AccountId,
        amount: U128,
        #[callback_result] executed: Result<(), PromiseError>,
    ) {
        if executed.is_err() {
            env::panic_str("the verifier rejected the pair");
        }
        self.submissions += 1;
        self.pay(gateway, amount.0).detach();
    }

    /// Credits a payout that did not go through, for [`Rewards::claim`].
    #[private]
    pub fn on_paid(
        &mut self,
        gateway: AccountId,
        amount: U128,
        #[callback_result] transferred: Result<(), PromiseError>,
    ) -> bool {
        if transferred.is_ok() {
            self.paid += amount.0;
            return true;
        }
        let owed = self.pending.get(&gateway).copied().unwrap_or(0);
        self.pending.insert(gateway, owed + amount.0);
        false
    }

    /// Retries the caller's failed payouts.
    pub fn claim(&mut self) -> Promise {
        let gateway = env::predecessor_account_id();
        let amount = self.pending.remove(&gateway).unwrap_or(0);
        require!(amount > 0, "nothing to claim");
        // Removed before the transfer, so a second claim cannot pay it twice;
        // `on_paid` restores it if the transfer fails.
        self.pay(gateway, amount)
    }

    pub fn pending_of(&self, account_id: AccountId) -> U128 {
        U128(self.pending.get(&account_id).copied().unwrap_or(0))
    }

    pub fn stats(&self) -> RewardStats {
        RewardStats {
            submissions: self.submissions,
            paid: U128(self.paid),
        }
    }

    pub fn config(&self) -> RewardConfig {
        RewardConfig {
            verifier: self.verifier.clone(),
            fee_token: self.fee_token.clone(),
            fee_per_side: U128(self.fee_per_side),
        }
    }
}

impl Rewards {
    fn pay(&self, gateway: AccountId, amount: u128) -> Promise {
        let args = serde_json::json!({
            "receiver_id": gateway,
            "token_id": self.fee_token,
            "amount": U128(amount),
        });
        Promise::new(self.verifier.clone())
            .function_call(
                "mt_transfer".to_owned(),
                args.to_string().into_bytes(),
                NearToken::from_yoctonear(1),
                GAS_MT_TRANSFER,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_ON_PAID)
                    .on_paid(gateway, U128(amount)),
            )
    }

    /// Returns why this payload does not pay the fee, if it does not.
    fn check_payload(&self, payload: &Value) -> Result<(), &'static str> {
        let side =
            SignedSide::deserialize(payload).map_err(|_| "payload has an unexpected shape")?;
        if side.standard != "nep413" {
            return Err("payload is not nep413");
        }
        if side.payload.recipient != self.verifier.as_str() {
            return Err("payload is not signed for this verifier");
        }
        let message: Message = serde_json::from_str(&side.payload.message)
            .map_err(|_| "message has an unexpected shape")?;
        if message.signer_id == env::current_account_id() {
            return Err("this contract cannot pay itself");
        }
        let (diff, fee) = message.intents;
        if diff.intent != "token_diff" {
            return Err("first intent must be a token_diff");
        }
        if fee.intent != "transfer" || fee.receiver_id != env::current_account_id() {
            return Err("second intent must transfer the fee to this contract");
        }
        if fee.tokens.id != self.fee_token || fee.tokens.amount != self.fee_per_side.to_string() {
            return Err("fee is not the required amount of the fee token");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::{testing_env, PromiseResult};

    const FEE: u128 = 10_000;
    const USDC: &str = "nep141:usdc.test";

    fn account(name: &str) -> AccountId {
        name.parse().unwrap()
    }

    fn context(predecessor: &str, results: Vec<PromiseResult>) {
        let mut builder = VMContextBuilder::new();
        builder
            .current_account_id(account("rewards.test"))
            .predecessor_account_id(account(predecessor))
            .prepaid_gas(Gas::from_tgas(300));
        testing_env!(
            builder.build(),
            near_sdk::test_vm_config(),
            near_sdk::RuntimeFeesConfig::test(),
            Default::default(),
            results
        );
    }

    fn contract() -> Rewards {
        context("rewards.test", vec![]);
        Rewards::new(account("verifier.test"), USDC.into(), U128(FEE))
    }

    fn diff() -> Value {
        serde_json::json!({ "intent": "token_diff", "diff": { USDC: "-1", "nep141:wnear.test": "1" } })
    }

    fn fee() -> Value {
        serde_json::json!({
            "intent": "transfer",
            "receiver_id": "rewards.test",
            "tokens": { USDC: FEE.to_string() },
        })
    }

    fn side(intents: Vec<Value>) -> Value {
        let message = serde_json::json!({
            "signer_id": "trader.test",
            "deadline": "2030-01-01T00:00:00.000Z",
            "intents": intents,
        });
        side_with_message(&message.to_string())
    }

    fn side_with_message(message: &str) -> Value {
        serde_json::json!({
            "standard": "nep413",
            "payload": { "message": message, "nonce": "AAAA", "recipient": "verifier.test" },
            "public_key": "ed25519:key",
            "signature": "ed25519:sig",
        })
    }

    fn with(mut value: Value, field: &str, extra: Value) -> Value {
        value[field] = extra;
        value
    }

    #[test]
    fn the_payload_the_app_signs_passes() {
        assert_eq!(contract().check_payload(&side(vec![diff(), fee()])), Ok(()));
    }

    #[test]
    fn a_payload_without_the_fee_is_refused() {
        assert!(contract().check_payload(&side(vec![diff()])).is_err());
    }

    /// Each of these could leave the contract paying out a fee it did not keep.
    #[test]
    fn a_transfer_that_could_be_undone_is_refused() {
        for field in ["msg", "notification", "min_gas", "memo", "anything_new"] {
            let payload = side(vec![diff(), with(fee(), field, serde_json::json!("x"))]);
            assert!(
                contract().check_payload(&payload).is_err(),
                "{field} passed"
            );
        }
    }

    #[test]
    fn a_fee_of_the_wrong_size_or_token_is_refused() {
        let contract = contract();
        for tokens in [
            serde_json::json!({ USDC: "9999" }),
            serde_json::json!({ USDC: "010000" }),
            serde_json::json!({ "nep141:other.test": "10000" }),
            serde_json::json!({ USDC: "10000", "nep141:other.test": "1" }),
        ] {
            let payload = side(vec![diff(), with(fee(), "tokens", tokens.clone())]);
            assert!(contract.check_payload(&payload).is_err(), "{tokens} passed");
        }
    }

    #[test]
    fn a_fee_to_someone_else_is_refused() {
        let payload = side(vec![
            diff(),
            with(fee(), "receiver_id", serde_json::json!("gateway.test")),
        ]);
        assert!(contract().check_payload(&payload).is_err());
    }

    #[test]
    fn any_other_intent_beside_the_fee_is_refused() {
        let contract = contract();
        let withdraw = serde_json::json!({
            "intent": "ft_withdraw", "token": "usdc.test", "receiver_id": "x.test", "amount": "1",
        });
        assert!(contract
            .check_payload(&side(vec![withdraw.clone(), fee()]))
            .is_err());
        assert!(contract
            .check_payload(&side(vec![diff(), fee(), withdraw]))
            .is_err());
        assert!(contract.check_payload(&side(vec![fee(), diff()])).is_err());
        assert!(contract.check_payload(&side(vec![fee(), fee()])).is_err());
    }

    /// A duplicated key could mean one thing here and another to the verifier.
    #[test]
    fn a_message_with_a_duplicated_key_is_refused() {
        let message = format!(
            r#"{{"signer_id":"trader.test","deadline":"d","intents":[],"intents":[{},{}]}}"#,
            diff(),
            fee()
        );
        assert!(contract()
            .check_payload(&side_with_message(&message))
            .is_err());
    }

    #[test]
    fn a_duplicated_key_inside_the_fee_intent_is_refused() {
        let fee = format!(
            r#"{{"intent":"token_diff","intent":"transfer","receiver_id":"rewards.test","tokens":{{"{USDC}":"10000"}}}}"#
        );
        let message = format!(
            r#"{{"signer_id":"trader.test","deadline":"d","intents":[{},{fee}]}}"#,
            diff()
        );
        assert!(contract()
            .check_payload(&side_with_message(&message))
            .is_err());
    }

    #[test]
    fn a_payload_for_another_verifier_is_refused() {
        let mut payload = side(vec![diff(), fee()]);
        payload["payload"]["recipient"] = serde_json::json!("other.test");
        assert!(contract().check_payload(&payload).is_err());
    }

    #[test]
    fn the_contract_cannot_be_the_signer() {
        let message = serde_json::json!({
            "signer_id": "rewards.test",
            "deadline": "d",
            "intents": [diff(), fee()],
        });
        assert!(contract()
            .check_payload(&side_with_message(&message.to_string()))
            .is_err());
    }

    #[test]
    #[should_panic(expected = "the verifier rejected the pair")]
    fn a_rejected_pair_fails_the_transaction_and_pays_nothing() {
        let mut contract = contract();
        context("rewards.test", vec![PromiseResult::Failed]);
        contract.on_executed(
            account("gateway.test"),
            U128(FEE * 2),
            Err(PromiseError::Failed),
        );
    }

    #[test]
    fn a_duplicated_token_in_the_fee_is_refused() {
        let fee = format!(
            r#"{{"intent":"transfer","receiver_id":"rewards.test","tokens":{{"{USDC}":"1","{USDC}":"10000"}}}}"#
        );
        let message = format!(
            r#"{{"signer_id":"trader.test","deadline":"d","intents":[{},{fee}]}}"#,
            diff()
        );
        assert!(contract()
            .check_payload(&side_with_message(&message))
            .is_err());
    }

    #[test]
    fn a_failed_payout_can_be_claimed_once() {
        let mut contract = contract();
        let gateway = account("gateway.test");
        context("rewards.test", vec![PromiseResult::Failed]);
        assert!(!contract.on_paid(gateway.clone(), U128(FEE * 2), Err(PromiseError::Failed)));
        assert_eq!(contract.pending_of(gateway.clone()), U128(FEE * 2));
        assert_eq!(contract.stats().paid, U128(0));

        context("gateway.test", vec![]);
        let _ = contract.claim();
        assert_eq!(contract.pending_of(gateway), U128(0));
    }

    #[test]
    #[should_panic(expected = "nothing to claim")]
    fn claiming_with_nothing_owed_is_refused() {
        let mut contract = contract();
        context("gateway.test", vec![]);
        let _ = contract.claim();
    }

    #[test]
    fn a_failed_retry_stays_claimable() {
        let mut contract = contract();
        let gateway = account("gateway.test");
        context("rewards.test", vec![PromiseResult::Failed]);
        contract.on_paid(gateway.clone(), U128(FEE), Err(PromiseError::Failed));
        context("gateway.test", vec![]);
        let _ = contract.claim();
        context("rewards.test", vec![PromiseResult::Failed]);
        contract.on_paid(gateway.clone(), U128(FEE), Err(PromiseError::Failed));
        assert_eq!(contract.pending_of(gateway), U128(FEE));
    }

    #[test]
    fn a_settled_payout_counts_as_paid() {
        let mut contract = contract();
        context("rewards.test", vec![PromiseResult::Successful(vec![])]);
        assert!(contract.on_paid(account("gateway.test"), U128(FEE * 2), Ok(())));
        assert_eq!(contract.stats().paid, U128(FEE * 2));
    }
}
