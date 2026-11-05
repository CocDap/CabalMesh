use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::error::Error;

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaRequest {
    pub model: String,
    pub prompt: String,
    pub stream: bool,
    pub system: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaResponse {
    pub response: String,
    pub done: bool,
}

/// Real local-LLM read of a PDF page's extracted text — classifies what kind
/// of book/document content it looks like, and flags text that doesn't read
/// like genuine document content (e.g. gibberish, placeholder text, garbled
/// PDF-extraction noise) so a listing isn't created on top of it unchecked.
#[derive(Debug, Serialize, Deserialize)]
pub struct ContentAnalysis {
    pub content_type: String,
    pub is_real_document: bool,
    pub reasoning: String,
}

pub struct SharkAgent {
    client: Client,
    ollama_url: Option<String>,
}

impl SharkAgent {
    pub fn new(ollama_url: Option<String>) -> Self {
        SharkAgent {
            client: Client::new(),
            ollama_url,
        }
    }

    /// Resolved per request rather than cached at construction, so a URL set at
    /// runtime — the only way to reach a model on iOS — applies without a restart.
    fn url(&self) -> String {
        self.ollama_url
            .clone()
            .unwrap_or_else(crate::ollama_config::url)
    }

    /// Asks the local LLM to actually read the extracted PDF text and judge
    /// what kind of book/document content it is, as a real check that the
    /// uploaded page is genuine content rather than noise/gibberish — not a
    /// rubber-stamp, since the model can and does say `is_real_document: false`.
    pub async fn analyze_content(&self, text: &str) -> Result<ContentAnalysis, Box<dyn Error>> {
        let system_prompt = r#"You are a document classifier. You will be given the text extracted from page 1 of an uploaded PDF that is being listed for sale as a "book page" on a marketplace.

Judge honestly whether this reads like genuine book/document content (a real excerpt of prose, a technical page, a poem, etc.) versus meaningless noise (garbled extraction artifacts, random characters, an empty/near-empty page, or placeholder text).

Respond ONLY with JSON in this exact format:
{
  "content_type": "<short label, e.g. 'Fiction novel excerpt', 'Technical manual page', 'Unclear / not enough text'>",
  "is_real_document": <true or false>,
  "reasoning": "<one short sentence>"
}"#.to_string();

        let request = OllamaRequest {
            model: "llama2".to_string(),
            prompt: format!("Classify this page 1 text:\n\n{}", text),
            stream: false,
            system: Some(system_prompt),
        };

        let response = self
            .client
            .post(format!("{}/api/generate", self.url()))
            .json(&request)
            .send()
            .await?;

        let ollama_response: OllamaResponse = response.json().await?;

        // llama2 often wraps its JSON in explanatory prose despite being told
        // not to — parse just the {...} block, not the whole raw response.
        let parsed: serde_json::Value = serde_json::from_str(crate::llm_json::extract_json_object(&ollama_response.response))
            .unwrap_or_else(|_| {
                serde_json::json!({
                    "content_type": "Unknown (model did not return valid JSON)",
                    "is_real_document": false,
                    "reasoning": "Local LLM response could not be parsed."
                })
            });

        Ok(ContentAnalysis {
            content_type: parsed["content_type"].as_str().unwrap_or("Unknown").to_string(),
            is_real_document: parsed["is_real_document"].as_bool().unwrap_or(false),
            reasoning: parsed["reasoning"].as_str().unwrap_or("").to_string(),
        })
    }

}
