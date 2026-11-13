//! The last balances this device read, kept for when it cannot read them.
//!
//! A device composing over Bluetooth has no RPC, and a blank screen would claim
//! it holds nothing. A cached figure is shown as what it is: the last one seen.

use crate::near_bridge::{NearBridge, NearError};
use crate::network_config::{AssetSpec, NearConfig, Network};
use cabal_core::{Action, Condition, IntentDraft};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Where the last read is kept.
#[must_use]
pub fn store() -> BalanceStore {
    BalanceStore::new(crate::app_paths::in_data_dir("near_balances.json"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedAsset {
    pub symbol: String,
    pub contract: String,
    pub decimals: u8,
}

impl From<&AssetSpec> for CachedAsset {
    fn from(spec: &AssetSpec) -> Self {
        Self { symbol: spec.symbol.into(), contract: spec.contract.into(), decimals: spec.decimals }
    }
}

/// Account snapshots are separate from settings documents and are only saved by
/// a completed chain refresh (or the one-time legacy import).
#[derive(Clone)]
pub struct BalanceStore { legacy: cabal_store::DocumentStore }

impl BalanceStore {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self { legacy: cabal_store::DocumentStore::new(path) }
    }

    fn database(&self) -> Result<cabal_store::Database, cabal_store::StoreError> {
        let database = self.legacy.database()?;
        if !database.migrated("account:balances")? {
            let legacy = match self.legacy.load_optional::<Balances>() {
                Ok(legacy) => legacy,
                Err(error @ cabal_store::StoreError::Decode { .. }) => {
                    tracing::warn!(%error,"legacy balance cache is invalid; keeping its file as a backup");
                    None
                }
                Err(error) => return Err(error),
            };
            let snapshot = legacy.as_ref().filter(|old| old.network.is_some() && !old.account.is_empty())
                .map(|old| self.snapshot(old)).transpose()?;
            database.import_snapshot("account:balances", snapshot.as_ref())?;
        }
        Ok(database)
    }

    fn snapshot(&self, balances: &Balances) -> Result<cabal_store::SnapshotRow, cabal_store::StoreError> {
        let path = self.legacy.path().to_path_buf();
        let invalid = |reason| cabal_store::StoreError::InvalidData { path: path.clone(), reason };
        let network = balances.network.ok_or_else(|| invalid("snapshot has no network"))?;
        if balances.account.is_empty() { return Err(invalid("snapshot has no account")); }
        let assets: Vec<CachedAsset> = if balances.assets.is_empty() {
            crate::network_config::assets(network.chain()).iter().map(CachedAsset::from).collect()
        } else { balances.assets.clone() };
        let mut tokens = Vec::new();
        for asset in &assets {
            if let Some(balance) = balances.tokens.get(&asset.symbol) {
                tokens.push(cabal_store::TokenRow { contract: asset.contract.clone(), symbol: asset.symbol.clone(), decimals: asset.decimals,
                    wallet: balance.wallet.clone(), in_intents: balance.in_intents.clone() });
            }
        }
        if let (Some(wrapper), Some(token)) = (&balances.native_wrapper, &balances.native_token) {
            if !tokens.iter().any(|token| &token.contract == wrapper) {
                tokens.push(cabal_store::TokenRow { contract: wrapper.clone(), symbol: "wNEAR".into(), decimals: 24,
                    wallet: token.wallet.clone(), in_intents: token.in_intents.clone() });
            }
        }
        for amount in tokens.iter().flat_map(|token| [&token.wallet,&token.in_intents])
            .chain(balances.native.iter()).chain(balances.native_available.iter()) {
            if amount.parse::<u128>().is_err() { return Err(invalid("invalid raw token amount")); }
        }
        Ok(cabal_store::SnapshotRow {
            account: balances.account.clone(), network: network_key(network).into(), network_label: network.label().into(),
            native: balances.native.clone(), native_available: balances.native_available.clone(), wrapper: balances.native_wrapper.clone(),
            updated_ms: i64::try_from(crate::intents::now_ms()).unwrap_or(i64::MAX),
            payload: serde_json::to_string(balances).map_err(|source| cabal_store::StoreError::Encode { path, source })?, tokens,
        })
    }

    pub fn save(&self, balances: &Balances) -> Result<(), cabal_store::StoreError> {
        let snapshot = self.snapshot(balances)?;
        self.database()?.save_snapshot(&snapshot)
    }

    pub fn load(&self, account: &str, network: Network) -> Result<Balances, cabal_store::StoreError> {
        match self.database()?.snapshot(account, network_key(network))? {
            Some(raw) => serde_json::from_str(&raw).map_err(|source| cabal_store::StoreError::Decode { path: self.legacy.path().to_path_buf(), source }),
            None => Ok(Balances::default()),
        }
    }
}

fn network_key(network: Network) -> &'static str {
    match network { Network::Testnet => "near-testnet", Network::Mainnet => "near-mainnet" }
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
    /// NEAR-backed wrapper balances, never the unrelated Tidepool faucet token.
    #[serde(default)]
    pub native_token: Option<TokenBalance>,
    #[serde(default)]
    pub native_available: Option<String>,
    #[serde(default)]
    pub native_wrapper: Option<String>,
    #[serde(default)]
    pub assets: Vec<CachedAsset>,
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
        let wrapper = network.near().wrapped_native;
        let native_token = if let Some(asset) = assets.iter().find(|asset| asset.contract == wrapper) {
            tokens.get(asset.symbol).cloned()
                .ok_or_else(|| NearError::Rpc("wrapper balance was not read".into()))?
        } else {
            TokenBalance {
                wallet: bridge.wallet_balance(wrapper).await?.to_string(),
                in_intents: bridge.verifier_balance(wrapper).await?.to_string(),
            }
        };
        Ok(Self {
            account: bridge.account_id().to_string(),
            network: Some(network),
            native: Some(bridge.native_balance().await?.to_string()),
            native_token: Some(native_token),
            native_available: Some(bridge.native_deposit_available(wrapper).await?.to_string()),
            native_wrapper: Some(wrapper.into()),
            assets: assets.iter().map(CachedAsset::from).collect(),
            tokens,
        })
    }

    /// The cached read, unless it belongs to another account or network: after
    /// an import or a switch those figures would describe somebody else.
    #[must_use]
    pub fn load_for(store: &BalanceStore, account: &str, network: Network) -> Self {
        let cached = match store.load(account, network) {
            Ok(cached) => cached,
            Err(error) => { tracing::warn!(%error,"could not read cached account"); Self::default() }
        };
        if cached.account == account && cached.network == Some(network) {
            let mut cached = cached;
            if cached.native_wrapper.as_deref() != Some(network.near().wrapped_native) {
                cached.native_token = None;
                cached.native_available = None;
            }
            cached
        } else {
            Self::default()
        }
    }

    /// Whether this read shows nothing worth stealing: native NEAR at or
    /// below `dust_yocto` and every token at zero. Anything unknown —
    /// never read, an unparseable figure — is *not* empty, because this
    /// gates skipping the restore delay and must fail towards the delay.
    #[must_use]
    pub fn is_known_empty(&self, dust_yocto: u128) -> bool {
        let Some(native) = self.native.as_deref().and_then(|n| n.parse::<u128>().ok()) else { return false };
        let zero = |t: &TokenBalance| t.wallet.parse::<u128>() == Ok(0) && t.in_intents.parse::<u128>() == Ok(0);
        native <= dust_yocto && self.tokens.values().all(zero) && self.native_token.as_ref().is_none_or(zero)
    }

    /// `asset`'s tradable amount, or `None` if it is not known.
    #[must_use]
    pub fn in_intents_raw(&self, symbol: &str) -> Option<u128> {
        self.tokens.get(symbol)?.in_intents_raw()
    }

    pub fn token_for(&self, spec: &AssetSpec) -> Option<&TokenBalance> {
        if !self.assets.is_empty() && !self.assets.iter().any(|asset| asset.symbol == spec.symbol && asset.contract == spec.contract && asset.decimals == spec.decimals) {
            return None;
        }
        self.tokens.get(spec.symbol)
    }
}

