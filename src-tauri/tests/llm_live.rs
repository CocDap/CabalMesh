//! The client against a real local Ollama, through its OpenAI-compatible API.
//!
//! ```bash
//! cargo test --test llm_live -- --ignored --nocapture
//! ```

use cabalmesh_lib::llm_budget::LlmBudget;
use cabalmesh_lib::llm_client::LlmClient;
use cabalmesh_lib::llm_config::LlmSettings;
use std::sync::Arc;

fn client() -> LlmClient {
    LlmClient::new(LlmSettings::default(), None, reqwest::Client::new(), Arc::new(LlmBudget::new(10)))
}

#[tokio::test]
#[ignore = "needs Ollama running with the default model"]
async fn the_default_settings_reach_a_local_ollama() {
    let answer = client()
        .chat_json("Respond ONLY with a JSON object.", r#"Return exactly {"price": 4.05}"#)
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
