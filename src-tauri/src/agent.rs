//! One decision turn of the negotiating agent.
//!
//! The model gets the four tools in [`crate::agent_tools`], and whatever it asks for is
//! run here. The turn is bounded by call count and wall clock, because this runs
//! unattended on the user's API key. What comes out is only a suggestion to
//! [`cabal_negotiation::Negotiation::propose`], which owns whether it may be sent.

use crate::agent_tools::{self, ToolContext, PROPOSE_OFFER};
use crate::llm_client::{LlmClient, LlmError, Message, ToolCall};
use cabal_negotiation::{Msg, Side};
use serde_json::Value;
use std::time::Duration;

pub const MAX_TOOL_CALLS: usize = 5;
const CALL_THE_TOOL: &str =
    "Make your move now by calling the propose_offer tool, with price_usd as one plain number.";
pub const TURN_TIMEOUT: Duration = Duration::from_secs(30);

/// One message in a negotiation, and who sent it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    pub by_us: bool,
    pub msg: Msg,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Propose {
        price_cents: u64,
    },
    /// The price is too old to act on. `note` goes to the intent's log, and nothing is sent.
    SuggestOnly {
        note: String,
    },
    /// The model answered, but with no usable price. Nothing is sent this round.
    Nothing,
    /// No model answered: unreachable, refused, too slow, out of budget, or none set. Unlike
    /// a model that misbehaved, this says nothing about the trade, so the caller can
    /// still use the built-in schedule.
    ModelUnavailable {
        market_cents: u64,
    },
}

/// Decides this side's move.
pub async fn decide(client: &LlmClient, ctx: &ToolContext, history: &[Said]) -> Decision {
    decide_within(client, ctx, history, TURN_TIMEOUT).await
}

async fn decide_within(
    client: &LlmClient,
    ctx: &ToolContext,
    history: &[Said],
    limit: Duration,
) -> Decision {
    // Checked before any model call: the answer cannot change it, and a call costs budget.
    let Some(quote) = ctx.feed.quote(&ctx.asset, ctx.now_ms).await else {
        return Decision::SuggestOnly {
            note: format!("NO MARKET PRICE FOR {}. AGENT WILL NOT OFFER.", ctx.asset),
        };
    };
    if quote.stale {
        return Decision::SuggestOnly {
            note: format!(
                "LAST KNOWN PRICE {} IS {} MIN OLD. AGENT WILL NOT OFFER.",
                cabal_core::UsdPrice::from_cents(quote.usd_cents),
                ctx.now_ms.saturating_sub(quote.updated_at_ms) / 60_000,
            ),
        };
    }

    let turn = async {
        if client.tools_supported() == Some(false) {
            return without_tools(client, ctx, history, quote.usd_cents).await;
        }
        with_tools(client, ctx, history, quote.usd_cents).await
    };
    // Too slow is not misbehaving: a local model reloading into a busy machine takes this
    // long while saying nothing about the trade. The state machine still checks the price.
    tokio::time::timeout(limit, turn).await.unwrap_or_else(|_| {
        tracing::warn!(target: "cabalmesh::agent", "the model ran out of time");
        Decision::ModelUnavailable {
            market_cents: quote.usd_cents,
        }
    })
}

