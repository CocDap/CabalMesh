//! Parses free text into `IntentFields` via the configured LLM.
//!
//! # The safety property this preserves
//!
//! This module produces exactly the same [`IntentFields`] shape the
//! hand-filled form already sends to `parse_draft` — never a validated
//! `IntentDraft`, never anything closer to broadcast than that. Whatever the
//! model returns is exactly as trusted as what a user typed into a raw text
//! box, no more: `preview_intent` and `broadcast_intent` validate it
//! identically either way, and neither this module nor the command wrapping
//! it can skip that step. See `docs/intent-chat-and-modules-design.md`.

use crate::commands::{FormOptions, IntentFields};
use crate::llm_client::{LlmClient, LlmError};
use serde::Deserialize;

/// Failures that can occur before an LLM proposal reaches the ordinary intent
/// validation boundary.
#[derive(Debug, thiserror::Error)]
pub enum IntentChatError {
    #[error("intent model request failed")]
    Llm(#[from] LlmError),
    #[error("intent model returned invalid structured output")]
    InvalidResponse(#[from] serde_json::Error),
}

/// The model's answer, every field optional on the wire — free text
/// reasonably leaves fields unmentioned ("buy 10 wnear" says nothing about
/// privacy), and a blank here becomes `parse_draft`'s ordinary missing-field
/// rejection later, the same path an unfilled form field already takes.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ParsedFields {
    action: String,
    asset: String,
    condition: String,
    price: String,
    amount: String,
    mode: String,
    privacy: String,
}

impl From<ParsedFields> for IntentFields {
    fn from(parsed: ParsedFields) -> Self {
        Self {
            action: parsed.action,
            asset: parsed.asset,
            condition: parsed.condition,
            price: parsed.price,
            amount: parsed.amount,
            mode: parsed.mode,
            privacy: parsed.privacy,
        }
    }
}

/// Builds the instruction that tells the model exactly which strings are
/// legal for each field — the same vocabulary the segmented controls offer
/// (`intent_form_options`), embedded here rather than duplicated, so a
/// second hardcoded list can never drift from what the form actually
/// accepts.
fn system_prompt(options: &FormOptions) -> String {
    let actions = options.actions.join(", ");
    let assets = options
        .assets
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let conditions = options
        .conditions
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let modes = options
        .modes
        .iter()
        .map(|m| format!("\"{}\"", m.label))
        .collect::<Vec<_>>()
        .join(", ");
    let privacy = options.privacy_levels.join(", ");

    format!(
        r#"You convert a trading request written in plain English into structured JSON fields for the CabalMesh intent form. Extract only what the text actually states, and leave a field as an empty string "" when it is not mentioned. Never invent a number, a price, or a choice the text does not support.

Allowed values (copy one exactly, case-sensitive where quoted):
- action: one of {actions}
- asset: one of {assets}
- condition: one of {conditions}
- price: a plain number in US dollars, only when condition is not the "any price" option; otherwise ""
- amount: a plain number, no currency symbol or asset name
- mode: one of {modes}
- privacy: one of {privacy}

Respond with ONLY this JSON object, nothing else, no explanation:
{{"action": "", "asset": "", "condition": "", "price": "", "amount": "", "mode": "", "privacy": ""}}"#
    )
}

/// Asks the model to turn `text` into intent fields, telling it exactly which
/// values `options` makes legal.
///
/// # Errors
///
/// The model call failed, or its answer does not have the fields' shape. Invalid
/// model output is reported instead of being silently converted into seven empty
/// form fields; a model failure is not the same thing as missing user input.
pub async fn parse(client: &LlmClient, text: &str, options: &FormOptions) -> Result<IntentFields, IntentChatError> {
    let answer = client.chat_json(&system_prompt(options), &format!("Convert this request:\n\n{text}")).await?;
    let parsed: ParsedFields = serde_json::from_value(answer)?;
    Ok(parsed.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{AssetOption, ModeOption};
    use crate::llm_client::mock;

    fn options() -> FormOptions {
        FormOptions {
            actions: vec!["BUY".into(), "SELL".into()],
            assets: vec![AssetOption {
                name: "wNEAR".into(),
                tag: "NEA".into(),
                decimals: 24,
                available: None,
            }],
            quote: AssetOption {
                name: "USDC".into(),
                tag: "USD".into(),
                decimals: 6,
                available: None,
            },
            conditions: vec!["Price under".into(), "Any price".into()],
            modes: vec![ModeOption {
                label: "SHARK MODE".into(),
                description: String::new(),
            }],
            privacy_levels: vec!["HIGH".into()],
        }
    }

    #[test]
    fn the_prompt_names_every_allowed_value_so_none_is_invented() {
        let prompt = system_prompt(&options());
        for expected in [
            "BUY",
            "SELL",
            "wNEAR",
            "Price under",
            "Any price",
            "SHARK MODE",
            "HIGH",
        ] {
            assert!(
                prompt.contains(expected),
                "prompt is missing {expected:?}: {prompt}"
            );
        }
    }

    #[test]
    fn a_well_formed_response_becomes_intent_fields() {
        let raw = r#"{"action": "BUY", "asset": "wNEAR", "condition": "Price under", "price": "95", "amount": "10", "mode": "SHARK MODE", "privacy": "HIGH"}"#;
        let parsed: ParsedFields = serde_json::from_str(raw).unwrap();
        let fields: IntentFields = parsed.into();
        assert_eq!(fields.action, "BUY");
        assert_eq!(fields.amount, "10");
        assert_eq!(fields.mode, "SHARK MODE");
    }

    #[test]
    fn a_response_wrapped_in_prose_still_parses() {
        // llama2's own habit, per llm_json.rs — reused here rather than
        // re-solved.
        let raw = "Sure! Here you go:\n{\"action\": \"BUY\", \"asset\": \"wNEAR\", \"condition\": \"\", \"price\": \"\", \"amount\": \"10\", \"mode\": \"\", \"privacy\": \"\"}\nLet me know if you need anything else.";
        let parsed: ParsedFields =
            serde_json::from_str(crate::llm_json::extract_json_object(raw)).unwrap();
        assert_eq!(parsed.action, "BUY");
        assert_eq!(parsed.amount, "10");
    }

    #[test]
    fn a_field_the_text_never_mentioned_is_left_blank_not_invented() {
        let raw = r#"{"action": "BUY", "amount": "10"}"#;
        let parsed: ParsedFields = serde_json::from_str(raw).unwrap();
        assert_eq!(
            parsed.privacy, "",
            "an unmentioned field must stay blank, not default to a guess"
        );
        assert_eq!(parsed.condition, "");
    }

    #[test]
    fn garbage_that_is_not_json_is_an_explicit_model_error() {
        let raw = "I'm not sure what you mean by that.";
        let parsed =
            serde_json::from_str::<ParsedFields>(crate::llm_json::extract_json_object(raw));
        assert!(parsed.is_err());
    }

    fn all_fields() -> String {
        serde_json::json!({
            "action": "BUY",
            "asset": "wNEAR",
            "condition": "Price under",
            "price": "95",
            "amount": "10",
            "mode": "SHARK MODE",
            "privacy": "HIGH"
        })
        .to_string()
    }

    #[tokio::test]
    async fn request_uses_the_configured_model_in_json_mode() {
        let (url, server) = mock::serve(vec![mock::reply(&all_fields())]);

        let fields = parse(&mock::client(&url, None), "buy 10 wnear under 95", &options()).await.unwrap();
        let request = server.join().unwrap().remove(0);

        assert_eq!(fields.action, "BUY");
        assert_eq!(fields.amount, "10");
        assert!(request.starts_with("POST /v1/chat/completions "), "{request}");
        assert!(request.contains(r#""model":"llama3.1:8b""#), "{request}");
        assert!(request.contains(r#""response_format":{"type":"json_object"}"#), "{request}");
        assert!(request.contains(r#""temperature":0"#), "{request}");
        assert!(request.contains("buy 10 wnear under 95"), "{request}");
    }

    #[tokio::test]
    async fn malformed_model_output_is_rejected() {
        let (url, server) = mock::serve(vec![mock::reply("not JSON")]);
        let result = parse(&mock::client(&url, None), "buy wnear", &options()).await;
        server.join().unwrap();
        assert!(matches!(result, Err(IntentChatError::Llm(LlmError::BadResponse(_)))));
    }

    #[tokio::test]
    async fn json_of_the_wrong_shape_is_rejected() {
        let (url, server) = mock::serve(vec![mock::reply(r#"{"action": 5}"#)]);
        let result = parse(&mock::client(&url, None), "buy wnear", &options()).await;
        server.join().unwrap();
        assert!(matches!(result, Err(IntentChatError::InvalidResponse(_))));
    }

    #[tokio::test]
    async fn unreachable_model_is_reported_as_a_request_error() {
        let result = parse(&mock::client(&mock::refused_url(), None), "buy wnear", &options()).await;
        assert!(matches!(result, Err(IntentChatError::Llm(LlmError::Unreachable))));
    }
}
