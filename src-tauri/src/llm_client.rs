//! One client for every model call: OpenAI-compatible `/chat/completions`.
//!
//! The key is sent only as a bearer header, and only when one is saved. Nothing the
//! model says is trusted beyond text: callers validate what comes back exactly as
//! they validate what a user typed.

use crate::llm_budget::LlmBudget;
use crate::llm_config::LlmSettings;
use cabal_vault::Secret;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);
/// Bounds what one unattended call can cost. High enough for models that spend
/// tokens reasoning before they answer.
const MAX_TOKENS: u32 = 1024;
/// A provider's error body is shown and logged, so it is kept short.
const ERROR_BODY_LIMIT: usize = 300;
const PROBE_TOOL: &str = "echo_number";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A function call the model asked for. `arguments` is JSON text from the model,
/// so whoever runs the tool must validate it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_call_id: Option<String>,
}

impl Message {
    #[must_use]
    pub fn system(content: &str) -> Self {
        Self { role: Role::System, content: Some(content.to_string()), tool_calls: Vec::new(), tool_call_id: None }
    }

    #[must_use]
    pub fn user(content: &str) -> Self {
        Self { role: Role::User, content: Some(content.to_string()), tool_calls: Vec::new(), tool_call_id: None }
    }

    fn to_wire(&self) -> Value {
        let mut wire = json!({ "role": self.role, "content": self.content });
        if !self.tool_calls.is_empty() {
            wire["tool_calls"] = self
                .tool_calls
                .iter()
                .map(|call| {
                    json!({
                        "id": call.id,
                        "type": "function",
                        "function": { "name": call.name, "arguments": call.arguments },
                    })
                })
                .collect();
        }
        if let Some(id) = &self.tool_call_id {
            wire["tool_call_id"] = json!(id);
        }
        wire
    }
}

/// A function offered to the model.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
}

impl ToolSpec {
    fn to_wire(&self) -> Value {
        json!({
            "type": "function",
            "function": { "name": self.name, "description": self.description, "parameters": self.parameters },
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("the model endpoint could not be reached")]
    Unreachable,
    #[error("the model endpoint answered {status}: {body}")]
    Http { status: u16, body: String },
    #[error("the model's answer could not be used: {0}")]
    BadResponse(String),
    #[error("the hourly model call budget is used up")]
    BudgetExhausted,
    #[error("no model is set")]
    NoModel,
}

/// What a connection test found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct LlmProbe {
    pub reachable: bool,
    pub json_ok: bool,
    pub tools_ok: bool,
    #[cfg_attr(feature = "ts-rs", ts(type = "number"))]
    pub latency_ms: u64,
    pub error: Option<String>,
}

/// Built per use, so a change in Profile applies to the next call without a restart.
#[derive(Debug)]
pub struct LlmClient {
    settings: LlmSettings,
    key: Option<Secret>,
    http: reqwest::Client,
    budget: Arc<LlmBudget>,
}

impl LlmClient {
    #[must_use]
    pub fn new(settings: LlmSettings, key: Option<Secret>, http: reqwest::Client, budget: Arc<LlmBudget>) -> Self {
        Self { settings, key, http, budget }
    }

    /// One model turn.
    ///
    /// With `json`, asks for JSON mode. Some providers refuse that or the other
    /// optional fields with a 400, so it retries once without them.
    ///
    /// # Errors
    ///
    /// See [`LlmError`]. A refused budget sends nothing.
    pub async fn chat(&self, messages: &[Message], tools: &[ToolSpec], json: bool) -> Result<Message, LlmError> {
        self.require_model()?;
        if !self.budget.try_acquire(Instant::now()) {
            return Err(LlmError::BudgetExhausted);
        }
        self.request(messages, tools, json, REQUEST_TIMEOUT).await
    }

    /// One system and user message, answered as a JSON value. Recovers JSON wrapped in
    /// prose or code fences, which some models produce even in JSON mode.
    ///
    /// # Errors
    ///
    /// See [`LlmError`]. An answer with no JSON in it is [`LlmError::BadResponse`].
    pub async fn chat_json(&self, system: &str, user: &str) -> Result<Value, LlmError> {
        let reply = self.chat(&[Message::system(system), Message::user(user)], &[], true).await?;
        parse_json(&reply)
    }

