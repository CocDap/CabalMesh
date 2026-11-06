//! NEAR-backed deposits and withdrawals. Never wrap Tidepool's faucet token.

use super::*;
use cabal_near::native_withdraw_payload;

/// Leave 0.01 NEAR available for subsequent transactions, beyond storage stake.
pub const GAS_RESERVE: u128 = 10_000_000_000_000_000_000_000;
const GAS_WRAP: u64 = 30_000_000_000_000;

struct Registration {
    calls: Vec<FunctionCall>,
    cost: u128,
}

impl NearBridge {
    async fn wrapper_registration(&self, wrapper: &str) -> Result<Registration, NearError> {
        let mut missing = Vec::new();
        for account in [self.account_id(), self.verifier.clone()] {
            let balance = self
                .view(
                    wrapper,
                    "storage_balance_of",
                    &json!({ "account_id": account }),
                )
                .await?;
            if balance.is_null() {
                missing.push(account);
            } else {
                // A malformed RPC response must not mean registration succeeded.
                parse_amount(balance.get("total").unwrap_or(&Value::Null))?;
            }
        }
        if missing.is_empty() {
            return Ok(Registration {
                calls: Vec::new(),
                cost: 0,
            });
        }
        let bounds = self
            .view(wrapper, "storage_balance_bounds", &json!({}))
            .await?;
        let minimum = parse_amount(bounds.get("min").unwrap_or(&Value::Null))?;
        let cost = minimum
            .checked_mul(missing.len() as u128)
            .ok_or_else(|| NearError::Rpc("invalid storage cost".into()))?;
        Ok(Registration {
            cost,
            calls: missing
                .into_iter()
                .map(|account| FunctionCall {
                    method_name: "storage_deposit".into(),
                    args: json!({ "account_id": account, "registration_only": true })
                        .to_string()
                        .into_bytes(),
                    gas: GAS_WRAP,
                    deposit: minimum,
                })
                .collect(),
        })
    }

    async fn native_after_reserve(&self) -> Result<u128, NearError> {
        let account = match self.rpc("query", json!({
            "request_type": "view_account", "finality": "final", "account_id": self.account_id(),
        })).await {
            Ok(account) => account,
            Err(NearError::Rpc(text)) if names_missing_account(&text) => return Ok(0),
            Err(error) => return Err(error),
        };
        let amount = parse_amount(account.get("amount").unwrap_or(&Value::Null))?;
        let usage = account
            .get("storage_usage")
            .and_then(Value::as_u64)
            .ok_or_else(|| NearError::Rpc("account had no storage usage".into()))?;
        let config = self
            .rpc(
                "EXPERIMENTAL_protocol_config",
                json!({ "finality": "final" }),
            )
            .await?;
        let byte_cost = parse_amount(
            config
                .pointer("/runtime_config/storage_amount_per_byte")
                .unwrap_or(&Value::Null),
        )?;
        let storage = byte_cost
            .checked_mul(u128::from(usage))
            .ok_or_else(|| NearError::Rpc("invalid account storage cost".into()))?;
        Ok(amount.saturating_sub(storage).saturating_sub(GAS_RESERVE))
    }

    /// Spendable native NEAR, after account stake, wrapper registration and gas.
    pub async fn native_deposit_available(&self, wrapper: &str) -> Result<u128, NearError> {
        let remaining = self.native_after_reserve().await?;
        if remaining == 0 {
            return Ok(0);
        }
        Ok(remaining.saturating_sub(self.wrapper_registration(wrapper).await?.cost))
    }

