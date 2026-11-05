//! The last balances this device read, kept for when it cannot read them.
//!
//! A device composing over Bluetooth has no RPC, and a blank screen would claim
//! it holds nothing. A cached figure is shown as what it is: the last one seen.

use crate::near_bridge::{NearBridge, NearError};
use crate::network_config::{AssetSpec, Network};
use cabal_core::{Action, Condition, IntentDraft};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Where the last read is kept.
#[must_use]
pub fn store() -> cabal_store::JsonStore {
    cabal_store::JsonStore::new(crate::app_paths::in_data_dir("near_balances.json"))
}

/// One token's raw amounts, as decimal strings: `u128` does not survive every
/// JSON reader.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenBalance {
    pub wallet: String,
    pub in_intents: String,
}

impl TokenBalance {
    /// What can be traded right now: only a verifier balance can settle.
    #[must_use]
    pub fn in_intents_raw(&self) -> Option<u128> {
        self.in_intents.parse().ok()
    }
}

/// A cached read of one account on one network.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Balances {
    pub account: String,
    pub network: Option<Network>,
    /// Native NEAR in yocto, which pays for gas.
    pub native: Option<String>,
    /// By asset symbol.
    pub tokens: BTreeMap<String, TokenBalance>,
}

impl Balances {
    /// Reads every balance. Nothing is cached unless all of it succeeded, so a
    /// half-finished read cannot replace a good one.
    ///
    /// # Errors
    ///
    /// Any RPC failure.
    pub async fn read(bridge: &NearBridge, network: Network, assets: &[AssetSpec]) -> Result<Self, NearError> {
        let mut tokens = BTreeMap::new();
        for asset in assets {
            tokens.insert(
                asset.symbol.to_owned(),
                TokenBalance {
                    wallet: bridge.wallet_balance(asset.contract).await?.to_string(),
                    in_intents: bridge.verifier_balance(asset.contract).await?.to_string(),
                },
            );
        }
        Ok(Self {
            account: bridge.account_id().to_string(),
            network: Some(network),
            native: Some(bridge.native_balance().await?.to_string()),
            tokens,
        })
    }

    /// The cached read, unless it belongs to another account or network: after
    /// an import or a switch those figures would describe somebody else.
    #[must_use]
    pub fn load_for(store: &cabal_store::JsonStore, account: &str, network: Network) -> Self {
        let cached: Self = store.load_or(Self::default());
        if cached.account == account && cached.network == Some(network) {
            cached
        } else {
            Self::default()
        }
    }

    /// `asset`'s tradable amount, or `None` if it is not known.
    #[must_use]
    pub fn in_intents_raw(&self, symbol: &str) -> Option<u128> {
        self.tokens.get(symbol)?.in_intents_raw()
    }
}

/// Refuses a draft this account could not settle, when that is known.
///
/// An unknown balance passes: composing offline is a supported path, and the
/// verifier refuses an unfunded diff anyway. A sell needs the base; a buy needs
/// the most its ceiling could cost, and with no ceiling there is no figure to
/// check.
#[must_use]
pub fn covers(balances: &Balances, draft: &IntentDraft, base: &AssetSpec, quote: &AssetSpec) -> bool {
    match draft.action {
        Action::Sell => balances.in_intents_raw(base.symbol).is_none_or(|held| held >= draft.amount.raw()),
        Action::Buy => {
            let Condition::Under { price } = draft.condition else {
                return true;
            };
            let Some(cost) = cabal_near::quote_amount(draft.amount.raw(), base.decimals, price.cents(), quote.decimals)
            else {
                return false;
            };
            balances.in_intents_raw(quote.symbol).is_none_or(|held| held >= cost)
        }
        Action::Swap | Action::Stake => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cabal_core::{Chain, ExecutionMode, PrivacyLevel, TokenAmount, UsdPrice};
    use tempfile::TempDir;

    const WNEAR: u128 = 1_000_000_000_000_000_000_000_000;

    fn specs() -> (AssetSpec, AssetSpec) {
        let near = Network::Testnet.near();
        (near.base, near.quote)
    }

    fn held(wnear: u128, usdc: u128) -> Balances {
        let mut balances = Balances::default();
        for (symbol, amount) in [("wNEAR", wnear), ("USDC", usdc)] {
            balances.tokens.insert(
                symbol.to_owned(),
                TokenBalance { wallet: "0".into(), in_intents: amount.to_string() },
            );
        }
        balances
    }

    fn draft(action: Action, amount: &str, condition: Condition) -> IntentDraft {
        IntentDraft {
            chain: Chain::NearTestnet,
            action,
            asset: "wNEAR".into(),
            condition,
            amount: TokenAmount::parse(amount, 24).unwrap(),
            mode: ExecutionMode::Shark,
            privacy: PrivacyLevel::Low,
        }
    }

    #[test]
    fn broadcast_rejects_near_amount_above_intents_balance() {
        let (base, quote) = specs();
        let sell = draft(Action::Sell, "1", Condition::Any);
        assert!(!covers(&held(WNEAR / 2, 0), &sell, &base, &quote));
        assert!(covers(&held(WNEAR, 0), &sell, &base, &quote));
    }

    #[test]
    fn a_buy_is_checked_against_quote_at_its_ceiling() {
        let (base, quote) = specs();
        let buy = draft(Action::Buy, "1", Condition::Under { price: UsdPrice::from_cents(420) });
        assert!(!covers(&held(0, 4_199_999), &buy, &base, &quote));
        assert!(covers(&held(0, 4_200_000), &buy, &base, &quote));
    }

    #[test]
    fn a_buy_with_no_ceiling_has_no_figure_to_check() {
        let (base, quote) = specs();
        assert!(covers(&held(0, 0), &draft(Action::Buy, "1", Condition::Any), &base, &quote));
    }

    #[test]
    fn an_unknown_balance_lets_the_draft_through() {
        let (base, quote) = specs();
        assert!(covers(&Balances::default(), &draft(Action::Sell, "1", Condition::Any), &base, &quote));
    }

    #[test]
    fn a_cached_read_belongs_to_one_account_on_one_network() {
        let dir = TempDir::new().unwrap();
        let store = cabal_store::JsonStore::new(dir.path().join("balances.json"));
        let mut balances = held(WNEAR, 1);
        balances.account = "alice".into();
        balances.network = Some(Network::Testnet);
        store.save(&balances).unwrap();

        assert_eq!(Balances::load_for(&store, "alice", Network::Testnet), balances);
        assert_eq!(Balances::load_for(&store, "bob", Network::Testnet), Balances::default());
        assert_eq!(Balances::load_for(&store, "alice", Network::Mainnet), Balances::default());
    }
}