    /// Tests the endpoint: one tiny JSON request, then one offering a tool the model
    /// is told to call.
    ///
    /// Takes no budget. It runs only when the user presses the button, and a test
    /// that fails because the budget is spent would say nothing about the endpoint.
    pub async fn probe(&self) -> LlmProbe {
        let mut probe = LlmProbe { reachable: false, json_ok: false, tools_ok: false, latency_ms: 0, error: None };
        if let Err(error) = self.require_model() {
            probe.error = Some(error.to_string());
            return probe;
        }

        let messages = [
            Message::system("Respond ONLY with a JSON object."),
            Message::user(r#"Return exactly {"ok":true}"#),
        ];
        let started = Instant::now();
        let answer = self.request(&messages, &[], true, PROBE_TIMEOUT).await;
        probe.latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        match answer {
            Ok(reply) => {
                probe.reachable = true;
                probe.json_ok = parse_json(&reply).is_ok();
            }
            Err(error) => {
                probe.reachable = !matches!(error, LlmError::Unreachable);
                probe.error = Some(error.to_string());
                return probe;
            }
        }

        let tool = ToolSpec {
            name: PROBE_TOOL,
            description: "Echoes a number back.",
            parameters: json!({
                "type": "object",
                "properties": { "n": { "type": "integer" } },
                "required": ["n"],
            }),
        };
        let messages = [Message::user("Call the echo_number tool with n set to 7.")];
        // A failure here is the answer, not an error: many servers return 400 for a
        // model with no tool support.
        if let Ok(reply) = self.request(&messages, &[tool], false, PROBE_TIMEOUT).await {
            probe.tools_ok = reply.tool_calls.iter().any(called_probe_tool);
        }
        probe
    }

    fn require_model(&self) -> Result<(), LlmError> {
        if self.settings.model.trim().is_empty() {
            return Err(LlmError::NoModel);
        }
        Ok(())
    }

    async fn request(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
        json: bool,
        timeout: Duration,
    ) -> Result<Message, LlmError> {
        let mut body = json!({
            "model": self.settings.model,
            "messages": messages.iter().map(Message::to_wire).collect::<Vec<_>>(),
            "stream": false,
        });
        if !tools.is_empty() {
            body["tools"] = tools.iter().map(ToolSpec::to_wire).collect();
        }
        let mut optional = body.clone();
        optional["temperature"] = json!(0);
        optional["max_tokens"] = json!(MAX_TOKENS);
        if json {
            optional["response_format"] = json!({ "type": "json_object" });
        }

        match self.post(&optional, timeout).await {
            Err(LlmError::Http { status: 400, .. }) => self.post(&body, timeout).await,
            other => other,
        }
    }

    async fn post(&self, body: &Value, timeout: Duration) -> Result<Message, LlmError> {
        let mut request =
            self.http.post(format!("{}/chat/completions", self.settings.base_url)).timeout(timeout).json(body);
        if let Some(key) = &self.key {
            request = request.bearer_auth(key.expose());
        }
        let response = request.send().await.map_err(|error| {
            // Logged here because the variant carries nothing: reqwest's text names the URL.
            tracing::debug!(target: "cabalmesh::llm", %error, "model request failed");
            LlmError::Unreachable
        })?;

        let status = response.status();
        let text = response.text().await.map_err(|_| LlmError::Unreachable)?;
        if !status.is_success() {
            return Err(LlmError::Http { status: status.as_u16(), body: self.scrubbed(&text) });
        }
        parse_reply(&text)
    }

    /// Some providers quote the key they refused. This text is logged and shown.
    fn scrubbed(&self, body: &str) -> String {
        let body = match &self.key {
            Some(key) if !key.is_empty() => body.replace(key.expose(), "<redacted>"),
            _ => body.to_string(),
        };
        body.chars().take(ERROR_BODY_LIMIT).collect()
    }
}

fn called_probe_tool(call: &ToolCall) -> bool {
    call.name == PROBE_TOOL
        && serde_json::from_str::<Value>(&call.arguments).is_ok_and(|arguments| arguments["n"] == 7)
}

fn parse_json(reply: &Message) -> Result<Value, LlmError> {
    let text = reply.content.as_deref().unwrap_or_default();
    serde_json::from_str(crate::llm_json::extract_json_object(text))
        .map_err(|_| LlmError::BadResponse("the answer held no JSON".into()))
}

fn parse_reply(text: &str) -> Result<Message, LlmError> {
    let bad = |why: &str| LlmError::BadResponse(why.to_string());
    let reply: Value = serde_json::from_str(text).map_err(|_| bad("the body was not JSON"))?;
    let message = reply.pointer("/choices/0/message").ok_or_else(|| bad("the body had no message"))?;

    let tool_calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|calls| calls.iter().filter_map(parse_tool_call).collect())
        .unwrap_or_default();

