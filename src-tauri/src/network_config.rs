//! Which NEAR network to talk to, and where its contracts live.
//!
//! # Why environment variables are only a desktop layer
//!
//! A mobile bundle has **no environment to read and no file to load**, so the
//! values are a compiled-in table keyed by network, overridable at runtime for
//! anyone pointing the app at their own deployment.
//!
//! # Why the default is a testnet
//!
//! This build is still moving, and a wrong default here spends real money
//! rather than displaying something wrong. Promoting to mainnet is a deliberate
//! toggle.

use cabal_core::Chain;
use serde::{Deserialize, Serialize};

/// A NEAR network this app knows how to talk to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Network {
    /// NEAR testnet, through the Tidepool deployment of the Intents verifier.
    /// The default, deliberately.
    #[default]
    Testnet,
    /// NEAR mainnet. Real funds.
    Mainnet,
}

/// A token this app can hold and trade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetSpec {
    pub symbol: &'static str,
    /// Three-letter tag shown in lists.
    pub code: &'static str,
    pub decimals: u8,
    /// The NEP-141 contract.
    pub contract: &'static str,
}

impl AssetSpec {
    /// The id the verifier knows this token by.
    #[must_use]
    pub fn token_id(&self) -> String {
        format!("nep141:{}", self.contract)
    }
}

/// Where a network's Intents deployment lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NearConfig {
    pub rpc_url: &'static str,
    pub verifier: &'static str,
    pub base: AssetSpec,
    /// Every trade is `asset ⇄ quote` at the agreed USD price.
    pub quote: AssetSpec,
    pub price_feed: &'static str,
    /// The contract that pays gateways. `None` submits straight to the verifier
    /// and charges no fee. Set it only once the contract is deployed, since a
    /// fee signed to an account with no contract behind it pays nobody.
    pub rewards: Option<&'static str>,
    /// What each side pays the rewards contract, in the quote token's smallest unit.
    pub fee_per_side: u128,
    /// Block explorer, with no trailing slash.
    pub explorer: &'static str,
    pub faucets: &'static [&'static str],
}

const TESTNET: NearConfig = NearConfig {
    rpc_url: "https://rpc.testnet.fastnear.com",
    verifier: "verifier.tidepool-sandbox.testnet",
    base: AssetSpec {
        symbol: "wNEAR",
        code: "NEA",
        decimals: 24,
        contract: "wnear.tidepool-sandbox.testnet",
    },
    quote: AssetSpec {
        symbol: "USDC",
        code: "USD",
        decimals: 6,
        contract: "usdc.tidepool-sandbox.testnet",
    },
    price_feed: "https://www.usetidepool.tech/v0/tokens",
    rewards: None,
    fee_per_side: 10_000,
    explorer: "https://testnet.nearblocks.io",
    faucets: &[
        "https://near-faucet.io/",
        "https://www.usetidepool.tech/docs/guides/faucet",
    ],
};

const MAINNET: NearConfig = NearConfig {
    rpc_url: "https://rpc.mainnet.fastnear.com",
    verifier: "intents.near",
    base: AssetSpec {
        symbol: "wNEAR",
        code: "NEA",
        decimals: 24,
        contract: "wrap.near",
    },
    // Unverified: this id has not been checked against a real mainnet trade.
    quote: AssetSpec {
        symbol: "USDC",
        code: "USD",
        decimals: 6,
        contract: "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1",
    },
    price_feed: "https://1click.chaindefuser.com/v0/tokens",
    rewards: None,
    fee_per_side: 0,
    explorer: "https://nearblocks.io",
    faucets: &[],
};

impl NearConfig {
    /// The fee a trader signs, or `None` if this network pays no gateways.
    #[must_use]
    pub fn gateway_fee(&self) -> Option<cabal_near::GatewayFee> {
        Some(cabal_near::GatewayFee {
            to: cabal_near::AccountId::try_from(self.rewards?).ok()?,
            token: self.quote.token_id(),
            amount: self.fee_per_side,
        })
    }
}