/// Refuses a draft this account could not settle, when that is known.
///
/// An unknown balance passes: composing offline is a supported path, and the
/// verifier refuses an unfunded diff anyway. A sell needs the base; a buy needs
/// the most its ceiling could cost, and with no ceiling there is no figure to
/// check.
#[must_use]
pub fn covers(balances: &Balances, draft: &IntentDraft, near: &NearConfig) -> bool {
    let (base, quote) = (&near.base, &near.quote);
    match draft.action {
        // The seller's fee comes out of what the trade pays them.
        Action::Sell => balances
            .token_for(base).and_then(TokenBalance::in_intents_raw)
            .is_none_or(|held| held >= draft.amount.raw()),
        Action::Buy => {
            let Condition::Under { price } = draft.condition else {
                return true;
            };
            let fee = near.gateway_fee().map_or(0, |fee| fee.amount);
            let Some(cost) = cabal_near::quote_amount(
                draft.amount.raw(),
                base.decimals,
                price.cents(),
                quote.decimals,
            )
            .and_then(|cost| cost.checked_add(fee)) else {
                return false;
            };
            balances.token_for(quote).and_then(TokenBalance::in_intents_raw).is_none_or(|held| held >= cost)
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

    fn testnet() -> NearConfig {
        Network::Testnet.near()
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
        let sell = draft(Action::Sell, "1", Condition::Any);
        assert!(!covers(&held(WNEAR / 2, 0), &sell, &testnet()));
        assert!(covers(&held(WNEAR, 0), &sell, &testnet()));
    }

    #[test]
    fn a_buy_is_checked_against_quote_at_its_ceiling() {
        let buy = draft(
            Action::Buy,
            "1",
            Condition::Under {
                price: UsdPrice::from_cents(420),
            },
        );
        let near = NearConfig { rewards: None, ..testnet() };
        assert!(!covers(&held(0, 4_199_999), &buy, &near));
        assert!(covers(&held(0, 4_200_000), &buy, &near));
    }

    #[test]
    fn a_buy_must_also_cover_the_gateway_fee() {
        let near = NearConfig {
            rewards: Some("rewards.test"),
            fee_per_side: 10_000,
            ..testnet()
        };
        let buy = draft(
            Action::Buy,
            "1",
            Condition::Under {
                price: UsdPrice::from_cents(420),
            },
        );
        assert!(!covers(&held(0, 4_200_000), &buy, &near));
        assert!(covers(&held(0, 4_210_000), &buy, &near));
    }

    #[test]
    fn a_sell_needs_none_of_the_quote_for_its_fee() {
        let near = NearConfig {
            rewards: Some("rewards.test"),
            fee_per_side: 10_000,
            ..testnet()
        };
        assert!(covers(
            &held(WNEAR, 0),
            &draft(Action::Sell, "1", Condition::Any),
            &near
        ));
    }

    #[test]
    fn a_buy_with_no_ceiling_has_no_figure_to_check() {
        assert!(covers(
            &held(0, 0),
            &draft(Action::Buy, "1", Condition::Any),
            &testnet()
        ));
    }

    #[test]
    fn an_unknown_balance_lets_the_draft_through() {
        assert!(covers(
            &Balances::default(),
            &draft(Action::Sell, "1", Condition::Any),
            &testnet()
        ));
    }

    #[test]
    fn a_cached_read_belongs_to_one_account_on_one_network() {
        let dir = TempDir::new().unwrap();
        let store = BalanceStore::new(dir.path().join("balances.json"));
        let mut balances = held(WNEAR, 1);
        balances.account = "alice".into();
        balances.network = Some(Network::Testnet);
        store.save(&balances).unwrap();

        assert_eq!(Balances::load_for(&store, "alice", Network::Testnet), balances);
        assert_eq!(Balances::load_for(&store, "bob", Network::Testnet), Balances::default());
        assert_eq!(Balances::load_for(&store, "alice", Network::Mainnet), Balances::default());
    }

    #[test]
    fn legacy_account_cache_migrates_without_overwriting_other_accounts() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("near_balances.json");
        let mut old = held(WNEAR,1);
        old.account = "alice.testnet".into(); old.network = Some(Network::Testnet);
        cabal_store::JsonStore::new(&path).save(&old).unwrap();
        let backup = std::fs::read(&path).unwrap();
        let store = BalanceStore::new(&path);
        assert_eq!(store.load("alice.testnet",Network::Testnet).unwrap(),old);
        let mut next = held(WNEAR * 2,2);
        next.account = "bob.testnet".into(); next.network = Some(Network::Testnet);
        store.save(&next).unwrap();
        assert_eq!(store.load("bob.testnet",Network::Testnet).unwrap(),next);
        assert_eq!(store.load("alice.testnet",Network::Testnet).unwrap(),old);
        assert_eq!(std::fs::read(&path).unwrap(),backup);
    }

    #[test]
    fn a_fresh_refresh_can_replace_a_corrupt_legacy_cache() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("near_balances.json");
        std::fs::write(&path,"broken JSON").unwrap();
        let store = BalanceStore::new(&path);
        assert_eq!(store.load("alice.testnet",Network::Testnet).unwrap(),Balances::default());
        let mut fresh = held(WNEAR,1);
        fresh.account = "alice.testnet".into(); fresh.network = Some(Network::Testnet);
        store.save(&fresh).unwrap();
        assert_eq!(store.load("alice.testnet",Network::Testnet).unwrap(),fresh);
        assert_eq!(std::fs::read_to_string(path).unwrap(),"broken JSON");
    }
}

#[cfg(test)]
mod emptiness_tests {
    use super::*;

    fn read(native: Option<&str>, token: &str) -> Balances {
        let mut tokens = BTreeMap::new();
        tokens.insert("USDC".to_owned(), TokenBalance { wallet: token.to_owned(), in_intents: "0".to_owned() });
        Balances { native: native.map(str::to_owned), tokens, ..Balances::default() }
    }

    #[test]
    fn only_a_known_dust_balance_counts_as_empty() {
        assert!(read(Some("10"), "0").is_known_empty(100));
        assert!(!read(Some("101"), "0").is_known_empty(100), "native above dust");
        assert!(!read(Some("10"), "1").is_known_empty(100), "a token balance");
        assert!(!read(None, "0").is_known_empty(100), "never read");
        assert!(!read(Some("10"), "garbage").is_known_empty(100), "unparseable");
    }
}