    Ok(Message {
        role: Role::Assistant,
        content: message.get("content").and_then(Value::as_str).map(str::to_string),
        tool_calls,
        tool_call_id: None,
    })
}

fn parse_tool_call(call: &Value) -> Option<ToolCall> {
    let function = call.get("function")?;
    let arguments = function.get("arguments")?;
    Some(ToolCall {
        // Some servers omit the id. A reply still has to name the call.
        id: call.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
        name: function.get("name")?.as_str()?.to_string(),
        // The spec says a JSON string. Some servers send the object itself.
        arguments: arguments.as_str().map_or_else(|| arguments.to_string(), str::to_string),
    })
}

/// A mock endpoint for tests elsewhere in the crate that need a real HTTP round trip.
#[cfg(test)]
pub(crate) mod mock {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    /// A chat completion whose message content is `content`.
    pub fn reply(content: &str) -> (u16, String) {
        (200, json!({ "choices": [{ "message": { "role": "assistant", "content": content } }] }).to_string())
    }

    /// Answers one request per entry in order, and returns each raw request.
    pub fn serve(responses: Vec<(u16, String)>) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            responses
                .into_iter()
                .map(|(status, body)| {
                    let (mut stream, _) = listener.accept().unwrap();
                    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    let mut request = Vec::new();
                    let mut buffer = [0_u8; 4096];
                    while !is_complete(&request) {
                        let read = stream.read(&mut buffer).unwrap();
                        if read == 0 {
                            break;
                        }
                        request.extend_from_slice(&buffer[..read]);
                    }
                    let response = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len(),
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                    String::from_utf8(request).unwrap()
                })
                .collect()
        });
        (format!("http://{address}/v1"), handle)
    }

    fn is_complete(request: &[u8]) -> bool {
        let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
            return false;
        };
        let headers = String::from_utf8_lossy(&request[..end]);
        let length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().ok()).flatten()
            })
            .unwrap_or(0);
        request.len() >= end + 4 + length
    }

    /// A client for `url` with `key`, plenty of budget, and the default model.
    pub fn client(url: &str, key: Option<&str>) -> LlmClient {
        client_with_budget(url, key, 100)
    }

    pub fn client_with_budget(url: &str, key: Option<&str>, per_hour: u32) -> LlmClient {
        LlmClient::new(
            LlmSettings { base_url: url.to_string(), ..LlmSettings::default() },
            key.map(Secret::from),
            reqwest::Client::new(),
            Arc::new(LlmBudget::new(per_hour)),
        )
    }

    /// An address nothing listens on, so a connection is refused at once.
    pub fn refused_url() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        format!("http://{address}/v1")
    }
}

#[cfg(test)]
mod tests {
    use super::mock::{client, client_with_budget, refused_url, reply, serve};
    use super::*;

    fn body_of(request: &str) -> Value {
        serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap()
    }