async fn with_tools(
    client: &LlmClient,
    ctx: &ToolContext,
    history: &[Said],
    market_cents: u64,
) -> Decision {
    let tools = agent_tools::tool_specs();
    let mut messages = vec![
        Message::system(&briefing(ctx, history, market_cents)),
        Message::user(CALL_THE_TOOL),
    ];
    let mut calls = 0;
    loop {
        let reply = match client.chat(&messages, &tools, false).await {
            Ok(reply) => reply,
            // Many servers answer this way for a model with no tool support.
            Err(LlmError::Http { status: 400, .. }) => {
                return without_tools(client, ctx, history, market_cents).await;
            }
            Err(error) => return failed(&error, market_cents),
        };
        if reply.tool_calls.is_empty() {
            // A local model will often write the call out as text instead, and may not
            // manage valid JSON doing it, so ask again rather than parse what it wrote.
            calls += 1;
            if calls >= MAX_TOOL_CALLS {
                tracing::warn!(target: "cabalmesh::agent", calls, "the model kept answering with no tool call");
                return Decision::Nothing;
            }
            messages.push(reply);
            messages.push(Message::user(CALL_THE_TOOL));
            continue;
        }
        let requested = reply.tool_calls.clone();
        messages.push(reply);

        for call in &requested {
            calls += 1;
            if calls > MAX_TOOL_CALLS || !has_usable_arguments(call) {
                tracing::warn!(target: "cabalmesh::agent", calls, "the model called too many tools, or sent bad arguments");
                return Decision::Nothing;
            }
            let result = agent_tools::execute(call, ctx).await;
            if let Some(price_cents) = ctx.proposal() {
                return Decision::Propose { price_cents };
            }
            messages.push(Message::tool(&result.call_id, &result.content));
        }
        if calls >= MAX_TOOL_CALLS {
            tracing::warn!(target: "cabalmesh::agent", calls, "the model used all its tool calls");
            return Decision::Nothing;
        }
    }
}

/// For a model that cannot call tools: the app reads what the tools would have, and
/// puts it in the prompt.
async fn without_tools(
    client: &LlmClient,
    ctx: &ToolContext,
    history: &[Said],
    market_cents: u64,
) -> Decision {
    let system = format!(
        "{}\nYour tradable balance is {}.\nRespond ONLY with JSON in this exact format: {{\"price_usd\": <number>}}",
        briefing(ctx, history, market_cents),
        ctx.balance.as_deref().unwrap_or("unknown"),
    );
    match client.chat_json(&system, "Make your move now.").await {
        Ok(answer) => agent_tools::price_cents_of(&answer)
            .map_or(Decision::Nothing, |price_cents| Decision::Propose {
                price_cents,
            }),
        Err(error) => failed(&error, market_cents),
    }
}

/// Holds numbers and this device's own values only. The limit is not among them.
fn briefing(ctx: &ToolContext, history: &[Said], market_cents: u64) -> String {
    let (role, aim) = match ctx.bound.side {
        Side::Sell => ("SELLER", "a high price"),
        Side::Buy => ("BUYER", "a low price"),
    };
    let said = if history.is_empty() {
        "No offers yet. You open.".to_string()
    } else {
        history.iter().map(said_line).collect::<Vec<_>>().join("\n")
    };
    format!(
        "You negotiate a peer-to-peer trade for your owner. You are the {role} of {amount} {asset}, and you want {aim}.\n\
         This is round {round} of {max}. If no price is agreed by the end, there is no trade, so move toward their price each round.\n\
         Prices are US dollars per token. The market price is ${market:.2}. Stay near it.\n\
         To accept the other side's last price, propose exactly that price.\n\
         Your owner's limit is enforced for you: if a price is refused, try another.\n\
         Offers so far:\n{said}",
        amount = ctx.amount,
        asset = ctx.asset,
        round = ctx.round,
        max = cabal_negotiation::MAX_ROUNDS,
        market = agent_tools::cents_to_dollars(market_cents),
    )
}

fn said_line(said: &Said) -> String {
    let (Msg::Offer {
        round, price_cents, ..
    }
    | Msg::Counter {
        round, price_cents, ..
    }
    | Msg::Accept {
        round, price_cents, ..
    }) = &said.msg;
    let who = if said.by_us { "You" } else { "They" };
    format!(
        "Round {round}: {who} said ${:.2}",
        agent_tools::cents_to_dollars(*price_cents)
    )
}

fn failed(error: &LlmError, market_cents: u64) -> Decision {
    if model_misbehaved(error) {
        tracing::warn!(target: "cabalmesh::agent", "the model's answer could not be used");
        Decision::Nothing
    } else {
        Decision::ModelUnavailable { market_cents }
    }
}

const fn model_misbehaved(error: &LlmError) -> bool {
    match error {
        LlmError::BadResponse(_) => true,
        LlmError::Unreachable
        | LlmError::Http { .. }
        | LlmError::BudgetExhausted
        | LlmError::NoModel => false,
    }
}

