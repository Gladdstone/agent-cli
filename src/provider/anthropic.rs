use reqwest::{Client as ReqwestClient};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug)]
pub struct AnthropicUsage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AnthropicContentBlock {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AnthropicTextDelta {
    #[serde(rename = "type")]
    pub delta_type: Option<String>,
    pub text: Option<String>,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
    pub usage: Option<AnthropicUsage>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AnthropicMessage {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub message_type: String,
    pub role: Option<String>,
    pub content: Option<Vec<AnthropicContentBlock>>,
    pub model: Option<String>,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
    pub usage: Option<AnthropicUsage>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub struct AnthropicChatCompletionChunk {
    #[serde(rename = "type")]
    pub event_type: String,
    pub index: Option<usize>,
    pub delta: Option<AnthropicTextDelta>,
    pub message: Option<AnthropicMessage>
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AnthropicErrorMessage {
    #[serde(rename = "type")]
    pub error_type: String,
    pub error: AnthropicErrorDetails,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AnthropicErrorDetails {
    pub details: Option<serde_json::Value>,
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

#[derive(Debug)]
pub enum ToolChoice {
    Auto,
    Any,
    Tool(String),
}

impl Serialize for ToolChoice {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            ToolChoice::Auto => serde::Serialize::serialize(&serde_json::json!({"type": "auto"}), serializer),
            ToolChoice::Any => serde::Serialize::serialize(&serde_json::json!({"type": "any"}), serializer),
            ToolChoice::Tool(name) => serde::Serialize::serialize(&serde_json::json!({"type": "tool", "name": name}), serializer),
        }
    }
}

pub struct Client {
    client: ReqwestClient,
    secret_key: String,
    model: String,
    messages: Value,
    tools: Value,
    // tool_choice: Option<types::ToolChoice>,
    metadata: Value,
    max_tokens: i32,
    stream: bool,
    verbose: bool,
    temperature: f32,
    system: String,
    version: String,
    stop_sequences: Vec<String>,
    beta: Option<String>,
    top_k: Option<i32>,
    top_p: Option<f64>,
}

#[derive(Deserialize)]
struct JsonResponse {
    content: Vec<Content>,
}

#[derive(Deserialize)]
struct Content {
    #[serde(rename = "type")]
    content_type: String,
    text: String,
}

impl Client {
    pub fn new() -> Self {
        Self {
            client: ReqwestClient::new(),
            secret_key: String::new(),
            model: String::new(),
            messages: Value::Null,
            tools: Value::Null,
            // tool_choice: None,
            metadata: Value::Null,
            max_tokens: 1024,
            stream: false,
            verbose: false,
            temperature: 0.0,
            system: String::new(),
            version: "2023-06-01".to_string(),
            stop_sequences: Vec::new(),
            beta: None,
            top_k: None,
            top_p: None,
        }
    }

    pub fn build(self) {
        let mut body_map: HashMap<&str, Value> = HashMap::new();
        body_map.insert("model", json!(self.model));
        body_map.insert("max_tokens", json!(self.max_tokens));
        body_map.insert("messages", json!(self.messages));
        body_map.insert("stream", json!(self.stream));
        body_map.insert("temperature", json!(self.temperature));
        body_map.insert("system", json!(self.system));

        if self.tools != Value::Null {
            body_map.insert("tools", self.tools.clone());
        }

        if self.metadata != Value::Null {
            body_map.insert("metadata", self.metadata.clone());
        }

        if self.stop_sequences.len() > 0 {
            body_map.insert("stop_sequences", json!(self.stop_sequences));
        }

        if let Some(top_k) = self.top_k {
            body_map.insert("top_k", json!(top_k));
        }

        if let Some(top_p) = self.top_p {
            body_map.insert("top_p", json!(top_p));
        }

        let mut request_builder = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", self.secret_key)
            .header("anthropic-version", self.version)
            .header("content-type", "application/json")
            .json(&body_map);

        if let Some(beta_value) = self.beta {
            request_builder = request_builder.header("anthropic-beta", beta_value);
        }

        // Ok(request_builder)
    }
}