    #[tokio::test]
    async fn posts_to_chat_completions_with_bearer() {
        let (url, server) = serve(vec![reply(r#"{"a":1}"#)]);
        let value = client(&url, Some("k")).chat_json("s", "u").await.unwrap();
        let requests = server.join().unwrap();
        assert_eq!(value["a"], 1);
        assert!(requests[0].starts_with("POST /v1/chat/completions "), "{}", requests[0]);
        assert!(requests[0].to_lowercase().contains("authorization: bearer k\r\n"), "{}", requests[0]);

        let body = body_of(&requests[0]);
        assert_eq!(body["model"], "llama3.1:8b");
        assert_eq!(body["messages"][0], json!({ "role": "system", "content": "s" }));
        assert_eq!(body["messages"][1], json!({ "role": "user", "content": "u" }));
        assert_eq!(body["response_format"]["type"], "json_object");
        assert_eq!(body["stream"], false);
    }

    #[tokio::test]
    async fn no_key_sends_no_authorization_header() {
        let (url, server) = serve(vec![reply("{}")]);
        client(&url, None).chat_json("s", "u").await.unwrap();
        assert!(!server.join().unwrap()[0].to_lowercase().contains("authorization"));
    }

    #[tokio::test]
    async fn json_mode_400_retries_without_response_format() {
        let (url, server) = serve(vec![(400, r#"{"error":"nope"}"#.into()), reply("Sure:\n{\"a\":2}\nDone.")]);
        let value = client(&url, None).chat_json("s", "u").await.unwrap();
        let requests = server.join().unwrap();
        assert_eq!(value["a"], 2);
        assert!(body_of(&requests[0]).get("response_format").is_some());
        let retry = body_of(&requests[1]);
        for field in ["response_format", "temperature", "max_tokens"] {
            assert!(retry.get(field).is_none(), "{field} should be gone from the retry");
        }
    }

    #[tokio::test]
    async fn a_second_400_is_reported() {
        let (url, server) = serve(vec![(400, "a".into()), (400, "b".into())]);
        let result = client(&url, None).chat_json("s", "u").await;
        server.join().unwrap();
        assert!(matches!(result, Err(LlmError::Http { status: 400, body }) if body == "b"));
    }

    #[tokio::test]
    async fn an_auth_failure_is_not_retried() {
        let (url, server) = serve(vec![(401, "bad key".into())]);
        let result = client(&url, None).chat_json("s", "u").await;
        assert_eq!(server.join().unwrap().len(), 1);
        assert!(matches!(result, Err(LlmError::Http { status: 401, .. })));
    }

    #[tokio::test]
    async fn fenced_json_is_recovered() {
        let (url, server) = serve(vec![reply("```json\n{\"price\": 4.05}\n```")]);
        let value = client(&url, None).chat_json("s", "u").await.unwrap();
        server.join().unwrap();
        assert_eq!(value["price"], 4.05);
    }

    #[tokio::test]
    async fn an_answer_with_no_json_is_a_bad_response() {
        let (url, server) = serve(vec![reply("I cannot help with that.")]);
        let result = client(&url, None).chat_json("s", "u").await;
        server.join().unwrap();
        assert!(matches!(result, Err(LlmError::BadResponse(_))));
    }

    #[tokio::test]
    async fn tool_calls_are_parsed() {
        let body = json!({ "choices": [{ "message": { "role": "assistant", "content": null, "tool_calls": [
            { "id": "c1", "type": "function", "function": { "name": "get_price", "arguments": "{\"asset\":\"wNEAR\"}" } },
            { "id": "c2", "type": "function", "function": { "name": "obj", "arguments": { "n": 7 } } },
        ] } }] });
        let (url, server) = serve(vec![(200, body.to_string())]);
        let tool = ToolSpec {
            name: "get_price",
            description: "d",
            parameters: json!({ "type": "object", "properties": {} }),
        };
        let reply = client(&url, None).chat(&[Message::user("u")], &[tool], false).await.unwrap();
        let sent = body_of(&server.join().unwrap()[0]);

        assert_eq!(sent["tools"][0]["function"]["name"], "get_price");
        assert!(sent.get("response_format").is_none());
        assert_eq!(reply.content, None);
        assert_eq!(reply.tool_calls[0].id, "c1");
        assert_eq!(reply.tool_calls[0].name, "get_price");
        assert_eq!(reply.tool_calls[0].arguments, r#"{"asset":"wNEAR"}"#);
        assert_eq!(reply.tool_calls[1].arguments, r#"{"n":7}"#);
    }

    #[tokio::test]
    async fn a_tool_result_goes_back_with_its_call_id() {
        let (url, server) = serve(vec![reply("done")]);
        let history = [
            Message {
                role: Role::Assistant,
                content: None,
                tool_calls: vec![ToolCall { id: "c1".into(), name: "t".into(), arguments: "{}".into() }],
                tool_call_id: None,
            },
            Message { role: Role::Tool, content: Some("7".into()), tool_calls: vec![], tool_call_id: Some("c1".into()) },
        ];
        client(&url, None).chat(&history, &[], false).await.unwrap();
        let sent = body_of(&server.join().unwrap()[0]);
        assert_eq!(sent["messages"][0]["tool_calls"][0]["function"]["name"], "t");
        assert_eq!(sent["messages"][1]["role"], "tool");
        assert_eq!(sent["messages"][1]["tool_call_id"], "c1");
    }

    #[tokio::test]
    async fn a_refused_connection_is_unreachable() {
        let result = client(&refused_url(), None).chat_json("s", "u").await;
        assert!(matches!(result, Err(LlmError::Unreachable)));
    }

    #[tokio::test]
    async fn an_error_that_quotes_the_key_is_scrubbed() {
        let (url, server) = serve(vec![(401, "bad key sk-very-secret".into())]);
        let error = client(&url, Some("sk-very-secret")).chat_json("s", "u").await.unwrap_err();
        server.join().unwrap();
        assert!(!format!("{error} {error:?}").contains("sk-very-secret"));
    }

    #[test]
    fn the_client_debug_has_no_key() {
        let printed = format!("{:?}", client("http://x/v1", Some("sk-very-secret")));
        assert!(!printed.contains("sk-very-secret"), "{printed}");
    }

    fn tool_reply(name: &str, arguments: &str) -> (u16, String) {
        let call = json!({ "id": "c", "type": "function", "function": { "name": name, "arguments": arguments } });
        (200, json!({ "choices": [{ "message": { "content": null, "tool_calls": [call] } }] }).to_string())
    }

    #[tokio::test]
    async fn probe_reports_tools_ok_when_model_calls_tool() {
        let (url, server) = serve(vec![reply(r#"{"ok":true}"#), tool_reply("echo_number", r#"{"n":7}"#)]);
        let probe = client(&url, None).probe().await;
        let requests = server.join().unwrap();
        assert!(probe.reachable && probe.json_ok && probe.tools_ok, "{probe:?}");
        assert_eq!(probe.error, None);
        assert_eq!(body_of(&requests[1])["tools"][0]["function"]["name"], "echo_number");
    }

    #[tokio::test]
    async fn probe_reports_tools_not_ok_on_plain_text_reply() {
        let (url, server) = serve(vec![reply(r#"{"ok":true}"#), reply("The number is 7.")]);
        let probe = client(&url, None).probe().await;
        server.join().unwrap();
        assert!(probe.reachable && probe.json_ok && !probe.tools_ok, "{probe:?}");
        assert_eq!(probe.error, None);
    }

    #[tokio::test]
    async fn probe_reports_tools_not_ok_on_the_wrong_number_or_tool() {
        for wrong in [tool_reply("echo_number", r#"{"n":8}"#), tool_reply("other", r#"{"n":7}"#), tool_reply("echo_number", "nope")] {
            let (url, server) = serve(vec![reply("{}"), wrong]);
            let probe = client(&url, None).probe().await;
            server.join().unwrap();
            assert!(!probe.tools_ok, "{probe:?}");
        }
    }

    #[tokio::test]
    async fn probe_treats_a_server_that_refuses_tools_as_no_tools() {
        let (url, server) =
            serve(vec![reply("{}"), (400, "does not support tools".into()), (400, "does not support tools".into())]);
        let probe = client(&url, None).probe().await;
        server.join().unwrap();
        assert!(probe.reachable && probe.json_ok && !probe.tools_ok, "{probe:?}");
    }

    #[tokio::test]
    async fn probe_reports_unreachable_on_connection_refused() {
        let probe = client(&refused_url(), None).probe().await;
        assert!(!probe.reachable && !probe.json_ok && !probe.tools_ok, "{probe:?}");
        assert!(probe.error.is_some());
    }

    #[tokio::test]
    async fn probe_shows_a_refusal_without_trying_tools() {
        let (url, server) = serve(vec![(401, "bad key sk-secret".into())]);
        let probe = client(&url, Some("sk-secret")).probe().await;
        assert_eq!(server.join().unwrap().len(), 1);
        assert!(probe.reachable && !probe.json_ok, "{probe:?}");
        let error = probe.error.unwrap();
        assert!(error.contains("401") && !error.contains("sk-secret"), "{error}");
    }

    #[tokio::test]
    async fn probe_says_when_no_model_is_set() {
        let client = LlmClient::new(
            LlmSettings { model: String::new(), ..LlmSettings::default() },
            None,
            reqwest::Client::new(),
            Arc::new(LlmBudget::new(1)),
        );
        let probe = client.probe().await;
        assert!(!probe.reachable);
        assert_eq!(probe.error.as_deref(), Some("no model is set"));
    }

    #[tokio::test]
    async fn probe_takes_no_budget() {
        let (url, server) = serve(vec![reply("{}"), tool_reply("echo_number", r#"{"n":7}"#)]);
        let probe = client_with_budget(&url, None, 0).probe().await;
        server.join().unwrap();
        assert!(probe.tools_ok, "{probe:?}");
    }

    #[tokio::test]
    async fn a_used_up_budget_sends_nothing() {
        let (url, server) = serve(vec![reply("{}")]);
        let client = client_with_budget(&url, None, 1);
        client.chat_json("s", "u").await.unwrap();
        server.join().unwrap();
        // The server has stopped, so a second send could only come back as Unreachable.
        assert!(matches!(client.chat_json("s", "u").await, Err(LlmError::BudgetExhausted)));
    }

    #[tokio::test]
    async fn a_missing_model_spends_no_budget() {
        let budget = Arc::new(LlmBudget::new(1));
        let client = LlmClient::new(
            LlmSettings { model: " ".into(), ..LlmSettings::default() },
            None,
            reqwest::Client::new(),
            budget.clone(),
        );
        assert!(matches!(client.chat_json("s", "u").await, Err(LlmError::NoModel)));
        assert!(budget.try_acquire(Instant::now()), "the one call is still there");
    }
}