    /// Register if needed, wrap exactly `amount`, then deposit in one transaction.
    pub async fn deposit_native(&self, wrapper: &str, amount: u128) -> Result<TxHash, NearError> {
        if amount == 0 {
            return Err(NearError::InsufficientNative);
        }
        let receiver = AccountId::try_from(wrapper)
            .map_err(|_| NearError::Rpc("invalid wrapper id".into()))?;
        let mut registration = self.wrapper_registration(wrapper).await?;
        if amount
            > self
                .native_after_reserve()
                .await?
                .saturating_sub(registration.cost)
        {
            return Err(NearError::InsufficientNative);
        }
        registration.calls.push(FunctionCall {
            method_name: "near_deposit".into(),
            args: b"{}".to_vec(),
            gas: GAS_WRAP,
            deposit: amount,
        });
        registration.calls.push(FunctionCall {
            method_name: "ft_transfer_call".into(),
            args: json!({ "receiver_id": self.verifier, "amount": amount.to_string(), "msg": "" })
                .to_string()
                .into_bytes(),
            gas: GAS_FT_TRANSFER_CALL,
            deposit: 1,
        });
        let (hash, outcome) = self
            .send_calls(receiver, &registration.calls)
            .await
            .map_err(native_send_error)?;
        // ft_transfer_call can succeed while refunding some/all wNEAR. Its final
        // result is the amount accepted, not a boolean transaction success.
        let used = outcome
            .pointer("/status/SuccessValue")
            .and_then(Value::as_str)
            .and_then(|value| STANDARD.decode(value).ok())
            .and_then(|bytes| serde_json::from_slice::<String>(&bytes).ok())
            .and_then(|value| value.parse::<u128>().ok());
        if used != Some(amount) || has_failed_receipt(&outcome) {
            return Err(NearError::NativeTransferUnconfirmed {
                transaction: Some(hash.0),
            });
        }
        Ok(hash)
    }

    /// Uses the verifier's built-in unwrap, paying the signer in native NEAR.
    pub async fn withdraw_native(
        &self,
        wrapper: &str,
        amount: u128,
        now_ms: u64,
    ) -> Result<TxHash, NearError> {
        if amount == 0 || self.verifier_balance(wrapper).await? < amount {
            return Err(NearError::InsufficientNative);
        }
        let deadline = now_ms + INTENT_LIFETIME_MS;
        let nonce = versioned_nonce(self.current_salt().await?, deadline, rand::random());
        let payload =
            native_withdraw_payload(&self.keypair, &self.verifier, amount, deadline, nonce);
        self.simulate(std::slice::from_ref(&payload)).await?;
        let (hash, outcome) = self
            .send_calls(
                self.verifier.clone(),
                &[FunctionCall {
                    method_name: "execute_intents".into(),
                    args: json!({ "signed": [payload] }).to_string().into_bytes(),
                    gas: GAS_EXECUTE_INTENTS,
                    deposit: 0,
                }],
            )
            .await
            .map_err(native_send_error)?;
        confirmed_native(hash, &outcome)
    }

    /// Recover NEAR-backed wNEAR held in the wallet, including a deposit refund.
    pub async fn unwrap_native(&self, wrapper: &str, amount: u128) -> Result<TxHash, NearError> {
        if amount == 0 || self.wallet_balance(wrapper).await? < amount {
            return Err(NearError::InsufficientNative);
        }
        let receiver = AccountId::try_from(wrapper)
            .map_err(|_| NearError::Rpc("invalid wrapper id".into()))?;
        let (hash, outcome) = self
            .send_calls(
                receiver,
                &[FunctionCall {
                    method_name: "near_withdraw".into(),
                    args: json!({ "amount": amount.to_string() })
                        .to_string()
                        .into_bytes(),
                    gas: GAS_WRAP,
                    deposit: 1,
                }],
            )
            .await
            .map_err(native_send_error)?;
        confirmed_native(hash, &outcome)
    }
}

fn native_send_error(error: NearError) -> NearError {
    tracing::warn!(%error, "native transfer could not be confirmed");
    match error {
        NearError::Rpc(_) | NearError::Network(_) | NearError::Rejected(_) => {
            NearError::NativeTransferUnconfirmed { transaction: None }
        }
        other => other,
    }
}

fn has_failed_receipt(outcome: &Value) -> bool {
    outcome
        .get("receipts_outcome")
        .and_then(Value::as_array)
        .is_some_and(|receipts| {
            receipts
                .iter()
                .any(|receipt| receipt.pointer("/outcome/status/Failure").is_some())
        })
}