/// Arguments that are not JSON, or a proposal with no usable price, end the turn: this
/// model is not going to produce a number worth sending.
fn has_usable_arguments(call: &ToolCall) -> bool {
    let Ok(arguments) = serde_json::from_str::<Value>(&call.arguments) else {
        return false;
    };
    call.name != PROPOSE_OFFER || agent_tools::price_cents_of(&arguments).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_tools::tests::context;
    use crate::agent_tools::GET_MARKET_PRICE;
    use crate::llm_budget::LlmBudget;
    use crate::llm_client::mock;
    use crate::llm_config::LlmSettings;
    use crate::price_feed::tests::{loaded_feed, NOON_MS};
    use crate::price_feed::{PriceFeed, STALE_AFTER_MS};
    use serde_json::json;
    use std::sync::Arc;

    fn tool_reply(name: &str, arguments: &str) -> (u16, String) {
        (
            200,
            json!({ "choices": [{ "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{ "id": "c1", "type": "function", "function": { "name": name, "arguments": arguments } }],
            } }] })
            .to_string(),
        )
    }

    async fn feed(dir: &tempfile::TempDir) -> Arc<PriceFeed> {
        Arc::new(loaded_feed(4.04, dir).await)
    }

    #[tokio::test]
    async fn returns_propose_after_price_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = context(Side::Sell, None, feed(&dir).await);
        let (url, server) = mock::serve(vec![
            tool_reply(GET_MARKET_PRICE, r#"{"asset":"wNEAR"}"#),
            tool_reply(PROPOSE_OFFER, r#"{"price_usd":4.10}"#),
        ]);

        let decision = decide(&mock::client(&url, None), &ctx, &[]).await;

        assert_eq!(decision, Decision::Propose { price_cents: 410 });
        let requests = server.join().unwrap();
        assert!(
            requests[0].contains("propose_offer"),
            "the tools are offered"
        );
        assert!(
            requests[1].contains("price_usd"),
            "the tool result goes back: {}",
            requests[1]
        );
        assert!(
            requests[1].contains(r#""tool_call_id":"c1""#),
            "{}",
            requests[1]
        );
    }

    #[tokio::test]
    async fn the_prompt_holds_the_history_but_never_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = context(Side::Buy, Some(39_017), feed(&dir).await);
        let history = [Said {
            by_us: false,
            msg: Msg::Offer {
                neg_id: "n".into(),
                round: 1,
                price_cents: 431,
            },
        }];
        let (url, server) = mock::serve(vec![tool_reply(PROPOSE_OFFER, r#"{"price_usd":4.10}"#)]);

        decide(&mock::client(&url, None), &ctx, &history).await;

        let request = &server.join().unwrap()[0];
        assert!(request.contains("4.31"), "{request}");
        assert!(
            !request.contains("390.17") && !request.contains("39017"),
            "{request}"
        );
    }

    #[tokio::test]
    async fn stops_after_five_tool_calls() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = context(Side::Sell, None, feed(&dir).await);
        let replies = vec![tool_reply(GET_MARKET_PRICE, r#"{"asset":"wNEAR"}"#); MAX_TOOL_CALLS];
        let (url, server) = mock::serve(replies);

        let decision = decide(&mock::client(&url, None), &ctx, &[]).await;

        assert_eq!(decision, Decision::Nothing);
        assert_eq!(
            server.join().unwrap().len(),
            MAX_TOOL_CALLS,
            "no sixth model call"
        );
    }

    #[tokio::test]
    async fn one_reply_with_many_calls_still_counts_each_one() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = context(Side::Sell, None, feed(&dir).await);
        let many: Vec<_> = (0..9)
            .map(|i| json!({ "id": format!("c{i}"), "type": "function", "function": { "name": GET_MARKET_PRICE, "arguments": "{}" } }))
            .chain([json!({ "id": "p", "type": "function", "function": { "name": PROPOSE_OFFER, "arguments": r#"{"price_usd":4.1}"# } })])
            .collect();
        let reply = json!({ "choices": [{ "message": { "role": "assistant", "content": null, "tool_calls": many } }] });
        let (url, server) = mock::serve(vec![(200, reply.to_string())]);

        let decision = decide(&mock::client(&url, None), &ctx, &[]).await;

        assert_eq!(
            decision,
            Decision::Nothing,
            "the proposal was call number ten"
        );
        assert_eq!(server.join().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_model_too_slow_for_the_turn_is_not_asked_again() {
        assert_eq!(TURN_TIMEOUT, Duration::from_secs(30));
        let dir = tempfile::tempdir().unwrap();
        let ctx = context(Side::Sell, None, feed(&dir).await);
        // Connections queue here, and nothing ever answers.
        let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", silent.local_addr().unwrap());

        let decision = decide_within(
            &mock::client(&url, None),
            &ctx,
            &[],
            Duration::from_millis(200),
        )
        .await;

        assert_eq!(
            decision,
            Decision::ModelUnavailable { market_cents: 404 },
            "too slow says nothing about the trade"
        );
    }

    #[tokio::test]
    async fn malformed_arguments_yield_nothing() {
        for arguments in [
            "not json",
            r#"{"price_usd":"cheap"}"#,
            r#"{"price_usd":-4}"#,
            "{}",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = context(Side::Sell, None, feed(&dir).await);
            let (url, server) = mock::serve(vec![tool_reply(PROPOSE_OFFER, arguments)]);

            let decision = decide(&mock::client(&url, None), &ctx, &[]).await;

            assert_eq!(decision, Decision::Nothing, "{arguments}");
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn a_price_outside_the_limit_gets_another_try() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = context(Side::Sell, Some(420), feed(&dir).await);
        let (url, server) = mock::serve(vec![
            tool_reply(PROPOSE_OFFER, r#"{"price_usd":4.05}"#),
            tool_reply(PROPOSE_OFFER, r#"{"price_usd":4.25}"#),
        ]);

        let decision = decide(&mock::client(&url, None), &ctx, &[]).await;

        assert_eq!(decision, Decision::Propose { price_cents: 425 });
        assert!(server.join().unwrap()[1].contains("Propose a higher one"));
    }

    /// A real llama3.1 went straight to `propose_offer` and guessed $1.50 five times.
    #[tokio::test]
    async fn the_model_is_told_the_market_price_up_front() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = context(Side::Sell, None, feed(&dir).await);
        let (url, server) = mock::serve(vec![tool_reply(PROPOSE_OFFER, r#"{"price_usd":4.10}"#)]);

        decide(&mock::client(&url, None), &ctx, &[]).await;

        let request = &server.join().unwrap()[0];
        assert!(request.contains("The market price is $4.04"), "{request}");
    }

    /// A real llama3.1 sent `{"name": "propose_offer", "parameters": {"price_usd": 3.94 + 0.1}}`
    /// as text, whenever the market price made it want to do sums.
    #[tokio::test]
    async fn a_call_written_as_text_gets_another_try() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = context(Side::Sell, None, feed(&dir).await);
        let (url, server) = mock::serve(vec![
            mock::reply(r#"{"name": "propose_offer", "parameters": {"price_usd": 3.94 + 0.1}}"#),
            tool_reply(PROPOSE_OFFER, r#"{"price_usd":4.04}"#),
        ]);

        let decision = decide(&mock::client(&url, None), &ctx, &[]).await;

        assert_eq!(decision, Decision::Propose { price_cents: 404 });
        assert!(server.join().unwrap()[1].contains("plain number"));
    }

    #[tokio::test]
    async fn a_model_that_only_writes_text_stops_after_five_tries() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = context(Side::Sell, None, feed(&dir).await);
        let (url, calls) = mock::serve_forever(mock::reply("I would rather chat.").1);

        assert_eq!(
            decide(&mock::client(&url, None), &ctx, &[]).await,
            Decision::Nothing
        );
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            MAX_TOOL_CALLS
        );
    }

    #[tokio::test]
    async fn stale_price_yields_suggest_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = context(Side::Sell, None, feed(&dir).await);
        ctx.now_ms = NOON_MS + STALE_AFTER_MS + 1;
        let refused = mock::refused_url();

        let Decision::SuggestOnly { note } = decide(&mock::client(&refused, None), &ctx, &[]).await
        else {
            panic!("a stale price may not be acted on");
        };
        assert!(note.contains("$4.04"), "{note}");
    }

    #[tokio::test]
    async fn no_known_price_yields_suggest_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = context(Side::Sell, None, feed(&dir).await);
        ctx.asset = "DOGE".into();
        let refused = mock::refused_url();

        assert!(matches!(
            decide(&mock::client(&refused, None), &ctx, &[]).await,
            Decision::SuggestOnly { .. }
        ));
    }

    fn no_tools_client(url: &str) -> LlmClient {
        LlmClient::new(
            LlmSettings {
                base_url: url.to_string(),
                tools_supported: Some(false),
                ..LlmSettings::default()
            },
            None,
            reqwest::Client::new(),
            Arc::new(LlmBudget::new(100)),
        )
    }

    #[tokio::test]
    async fn fallback_path_used_when_tools_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = context(Side::Sell, None, feed(&dir).await);
        let (url, server) = mock::serve(vec![mock::reply(r#"{"price_usd": 4.1}"#)]);

        let decision = decide(&no_tools_client(&url), &ctx, &[]).await;

        assert_eq!(decision, Decision::Propose { price_cents: 410 });
        let request = &server.join().unwrap()[0];
        assert!(!request.contains(r#""tools""#), "{request}");
        assert!(
            request.contains("4.04"),
            "the market price is put in the prompt: {request}"
        );
    }

    #[tokio::test]
    async fn a_server_that_refuses_tools_gets_the_fallback_path() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = context(Side::Sell, None, feed(&dir).await);
        let refusal = (
            400,
            r#"{"error":"model does not support tools"}"#.to_string(),
        );
        let (url, server) = mock::serve(vec![
            refusal.clone(),
            refusal,
            mock::reply(r#"{"price_usd": 4.1}"#),
        ]);

        let decision = decide(&mock::client(&url, None), &ctx, &[]).await;

        assert_eq!(decision, Decision::Propose { price_cents: 410 });
        server.join().unwrap();
    }

    #[tokio::test]
    async fn bad_json_on_the_fallback_path_yields_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = context(Side::Sell, None, feed(&dir).await);
        let (url, server) = mock::serve(vec![mock::reply("about four dollars")]);

        assert_eq!(
            decide(&no_tools_client(&url), &ctx, &[]).await,
            Decision::Nothing
        );
        server.join().unwrap();
    }

    #[tokio::test]
    async fn an_unreachable_model_is_reported_with_the_market_price() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = context(Side::Sell, None, feed(&dir).await);
        let refused = mock::refused_url();

        assert_eq!(
            decide(&mock::client(&refused, None), &ctx, &[]).await,
            Decision::ModelUnavailable { market_cents: 404 }
        );
    }

    #[tokio::test]
    async fn a_used_up_budget_is_reported_and_sends_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = context(Side::Sell, None, feed(&dir).await);
        let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        silent.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1", silent.local_addr().unwrap());

        assert_eq!(
            decide(&mock::client_with_budget(&url, None, 0), &ctx, &[]).await,
            Decision::ModelUnavailable { market_cents: 404 }
        );
        assert!(silent.accept().is_err(), "no request should have been made");
    }

    #[test]
    fn every_llm_error_has_a_home() {
        // A new variant must choose: did the model misbehave, or was it never asked?
        for (error, usable) in [
            (LlmError::Unreachable, false),
            (LlmError::BudgetExhausted, false),
            (LlmError::NoModel, false),
            (
                LlmError::Http {
                    status: 500,
                    body: String::new(),
                },
                false,
            ),
            (LlmError::BadResponse(String::new()), true),
        ] {
            assert_eq!(model_misbehaved(&error), usable);
        }
    }
}