impl Network {
    /// Reads the name a config file or `CABALMESH_NETWORK` uses.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(name.to_owned())).ok()
    }

    /// Human-readable name for logs and the profile screen.
    #[must_use]
    pub const fn label(self) -> &'static str {
        self.chain().label()
    }

    /// Whether transactions here move real value.
    ///
    /// The UI uses this to mark testnet plainly, so nobody mistakes a test
    /// balance for a real one.
    #[must_use]
    pub const fn is_testnet(self) -> bool {
        self.chain().is_testnet()
    }

    /// The chain a new intent names on this network.
    #[must_use]
    pub const fn chain(self) -> Chain {
        match self {
            Self::Testnet => Chain::NearTestnet,
            Self::Mainnet => Chain::NearMainnet,
        }
    }

    /// The compiled-in values for this network.
    #[must_use]
    pub const fn near(self) -> NearConfig {
        match self {
            Self::Testnet => TESTNET,
            Self::Mainnet => MAINNET,
        }
    }
}

/// The network a chain lives on.
#[must_use]
pub const fn network_of(chain: Chain) -> Network {
    match chain {
        Chain::NearTestnet => Network::Testnet,
        Chain::NearMainnet => Network::Mainnet,
    }
}

/// Every asset on a chain, tradable base first.
#[must_use]
pub fn assets(chain: Chain) -> Vec<AssetSpec> {
    let near = network_of(chain).near();
    vec![near.base, near.quote]
}

/// Resolved chain configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkConfig {
    #[serde(default)]
    pub network: Network,
    /// Overrides the network's RPC endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub near_rpc_url: Option<String>,
    /// Overrides the network's verifier, for a self-hosted deployment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub near_verifier: Option<String>,
}

impl NetworkConfig {
    /// Switches the active network and persists the choice.
    ///
    /// Reads and writes the raw file directly rather than going through
    /// [`NetworkConfig::load`] — that layers desktop environment overrides on
    /// top, and saving *that* back would bake a temporary env override into
    /// the persisted file forever.
    ///
    /// # Errors
    ///
    /// [`cabal_store::StoreError`] if the file cannot be written.
    pub fn switch(store: &cabal_store::JsonStore, network: Network) -> Result<(), cabal_store::StoreError> {
        let mut config: Self = store.load_or(Self::default());
        config.network = network;
        store.save(&config)
    }

    /// Loads configuration, layering: compiled-in defaults, then the config
    /// file, then environment variables on desktop.
    #[must_use]
    pub fn load(store: &cabal_store::JsonStore) -> Self {
        let mut config: Self = store.load_or(Self::default());

        #[cfg(desktop)]
        config.apply_environment_overrides();

        config
    }

    /// The configuration in the app's data directory.
    #[must_use]
    pub fn current() -> Self {
        Self::load(&cabal_store::JsonStore::new(crate::app_paths::in_data_dir("network.json")))
    }

    #[cfg(desktop)]
    fn apply_environment_overrides(&mut self) {
        fn var(name: &str) -> Option<String> {
            std::env::var(name).ok().filter(|value| !value.is_empty())
        }

        if let Some(network) = var("CABALMESH_NETWORK").as_deref().and_then(Network::from_name) {
            self.network = network;
        }
        if let Some(url) = var("NEAR_RPC_URL") {
            self.near_rpc_url = Some(url);
        }
        if let Some(verifier) = var("NEAR_VERIFIER") {
            self.near_verifier = Some(verifier);
        }
    }

    /// The compiled-in values for the active network.
    #[must_use]
    pub const fn near(&self) -> NearConfig {
        self.network.near()
    }

    /// The RPC endpoint to use.
    #[must_use]
    pub fn rpc_url(&self) -> String {
        self.near_rpc_url.clone().unwrap_or_else(|| self.near().rpc_url.to_owned())
    }