fn confirmed_native(hash: TxHash, outcome: &Value) -> Result<TxHash, NearError> {
    if has_failed_receipt(outcome) || outcome.pointer("/status/SuccessValue").is_none() {
        return Err(NearError::NativeTransferUnconfirmed {
            transaction: Some(hash.0),
        });
    }
    Ok(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::near_bridge::testing::{healthy, result, view_result, MockRpc};

    const ONE: u128 = 1_000_000_000_000_000_000_000_000;
    const REGISTRATION: u128 = 1_250_000_000_000_000_000_000;

    async fn node(registered: bool, accepted: u128, receipt_fails: bool) -> MockRpc {
        MockRpc::serve(move |method, params| {
            match method {
                "EXPERIMENTAL_protocol_config" => return result(json!({ "runtime_config": { "storage_amount_per_byte": "10000000000000000000" } })),
                "query" if params["request_type"] == "view_account" => return result(json!({ "amount": (2 * ONE).to_string(), "storage_usage": 100 })),
                "query" if params["request_type"] == "call_function" => {
                    let args: Value = serde_json::from_slice(&STANDARD.decode(params["args_base64"].as_str().unwrap()).unwrap()).unwrap();
                    let value = match params["method_name"].as_str().unwrap() {
                        "storage_balance_of" => if registered { json!({ "total": REGISTRATION.to_string() }) } else { Value::Null },
                        "storage_balance_bounds" => json!({ "min": REGISTRATION.to_string(), "max": REGISTRATION.to_string() }),
                        "ft_balance_of" => json!(ONE.to_string()),
                        // The faucet token has a larger balance. NEAR must not use it.
                        "mt_balance_of" => json!(if args["token_id"] == "nep141:wrap.testnet" { ONE.to_string() } else { (100 * ONE).to_string() }),
                        "current_salt" => json!("fc4d10f1"),
                        "simulate_intents" => json!({}),
                        other => panic!("unexpected native view {other}"),
                    };
                    return view_result(&value);
                }
                "send_tx" => return result(json!({
                    "status": { "SuccessValue": STANDARD.encode(serde_json::to_vec(&accepted.to_string()).unwrap()) },
                    "transaction": { "hash": "NATIVE_TX" },
                    "receipts_outcome": if receipt_fails { json!([{ "outcome": { "status": { "Failure": { "error": "unwrap failed" } } } }]) } else { json!([]) },
                })),
                _ => {}
            }
            healthy(method, params, |_| None)
        }).await
    }

    fn bridge(node: &MockRpc) -> NearBridge {
        NearBridge::new(
            node.url.clone(),
            "verifier.testnet",
            NearKeypair::from_secret_bytes(&[7; 32])
                .with_account_id(AccountId::try_from("alice.testnet").unwrap()),
            reqwest::Client::new(),
        )
        .unwrap()
    }

    // Inspect the actual signed transaction, including attached deposits and order.
    fn calls(node: &MockRpc) -> (String, Vec<FunctionCall>) {
        let recorded = node.calls.lock().unwrap();
        let params = &recorded
            .iter()
            .find(|(method, _)| method == "send_tx")
            .unwrap()
            .1;
        let tx = STANDARD
            .decode(params["signed_tx_base64"].as_str().unwrap())
            .unwrap();
        let mut bytes = tx.as_slice();
        fn take<'a>(bytes: &mut &'a [u8], length: usize) -> &'a [u8] {
            let (value, rest) = bytes.split_at(length);
            *bytes = rest;
            value
        }
        fn field(bytes: &mut &[u8]) -> Vec<u8> {
            let length = u32::from_le_bytes(take(bytes, 4).try_into().unwrap()) as usize;
            take(bytes, length).to_vec()
        }
        assert_eq!(
            String::from_utf8(field(&mut bytes)).unwrap(),
            "alice.testnet"
        );
        take(&mut bytes, 1 + 32 + 8);
        let receiver = String::from_utf8(field(&mut bytes)).unwrap();
        take(&mut bytes, 32);
        let count = u32::from_le_bytes(take(&mut bytes, 4).try_into().unwrap());
        let mut calls = Vec::new();
        for _ in 0..count {
            assert_eq!(take(&mut bytes, 1), &[2]);
            calls.push(FunctionCall {
                method_name: String::from_utf8(field(&mut bytes)).unwrap(),
                args: field(&mut bytes),
                gas: u64::from_le_bytes(take(&mut bytes, 8).try_into().unwrap()),
                deposit: u128::from_le_bytes(take(&mut bytes, 16).try_into().unwrap()),
            });
        }
        (receiver, calls)
    }

    #[tokio::test]
    async fn first_native_deposit_registers_wraps_and_transfers_exactly_once() {
        let node = node(false, ONE, false).await;
        let bridge = bridge(&node);
        let expected_available =
            2 * ONE - 100 * 10_000_000_000_000_000_000 - GAS_RESERVE - 2 * REGISTRATION;
        assert_eq!(
            bridge
                .native_deposit_available("wrap.testnet")
                .await
                .unwrap(),
            expected_available
        );
        assert_eq!(
            bridge.deposit_native("wrap.testnet", ONE).await.unwrap(),
            TxHash("NATIVE_TX".into())
        );
        let (receiver, calls) = calls(&node);
        assert_eq!(receiver, "wrap.testnet");
        assert_eq!(
            calls
                .iter()
                .map(|call| call.method_name.as_str())
                .collect::<Vec<_>>(),
            [
                "storage_deposit",
                "storage_deposit",
                "near_deposit",
                "ft_transfer_call"
            ]
        );
        assert_eq!(
            calls.iter().map(|call| call.deposit).collect::<Vec<_>>(),
            [REGISTRATION, REGISTRATION, ONE, 1]
        );
        let owners: Vec<Value> = calls[..2]
            .iter()
            .map(|call| serde_json::from_slice(&call.args).unwrap())
            .collect();
        assert_eq!(owners[0]["account_id"], "alice.testnet");
        assert_eq!(owners[1]["account_id"], "verifier.testnet");
        let args: Value = serde_json::from_slice(&calls[3].args).unwrap();
        assert_eq!(
            args,
            json!({ "receiver_id": "verifier.testnet", "amount": ONE.to_string(), "msg": "" })
        );
        assert!(calls.iter().map(|call| call.gas).sum::<u64>() <= GAS_EXECUTE_INTENTS);
        assert_eq!(
            node.order()
                .iter()
                .filter(|method| *method == "send_tx")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn existing_registration_is_not_paid_twice() {
        let node = node(true, ONE, false).await;
        bridge(&node)
            .deposit_native("wrap.testnet", ONE)
            .await
            .unwrap();
        let (_, calls) = calls(&node);
        assert_eq!(
            calls
                .iter()
                .map(|call| call.method_name.as_str())
                .collect::<Vec<_>>(),
            ["near_deposit", "ft_transfer_call"]
        );
    }

    #[tokio::test]
    async fn native_deposit_rejects_spending_the_gas_or_storage_reserve() {
        let node = node(false, ONE, false).await;
        assert!(matches!(
            bridge(&node).deposit_native("wrap.testnet", 2 * ONE).await,
            Err(NearError::InsufficientNative)
        ));
        assert!(!node.order().contains(&"send_tx".into()));
    }

    #[tokio::test]
    async fn refunded_or_partially_accepted_native_deposit_is_not_reported_successful() {
        for accepted in [0, ONE / 2] {
            let node = node(true, accepted, false).await;
            assert!(
                matches!(bridge(&node).deposit_native("wrap.testnet", ONE).await,
                Err(NearError::NativeTransferUnconfirmed { transaction: Some(hash) }) if hash == "NATIVE_TX")
            );
        }
    }

    #[tokio::test]
    async fn native_withdraw_uses_native_intent_and_only_the_backed_balance() {
        let node = node(true, ONE, false).await;
        let bridge = bridge(&node);
        assert!(matches!(
            bridge.withdraw_native("wrap.testnet", 2 * ONE, 1_000).await,
            Err(NearError::InsufficientNative)
        ));
        assert!(!node.order().contains(&"send_tx".into()));
        bridge
            .withdraw_native("wrap.testnet", ONE, 1_000)
            .await
            .unwrap();
        let (receiver, calls) = calls(&node);
        assert_eq!(receiver, "verifier.testnet");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method_name, "execute_intents");
        let args: Value = serde_json::from_slice(&calls[0].args).unwrap();
        let message: Value =
            serde_json::from_str(args["signed"][0]["payload"]["message"].as_str().unwrap())
                .unwrap();
        assert_eq!(
            message["intents"],
            json!([{ "intent": "native_withdraw", "receiver_id": "alice.testnet", "amount": ONE.to_string() }])
        );
        assert!(node.order().contains(&"simulate_intents".into()));
    }

    #[tokio::test]
    async fn wallet_unwrap_does_not_debit_intents_or_wrap_more_near() {
        let node = node(true, ONE, false).await;
        bridge(&node)
            .unwrap_native("wrap.testnet", ONE)
            .await
            .unwrap();
        let (receiver, calls) = calls(&node);
        assert_eq!(receiver, "wrap.testnet");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method_name, "near_withdraw");
        assert_eq!(calls[0].deposit, 1);
        assert_eq!(
            serde_json::from_slice::<Value>(&calls[0].args).unwrap()["amount"],
            ONE.to_string()
        );
        assert!(!node.order().contains(&"simulate_intents".into()));
    }

    #[tokio::test]
    async fn failed_unwrap_receipt_is_not_a_success() {
        let node = node(true, ONE, true).await;
        assert!(matches!(
            bridge(&node)
                .withdraw_native("wrap.testnet", ONE, 1_000)
                .await,
            Err(NearError::NativeTransferUnconfirmed { .. })
        ));
        assert!(matches!(
            bridge(&node).unwrap_native("wrap.testnet", ONE).await,
            Err(NearError::NativeTransferUnconfirmed { .. })
        ));
    }

    #[tokio::test]
    async fn cached_native_balance_does_not_alias_the_tidepool_token() {
        use crate::{
            balances::Balances,
            network_config::{assets, Network},
        };
        let node = node(true, ONE, false).await;
        let network = Network::Testnet;
        let balances = Balances::read(&bridge(&node), network, &assets(network.chain()))
            .await
            .unwrap();
        assert_eq!(
            balances.native_token.as_ref().unwrap().in_intents,
            ONE.to_string()
        );
        assert_eq!(balances.tokens["wNEAR"].in_intents, (100 * ONE).to_string());
        assert_eq!(balances.native_wrapper.as_deref(), Some("wrap.testnet"));
    }

    #[tokio::test]
    async fn mainnet_native_and_wnear_share_one_balance_read() {
        use crate::{
            balances::Balances,
            network_config::{assets, Network},
        };
        let node = node(true, ONE, false).await;
        let network = Network::Mainnet;
        let balances = Balances::read(&bridge(&node), network, &assets(network.chain()))
            .await
            .unwrap();
        assert_eq!(
            balances.native_token.as_ref().unwrap(),
            &balances.tokens["wNEAR"]
        );
        let calls = node.calls.lock().unwrap();
        let wrapper_reads = calls
            .iter()
            .filter(|(_, params)| {
                if params["method_name"] != "mt_balance_of" {
                    return false;
                }
                let args: Value = serde_json::from_slice(
                    &STANDARD
                        .decode(params["args_base64"].as_str().unwrap())
                        .unwrap(),
                )
                .unwrap();
                args["token_id"] == "nep141:wrap.near"
            })
            .count();
        assert_eq!(wrapper_reads, 1);
    }
}
