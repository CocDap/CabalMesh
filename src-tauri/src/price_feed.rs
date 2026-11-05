//! Market prices, from the `/v0/tokens` of the verifier in use.
//!
//! Same source as where trades settle, and it needs no API key. Prices are kept on
//! disk because an offline device can still trade over Bluetooth, and it needs to know
//! how old the last price it saw is.

use cabal_store::JsonStore;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

/// Past this, the agent may suggest a price but not send one.
pub const STALE_AFTER_MS: u64 = 30 * 60 * 1000;
/// A turn can ask several times. One read of the feed answers them all.
const FETCH_EVERY_MS: u64 = 30_000;
/// Short, because a decision turn only has 30 s and this may simply be offline.
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceQuote {
    pub symbol: String,
    pub usd_cents: u64,
    pub updated_at_ms: u64,
    pub stale: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Saved {
    usd_cents: u64,
    updated_at_ms: u64,
}

#[derive(Default)]
struct Cache {
    /// By lowercase symbol.
    quotes: BTreeMap<String, Saved>,
    last_fetch_ms: Option<u64>,
}

pub struct PriceFeed {
    url: String,
    http: reqwest::Client,
    store: JsonStore,
    cache: Mutex<Cache>,
}

impl PriceFeed {
    /// Opens the feed, adopting whatever prices were saved.
    #[must_use]
    pub fn open(url: &str, http: reqwest::Client, store: JsonStore) -> Self {
        let quotes = store.load_or(BTreeMap::new());
        Self {
            url: url.to_string(),
            http,
            store,
            cache: Mutex::new(Cache {
                quotes,
                last_fetch_ms: None,
            }),
        }
    }

    /// The latest price known for `symbol`. Reads the feed when online; otherwise this
    /// is the last price seen, for the caller to judge by `stale`.
    pub async fn quote(&self, symbol: &str, now_ms: u64) -> Option<PriceQuote> {
        self.refresh(now_ms).await;
        let saved = *self.lock().quotes.get(&symbol.to_ascii_lowercase())?;
        Some(PriceQuote {
            symbol: symbol.to_string(),
            usd_cents: saved.usd_cents,
            updated_at_ms: saved.updated_at_ms,
            stale: now_ms.saturating_sub(saved.updated_at_ms) > STALE_AFTER_MS,
        })
    }

    async fn refresh(&self, now_ms: u64) {
        {
            let mut cache = self.lock();
            if cache
                .last_fetch_ms
                .is_some_and(|last| now_ms.saturating_sub(last) < FETCH_EVERY_MS)
            {
                return;
            }
            // Counted when it starts, so a dead network costs one timeout per window.
            cache.last_fetch_ms = Some(now_ms);
        }
        let Some(fresh) = self.fetch().await else {
            return;
        };
        let snapshot = {
            let mut cache = self.lock();
            cache.quotes.extend(fresh);
            cache.quotes.clone()
        };
        if let Err(error) = self.store.save(&snapshot) {
            tracing::warn!(target: "cabalmesh::price", %error, "could not save prices");
        }
    }

    async fn fetch(&self) -> Option<BTreeMap<String, Saved>> {
        let response = self
            .http
            .get(&self.url)
            .timeout(FETCH_TIMEOUT)
            .send()
            .await
            .ok()?;
        let tokens: Vec<Token> = response.error_for_status().ok()?.json().await.ok()?;
        Some(tokens.iter().filter_map(Token::saved).collect())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One row of `/v0/tokens`. Unknown fields are ignored and a bad row is skipped, so
/// one odd token cannot take away the price of every other.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Token {
    symbol: String,
    #[serde(default)]
    blockchain: Option<String>,
    price: Option<f64>,
    price_updated_at: Option<String>,
}

impl Token {
    fn saved(&self) -> Option<(String, Saved)> {
        // Mainnet lists the same symbol once per chain, and trades here settle on NEAR.
        if self
            .blockchain
            .as_deref()
            .is_some_and(|chain| chain != "near")
        {
            return None;
        }
        let price = self
            .price
            .filter(|price| price.is_finite() && *price > 0.0)?;
        let updated =
            chrono::DateTime::parse_from_rfc3339(self.price_updated_at.as_deref()?).ok()?;
        let saved = Saved {
            usd_cents: (price * 100.0).round() as u64,
            updated_at_ms: u64::try_from(updated.timestamp_millis()).ok()?,
        };
        Some((self.symbol.to_ascii_lowercase(), saved))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::llm_client::mock;

    /// 2026-11-05T12:00:00Z
    pub const NOON_MS: u64 = 1_793_880_000_000;

    /// A `/v0/tokens` body with wNEAR at `price`, updated at noon.
    pub fn tokens_body(price: f64) -> String {
        serde_json::json!([
            { "symbol": "USDC", "blockchain": "near", "price": 0.9998, "priceUpdatedAt": "2026-11-05T12:00:00.000Z", "decimals": 6 },
            { "symbol": "wNEAR", "blockchain": "near", "price": price, "priceUpdatedAt": "2026-11-05T12:00:00.000Z", "decimals": 24 },
            { "symbol": "wNEAR", "blockchain": "eth", "price": 9.99, "priceUpdatedAt": "2026-11-05T12:00:00.000Z" },
            { "symbol": "ODD", "blockchain": "near", "price": null },
        ])
        .to_string()
    }

    pub fn feed_at(url: &str, dir: &tempfile::TempDir) -> PriceFeed {
        PriceFeed::open(
            url,
            reqwest::Client::new(),
            JsonStore::new(dir.path().join("prices.json")),
        )
    }

    /// A feed that has already read wNEAR at `price`, and whose server has gone away.
    pub async fn loaded_feed(price: f64, dir: &tempfile::TempDir) -> PriceFeed {
        let (url, server) = mock::serve(vec![(200, tokens_body(price))]);
        let feed = feed_at(&url, dir);
        feed.quote("wNEAR", NOON_MS).await.unwrap();
        server.join().unwrap();
        feed
    }

    #[test]
    fn noon_is_the_time_the_bodies_say() {
        let noon = chrono::DateTime::parse_from_rfc3339("2026-11-05T12:00:00.000Z").unwrap();
        assert_eq!(u64::try_from(noon.timestamp_millis()).unwrap(), NOON_MS);
    }

    #[tokio::test]
    async fn quote_parses_tidepool_tokens_body() {
        let dir = tempfile::tempdir().unwrap();
        let feed = loaded_feed(4.04, &dir).await;
        let quote = feed.quote("wNEAR", NOON_MS + 1).await.unwrap();
        assert_eq!(quote.usd_cents, 404, "the NEAR row, not the eth one");
        assert_eq!(quote.updated_at_ms, NOON_MS);
        assert!(!quote.stale);
    }

    #[tokio::test]
    async fn an_unknown_symbol_has_no_quote() {
        let dir = tempfile::tempdir().unwrap();
        let feed = loaded_feed(4.04, &dir).await;
        assert_eq!(feed.quote("ODD", NOON_MS + 1).await, None);
        assert_eq!(feed.quote("DOGE", NOON_MS + 1).await, None);
    }

    #[tokio::test]
    async fn quote_is_stale_after_30_minutes() {
        let dir = tempfile::tempdir().unwrap();
        let feed = loaded_feed(4.04, &dir).await;
        assert!(
            !feed
                .quote("wNEAR", NOON_MS + STALE_AFTER_MS)
                .await
                .unwrap()
                .stale
        );
        assert!(
            feed.quote("wNEAR", NOON_MS + STALE_AFTER_MS + 1)
                .await
                .unwrap()
                .stale
        );
    }

    #[tokio::test]
    async fn offline_returns_cached_quote_marked_stale() {
        let dir = tempfile::tempdir().unwrap();
        // The server is gone by now, so this refresh fails.
        let feed = loaded_feed(4.04, &dir).await;

        let recent = feed
            .quote("wNEAR", NOON_MS + FETCH_EVERY_MS + 1)
            .await
            .unwrap();
        assert_eq!(
            (recent.usd_cents, recent.stale),
            (404, false),
            "offline alone is not stale"
        );

        let old = feed
            .quote("wNEAR", NOON_MS + 2 * STALE_AFTER_MS)
            .await
            .unwrap();
        assert_eq!((old.usd_cents, old.stale), (404, true));
    }

    #[tokio::test]
    async fn a_saved_price_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        drop(loaded_feed(4.04, &dir).await);

        let restarted = feed_at(&mock::refused_url(), &dir);
        assert_eq!(
            restarted
                .quote("wNEAR", NOON_MS + 1)
                .await
                .unwrap()
                .usd_cents,
            404
        );
    }

    #[tokio::test]
    async fn the_feed_is_read_once_per_window() {
        let dir = tempfile::tempdir().unwrap();
        let (url, server) = mock::serve(vec![(200, tokens_body(4.04))]);
        let feed = feed_at(&url, &dir);
        for offset in 0..5 {
            feed.quote("wNEAR", NOON_MS + offset).await.unwrap();
        }
        assert_eq!(server.join().unwrap().len(), 1);
    }
}