    /// The verifier to use.
    #[must_use]
    pub fn verifier(&self) -> String {
        self.near_verifier.clone().unwrap_or_else(|| self.near().verifier.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn a_network_name_reads_like_the_config_file() {
        assert_eq!(Network::from_name("testnet"), Some(Network::Testnet));
        assert_eq!(Network::from_name("mainnet"), Some(Network::Mainnet));
        assert_eq!(Network::from_name("near-testnet"), None);
    }

    #[test]
    fn the_default_is_a_testnet() {
        // A wrong default here spends real money rather than showing something
        // wrong, so mainnet must never be the fallback.
        assert_eq!(Network::default(), Network::Testnet);
        assert!(Network::default().is_testnet());
    }

    #[test]
    fn near_testnet_points_at_tidepool() {
        let near = Network::Testnet.near();
        assert_eq!(near.rpc_url, "https://rpc.testnet.fastnear.com");
        assert_eq!(near.verifier, "verifier.tidepool-sandbox.testnet");
        assert_eq!(near.base.contract, "wnear.tidepool-sandbox.testnet");
        assert_eq!(near.base.decimals, 24);
        assert_eq!(near.quote.contract, "usdc.tidepool-sandbox.testnet");
        assert_eq!(near.quote.decimals, 6);
        assert_eq!(near.price_feed, "https://www.usetidepool.tech/v0/tokens");
    }

    #[test]
    fn near_mainnet_points_at_intents_near() {
        let near = Network::Mainnet.near();
        assert_eq!(near.rpc_url, "https://rpc.mainnet.fastnear.com");
        assert_eq!(near.verifier, "intents.near");
        assert_eq!(near.base.contract, "wrap.near");
        assert_eq!(near.price_feed, "https://1click.chaindefuser.com/v0/tokens");
    }

    #[test]
    fn mainnet_charges_no_fee_until_its_contract_is_audited() {
        assert_eq!(Network::Mainnet.near().gateway_fee(), None);
    }

    #[test]
    fn a_gateway_fee_is_paid_in_the_quote_token() {
        let near = NearConfig {
            rewards: Some("rewards.test"),
            fee_per_side: 10_000,
            ..Network::Testnet.near()
        };
        let fee = near.gateway_fee().unwrap();
        assert_eq!(fee.to.as_str(), "rewards.test");
        assert_eq!(fee.token, near.quote.token_id());
        assert_eq!(fee.amount, 10_000);
    }

    #[test]
    fn each_network_names_its_own_chain() {
        assert_eq!(Network::Testnet.chain(), Chain::NearTestnet);
        assert_eq!(Network::Mainnet.chain(), Chain::NearMainnet);
        assert_eq!(network_of(Chain::NearTestnet), Network::Testnet);
    }

    #[test]
    fn assets_are_per_chain() {
        assert_eq!(assets(Chain::NearTestnet)[0].symbol, "wNEAR");
        assert_eq!(assets(Chain::NearTestnet)[1].symbol, "USDC");
    }

    #[test]
    fn a_config_naming_an_unknown_network_falls_back_to_testnet() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("network.json");
        std::fs::write(&path, r#"{"network":"fuji","rpcUrl":"https://example.test"}"#).unwrap();

        let store = cabal_store::JsonStore::new(&path);
        assert!(store.load::<NetworkConfig>().is_err(), "the name is not known");
        assert_eq!(NetworkConfig::load(&store).network, Network::Testnet);
    }

    #[test]
    fn an_explicit_verifier_wins_over_the_compiled_table() {
        let config = NetworkConfig {
            near_verifier: Some("intents.cabalmesh.testnet".into()),
            ..NetworkConfig::default()
        };
        assert_eq!(config.verifier(), "intents.cabalmesh.testnet");
    }

    #[test]
    fn switching_network_persists_and_is_read_back() {
        let dir = TempDir::new().unwrap();
        let store = cabal_store::JsonStore::new(dir.path().join("network.json"));

        NetworkConfig::switch(&store, Network::Mainnet).unwrap();

        let reloaded = NetworkConfig::load(&store);
        assert_eq!(reloaded.network, Network::Mainnet);
        assert!(!reloaded.network.is_testnet());
    }

    #[test]
    fn switching_network_preserves_other_config_fields() {
        let dir = TempDir::new().unwrap();
        let store = cabal_store::JsonStore::new(dir.path().join("network.json"));
        store
            .save(&NetworkConfig {
                network: Network::Testnet,
                near_rpc_url: Some("http://custom".into()),
                ..NetworkConfig::default()
            })
            .unwrap();

        NetworkConfig::switch(&store, Network::Mainnet).unwrap();

        let raw: NetworkConfig = store.load().unwrap();
        assert_eq!(raw.network, Network::Mainnet);
        assert_eq!(raw.near_rpc_url.as_deref(), Some("http://custom"));
    }

    #[test]
    fn a_partial_config_file_still_loads() {
        // Config gains fields over time; an older file must not become
        // unloadable when it does.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("network.json");
        std::fs::write(&path, r#"{"network":"mainnet"}"#).unwrap();

        let config = NetworkConfig::load(&cabal_store::JsonStore::new(&path));
        assert_eq!(config.network, Network::Mainnet);
        assert!(!config.network.is_testnet());
    }
}
