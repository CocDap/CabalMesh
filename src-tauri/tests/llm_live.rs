//! The client against a real local Ollama, through its OpenAI-compatible API.
//!
//! ```bash
//! cargo test --test llm_live -- --ignored --nocapture
//! ```

use cabal_core::{Chain, IntentId};
use cabal_negotiation::{Bound, Side};
use cabal_store::DocumentStore;
use cabalmesh_lib::agent::{self, Decision};
use cabalmesh_lib::agent_tools::ToolContext;
use cabalmesh_lib::llm_budget::LlmBudget;
use cabalmesh_lib::llm_client::LlmClient;
use cabalmesh_lib::llm_config::LlmSettings;
use cabalmesh_lib::network_config::Network;
use cabalmesh_lib::price_feed::PriceFeed;
use std::sync::{Arc, Mutex};

fn client() -> LlmClient {
    LlmClient::new(
        LlmSettings::default(),
        None,
        reqwest::Client::new(),
        Arc::new(LlmBudget::new(10)),
    )
}

#[tokio::test]
#[ignore = "needs Ollama running with the default model"]
async fn the_default_settings_reach_a_local_ollama() {
    let answer = client()
        .chat_json(
            "Respond ONLY with a JSON object.",
            r#"Return exactly {"price": 4.05}"#,
        )
        .await
        .expect("the model answers");
    println!("chat_json: {answer}");
    assert_eq!(answer["price"], 4.05);
}

#[tokio::test]
#[ignore = "needs Ollama running with the default model"]
async fn the_probe_describes_a_local_ollama() {
    let probe = client().probe().await;
    println!("probe: {probe:?}");
    assert!(probe.reachable && probe.json_ok, "{probe:?}");
}

async fn live_context(
    side: Side,
    offset_from_market: i64,
    amount: &str,
    dir: &tempfile::TempDir,
) -> ToolContext {
    let feed = PriceFeed::open(
        Network::Testnet.near().price_feed,
        reqwest::Client::new(),
        DocumentStore::new(dir.path().join("prices.json")),
    );
    let now_ms = u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap();
    let market = feed
        .quote("wNEAR", now_ms)
        .await
        .expect("the feed has wNEAR");
    let limit = market
        .usd_cents
        .checked_add_signed(offset_from_market)
        .unwrap();
    println!("{side:?} market {} limit {limit}", market.usd_cents);
    ToolContext {
        chain: Chain::NearTestnet,
        asset: "wNEAR".into(),
        own_intent: IntentId::new("LIVE"),
        bound: Bound {
            side,
            limit_cents: Some(limit),
        },
        amount: amount.into(),
        balance: Some("40".into()),
        round: 1,
        mesh_offers: Vec::new(),
        feed: Arc::new(feed),
        now_ms,
        proposed: Mutex::new(None),
    }
}

/// A real model, the real tools and the real price feed. A slow model can hit the 30 s
/// limit on a turn, which is `Nothing`: this prints it rather than failing on it, and
/// fails only if a price past the limit gets out.
#[tokio::test]
#[ignore = "needs Ollama running with the default model, and internet for the price feed"]
async fn a_real_model_stays_inside_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    let (mut turns, mut offers) = (0, 0);
    for amount in ["1", "0.1", "2.5"] {
        for (side, offset) in [
            (Side::Sell, 3),
            (Side::Sell, -4),
            (Side::Buy, 26),
            (Side::Buy, -3),
        ] {
            let ctx = live_context(side, offset, amount, &dir).await;
            let started = std::time::Instant::now();
            let decision = agent::decide(&client(), &ctx, &[]).await;
            println!(
                "{side:?} {amount} {offset:+}: {decision:?} after {:?}",
                started.elapsed()
            );

            assert!(
                !matches!(decision, Decision::ModelUnavailable { .. }),
                "Ollama is not up"
            );
            turns += 1;
            if let Decision::Propose { price_cents } = decision {
                offers += 1;
                assert!(
                    ctx.bound.allows(price_cents),
                    "{price_cents} is past {:?}",
                    ctx.bound
                );
            }
        }
    }
    println!("{offers} offers in {turns} turns");
}
