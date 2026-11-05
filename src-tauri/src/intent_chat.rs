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
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Serialize)]
struct OllamaRequest {
    model: &'static str,
    prompt: String,
    stream: bool,
    system: String,
    format: &'static str,
    options: OllamaOptions,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    temperature: u8,
    num_predict: u16,
}

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    response: String,
}

/// Failures that can occur before an LLM proposal reaches the ordinary intent
/// validation boundary.
#[derive(Debug, thiserror::Error)]
pub enum IntentChatError {
    #[error("intent model request failed")]
    Request(#[from] reqwest::Error),
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

/// Turns free text into intent fields via the configured Ollama endpoint.
pub struct IntentChatParser {
    client: Client,
    ollama_url: Option<String>,
}

impl IntentChatParser {
    #[must_use]
    pub fn new(ollama_url: Option<String>) -> Self {
        Self {
            client: Client::new(),
            ollama_url,
        }
    }

    /// Resolved per request, matching `SharkAgent::url` — a URL set at
    /// runtime (the only way to reach a model on iOS, which has no local
    /// process) applies without a restart.
    fn url(&self) -> String {
        self.ollama_url
            .clone()
            .unwrap_or_else(crate::ollama_config::url)
    }

    /// Asks the model to turn `text` into intent fields, telling it exactly
    /// which values `options` makes legal.
    ///
    /// # Errors
    ///
    /// The request to Ollama failed (network, timeout, non-2xx), or the model
    /// did not return a JSON object. Invalid model output is reported instead
    /// of being silently converted into seven empty form fields; an LLM
    /// transport/format failure is not the same thing as missing user input.
    pub async fn parse(
        &self,
        text: &str,
        options: &FormOptions,
    ) -> Result<IntentFields, IntentChatError> {
        let request = OllamaRequest {
            model: crate::ollama_config::INTENT_MODEL,
            prompt: format!("Convert this request:\n\n{text}"),
            stream: false,
            system: system_prompt(options),
            format: "json",
            options: OllamaOptions {
                temperature: 0,
                num_predict: 256,
            },
        };

        let response = self
            .client
            .post(format!("{}/api/generate", self.url()))
            .timeout(REQUEST_TIMEOUT)
            .json(&request)
            .send()
            .await?
            .error_for_status()?;
        let ollama_response: OllamaResponse = response.json().await?;

        let parsed: ParsedFields = serde_json::from_str(crate::llm_json::extract_json_object(
            &ollama_response.response,
        ))?;

        Ok(parsed.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{AssetOption, ModeOption};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

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

    /// Starts one tiny HTTP/1.1 server and returns the complete request after
    /// responding. Keeping this local makes the parser tests independent of a
    /// developer's Ollama process and installed models.
    fn mock_ollama(body: String) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();

            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let read = stream.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if request_is_complete(&request) {
                    break;
                }
            }

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8(request).unwrap()
        });
        (format!("http://{address}"), handle)
    }

    fn request_is_complete(request: &[u8]) -> bool {
        let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
            return false;
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        request.len() >= header_end + 4 + content_length
    }

    #[tokio::test]
    async fn request_uses_llama31_and_ollama_json_mode() {
        let model_output = serde_json::json!({
            "action": "BUY",
            "asset": "wNEAR",
            "condition": "Price under",
            "price": "95",
            "amount": "10",
            "mode": "SHARK MODE",
            "privacy": "HIGH"
        })
        .to_string();
        let response = serde_json::json!({ "response": model_output }).to_string();
        let (url, request) = mock_ollama(response);

        let fields = IntentChatParser::new(Some(url))
            .parse("buy 10 wnear under 95", &options())
            .await
            .unwrap();
        let request = request.join().unwrap();

        assert_eq!(fields.action, "BUY");
        assert_eq!(fields.amount, "10");
        assert!(request.contains(r#""model":"llama3.1:8b""#));
        assert!(request.contains(r#""format":"json""#));
        assert!(request.contains(r#""temperature":0"#));
    }

    #[tokio::test]
    async fn malformed_model_output_is_rejected() {
        let response = serde_json::json!({ "response": "not JSON" }).to_string();
        let (url, request) = mock_ollama(response);

        let result = IntentChatParser::new(Some(url))
            .parse("buy wnear", &options())
            .await;
        request.join().unwrap();

        assert!(matches!(result, Err(IntentChatError::InvalidResponse(_))));
    }

    #[tokio::test]
    async fn unreachable_model_is_reported_as_a_request_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);

        let result = IntentChatParser::new(Some(format!("http://{address}")))
            .parse("buy wnear", &options())
            .await;

        assert!(matches!(result, Err(IntentChatError::Request(_))));
    }
}
