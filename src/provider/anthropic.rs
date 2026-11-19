use anyhow::{Context, Result, anyhow, bail};
use serde_json::{json, Value};
use std::collections::HashMap;
use futures_util::StreamExt;

use super::sse::{SseEvent, SseParser};
use super::{Delta, Provider, Request, Response};
use crate::core::message::{ContentBlock, Message, Role, Usage};

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }

    pub fn from_env() -> Result<Self> {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| anyhow!("ANTHROPIC_API_KEY is not set"))?;
        let base = std::env::var("ANTHROPIC_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.into());
        Ok(Self::new(key, base))
    }
}

pub fn build_body(req: &Request<'_>) -> Value {
    let mut body = json!({
        "model": req.model,
        "max_tokens": req.max_tokens,
        "stream": true,
        "messages": req.messages.iter().map(message_json).collect::<Vec<_>>(),
    });
    if let Some(system) = req.system {
        body["system"] = json!(system);
    }
    if !req.tools.is_empty() {
        body["tools"] = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            })
            .collect();
    }
    body
}

fn message_json(m: &Message) -> Value {
    let role = match m.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };

    let content: Vec<Value> = m
        .content
        .iter()
        .map(|b| match b {
            ContentBlock::Text(text) => json!({"type": "text", "text": text}),
            ContentBlock::ToolUse { id, name, input } => {
                json!({"type": "tool_use", "id": id, "name": name, "input": input})
            }
            ContentBlock::ToolResult {
                id,
                content,
                is_error,
            } => json!({
                "type": "tool_result",
                "tool_use_id": id,
                "content": content,
                "is_error": is_error,
            }),
        })
        .collect();
    json!({"role": role, "content": content})
}

impl Provider for AnthropicProvider {
    async fn stream(
        &self,
        req: &Request<'_>,
        on_delta: &mut (dyn FnMut(Delta) + Send),
    ) -> Result<Response> {
        let resp = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&build_body(req))
            .send()
            .await
            .context("request to Anthropic failed")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("Anthropic API returned {status}: {body}");
        }

        let mut parser = SseParser::new();
        let mut acc = StreamAccumulator::default();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("error reading response stream")?;
            for ev in parser.push(&chunk) {
                acc.handle(&ev, on_delta)?;
            }
        }
        acc.finish()
    }
}

enum Partial {
    Text(String),
    Tool {
        id: String,
        name: String,
        json: String,
    },
    Ignored,
}

#[derive(Default)]
pub struct StreamAccumulator {
    open: HashMap<u64, Partial>,
    content: Vec<ContentBlock>,
    usage: Usage,
    stop_reason: Option<String>,
    done: bool,
}

impl StreamAccumulator {
    pub fn handle(
        &mut self,
        ev: &SseEvent,
        on_delta: &mut (dyn FnMut(Delta) + Send),
    ) -> Result<()> {
        if ev.data.trim().is_empty() {
            return Ok(());
        }
        println!("{:?}", &ev);
        let v: Value = serde_json::from_str(&ev.data).context("invalid SSE JSON payload")?;
        let kind = v["type"].as_str().or(ev.event.as_deref()).unwrap_or("");
        let index = v["index"].as_u64().unwrap_or(0);
        match kind {
            "message_start" => {
                let u = &v["message"]["usage"];
                self.usage.input_tokens = u["input_tokens"].as_u64().unwrap_or(0);
                self.usage.output_tokens = u["output_tokens"].as_u64().unwrap_or(0);
            },
            "content_block_start" => {
                let b = &v["content_block"];
                let partial = match b["type"].as_str() {
                    Some("text") => {
                        let t = b["text"].as_str().unwrap_or("");
                        if !t.is_empty() {
                            on_delta(Delta::Text(t.to_string()));
                        }
                        Partial::Text(t.to_string())
                    }
                    Some("tool_use") => Partial::Tool {
                        id: b["id"].as_str().unwrap_or("").to_string(),
                        name: b["name"].as_str().unwrap_or("").to_string(),
                        json: String::new(),
                    },
                    _ => Partial::Ignored,
                };
                self.open.insert(index, partial);
            },
            "content_block_delta" => {
                let d = &v["delta"];
                match (self.open.get_mut(&index), d["type"].as_str()) {
                    (Some(Partial::Text(buf)), Some("text_delta")) => {
                        let t = d["text"].as_str().unwrap_or("");
                        buf.push_str(t);
                        on_delta(Delta::Text(t.to_string()));
                    }
                    (Some(Partial::Tool { json, .. }), Some("input_json_delta")) => {
                        json.push_str(d["partial_json"].as_str().unwrap_or(""));
                    }
                    _ => {}
                }
            }
            "content_block_stop" => match self.open.remove(&index) {
                Some(Partial::Text(t)) => self.content.push(ContentBlock::Text(t)),
                Some(Partial::Tool { id, name, json }) => {
                    let input = if json.trim().is_empty() {
                        json!({})
                    } else {
                        serde_json::from_str(&json)
                            .with_context(|| format!("invalid tool input JSON for {name}"))?
                    };
                    self.content.push(ContentBlock::ToolUse { id, name, input });
                }
                _ => {}
            },
            "message_delta" => {
                if let Some(r) = v["delta"]["stop_reason"].as_str() {
                    self.stop_reason = Some(r.to_string());
                }
                if let Some(n) = v["usage"]["output_tokens"].as_u64() {
                    self.usage.output_tokens = n;
                }
                if let Some(n) = v["usage"]["input_tokens"].as_u64() {
                    self.usage.input_tokens = n;
                }
            }
            "message_stop" => self.done = true,
            "error" => {
                let msg = v["error"]["message"].as_str().unwrap_or("unknown error");
                let ty = v["error"]["type"].as_str().unwrap_or("error");
                bail!("Anthropic API error ({ty}): {msg}");
            },
            _ => {}
        }
        Ok(())
    }

    pub fn finish(self) -> Result<Response> {
        if !self.done {
            bail!("stream ended before message_stop");
        }
        Ok(Response {
            message: Message {
                role: Role::Assistant,
                content: self.content,
            },
            stop_reason: self.stop_reason,
            usage: self.usage,
        })
    }
}

