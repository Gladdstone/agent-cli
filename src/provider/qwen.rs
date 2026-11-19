use std::collections::HashMap;
use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use serde_json::{json, Value};

use crate::core::message::{ContentBlock, Message, Role, Usage};
use super::{Delta, Provider, Request, Response};
use super::sse::{SseEvent, SseParser};


const DEFAULT_BASE_URL: &str = "http://localhost:11434";

pub struct QwenProvider {
    client: reqwest::Client,
    base_url: String,
}

// TODO - change this from qwen naming to openai
impl QwenProvider {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }

    pub fn from_env() -> Result<Self> {
        let base_url = std::env::var("QWEN_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.into());
        Ok(Self::new(base_url))
    }
}

// {
//     "model": "qwen2.5",
//     "messages": [
//         {
//             "role": "system",
//             "content": "You are a helpful assistant."
//         },
//         {
//             "role": "user",
//             "content": "What is the weather in Seattle?"
//         }
//     ],
//     "tools": [
//         {
//             "type": "function",
//             "function": {
//                 "name": "get_weather",
//                 "description": "Get the current weather for a city",
//                 "parameters": {
//                     "type": "object",
//                     "properties": {
//                         "city": {
//                             "type": "string",
//                             "description": "City name"
//                         }
//                     },
//                     "required": ["city"]
//                 }
//             }
//         }
//     ],
//     "tool_choice": "auto",
//     "stream": false
// }
pub fn build_body(req: &Request<'_>) -> Value {
    let mut body = json!({
        "model": req.model,
        "stream": true,
        "messages": req.messages.iter().map(message_json).collect::<Vec<_>>(),
    });
    // if let Some(system) = req.system {
    //     body["system"] = json!(system);
    // }
    println!("{}",body);
    // tools
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

impl Provider for QwenProvider {
    async fn stream(
        &self,
        req: &Request<'_>,
        on_delta: &mut (dyn FnMut(Delta) + Send),
    ) -> Result<Response> {
        let resp = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .header("Content-Type", "application/json")
            .json(&build_body(req))
            .send()
            .await
            .context("request to Anthropic failed")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("Qwen API returned {status}: {body}");
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
        let data = ev.data.trim();

        // OpenAI-compatible SSE stream terminator.
        if data == "[DONE]" {
            self.done = true;
            return Ok(());
        }

        if data.is_empty() {
            return Ok(());
        }

        let v: Value =
            serde_json::from_str(data).context("invalid SSE JSON payload")?;

        let object = v["object"].as_str().unwrap_or("");

        match object {
            "chat.completion.chunk" => {
                let choices = match v["choices"].as_array() {
                    Some(choices) => choices,
                    None => return Ok(()),
                };

                for choice in choices {
                    let delta = &choice["delta"];

                    // Stream text content.
                    if let Some(text) = delta["content"].as_str() {
                        if !text.is_empty() {
                            on_delta(Delta::Text(text.to_string()));

                            // Accumulate the complete response.
                            if let Some(Partial::Text(buf)) =
                                self.open.get_mut(&0)
                            {
                                buf.push_str(text);
                            } else {
                                self.open.insert(
                                    0,
                                    Partial::Text(text.to_string()),
                                );
                            }
                        }
                    }

                    // Handle completion.
                    if let Some(reason) =
                        choice["finish_reason"].as_str()
                    {
                        self.stop_reason = Some(reason.to_string());

                        // Finalize accumulated text.
                        if let Some(Partial::Text(text)) =
                            self.open.remove(&0)
                        {
                            self.content.push(ContentBlock::Text(text));
                        }
                    }
                }

                // Usage may be included in a final chunk when
                // stream_options.include_usage is enabled.
                if let Some(n) = v["usage"]["prompt_tokens"].as_u64() {
                    self.usage.input_tokens = n;
                }

                if let Some(n) = v["usage"]["completion_tokens"].as_u64() {
                    self.usage.output_tokens = n;
                }
            }

            _ => {}
        }

        Ok(())
    }

    pub fn finish(self) -> Result<Response> {
        if !self.done {
            bail!("stream ended before [DONE]");
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


// Response: {
// "id":"chatcmpl-593",
// "object":"chat.completion",
// "created":1794735674,
// "model":"qwen2.5-coder:7b",
// "system_fingerprint":"fp_ollama",
// "choices":
//  [{"index":0,
//      "message":{
//          "role":"assistant",
//          "content":"Hello! How can I assist you today?"
//      },
//      "finish_reason":"stop"
//  }],
//  "usage":{
//      "prompt_tokens":32,
//      "prompt_tokens_details":{
//          "cached_tokens":31
//      },
//      "completion_tokens":10,
//      "total_tokens":42
//      }
//  }

