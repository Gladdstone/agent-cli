use anyhow::{anyhow, Context, Result};
use reqwest::{RequestBuilder, StatusCode};
use serde_json::{Value};

use super::anthropic::{AnthropicChatCompletionChunk, AnthropicErrorMessage};
use crate::core::message::JsonResponse;


pub struct Request {
    request_builder: RequestBuilder,
    stream: bool,
    verbose: bool,
    tools: Value,
}

impl Request {
    pub async fn execute<F, Fut>(self, mut callback: F) -> Result<()>
    where
        F: FnMut(String) -> Fut,
        Fut: std::future::Future<Output = ()> + Send,
    {
        let mut response = self
            .request_builder
            .send()
            .await
            .context("Failed to send request")?;

        match response.status() {
            StatusCode::OK => {
                if self.stream {
                    let mut buffer = String::new();
                    while let Some(chunk) = response.chunk().await? {
                        let s = match std::str::from_utf8(&chunk) {
                            Ok(v) => v,
                            Err(e) => panic!("Invalid UTF-8 sequence: {}", e),
                        };
                        buffer.push_str(s);
                        loop {
                            if let Some(index) = buffer.find("\n\n") {
                                let chunk = buffer[..index].to_string();
                                buffer.drain(..=index + 1);

                                if self.verbose {
                                    callback(chunk.clone()).await;
                                } else {
                                    if chunk == "data: [DONE]" {
                                        break;
                                    }
                                    let processed_chunk = chunk
                                        .trim_start_matches("event: message_start")
                                        .trim_start_matches("event: content_block_start")
                                        .trim_start_matches("event: ping")
                                        .trim_start_matches("event: content_block_delta")
                                        .trim_start_matches("event: content_block_stop")
                                        .trim_start_matches("event: message_delta")
                                        .trim_start_matches("event: message_stop")
                                        .to_string();
                                    let cleaned_string = &processed_chunk
                                        .trim_start()
                                        .strip_prefix("data: ")
                                        .unwrap_or(&processed_chunk);
                                    match serde_json::from_str::<AnthropicChatCompletionChunk>(
                                        &cleaned_string,
                                    ) {
                                        Ok(d) => {
                                            if let Some(delta) = d.delta {
                                                if let Some(content) = delta.text {
                                                    callback(content).await;
                                                }
                                            }
                                        }
                                        Err(_) => {
                                            let processed_chunk = cleaned_string
                                                .trim_start_matches("event: error")
                                                .to_string();
                                            let cleaned_string = &processed_chunk
                                                .trim_start()
                                                .strip_prefix("data: ")
                                                .unwrap_or(&processed_chunk);
                                            match serde_json::from_str::<AnthropicErrorMessage>(
                                                &cleaned_string,
                                            ) {
                                                Ok(error_message) => {
                                                    return Err(anyhow!("{}: {}", error_message.error.error_type, error_message.error.message));
                                                }
                                                Err(_) => {
                                                    eprintln!(
                                                        "Couldn't parse AnthropicChatCompletionChunk or AnthropicErrorMessage: {}",
                                                        &cleaned_string
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            } else {
                                break;
                            }
                        }
                    }
                } else {
                    let json_text = response
                        .text()
                        .await
                        .context("Failed to read response text")?;
                    if self.tools == Value::Null && !self.verbose {
                        match serde_json::from_str::<JsonResponse>(&json_text) {
                            // Anthropic-style
                            // Ok(parsed_json) => {
                            //     if let Some(content) = parsed_json
                            //         .content
                            //         .iter()
                            //         .find(|c| c.content_type == "text")
                            //     {
                            //         callback(content.text.clone()).await;
                            //     }
                            // }
                            Ok(parsed_json) => {
                                if let Some(choice) = parsed_json.choices.first() {
                                    callback(choice.message.content.clone()).await;
                                }
                            }
                            Err(_) => return Err(anyhow!("Unable to parse JSON")),
                        }
                    } else {
                        callback(json_text).await;
                    }
                }
                Ok(())
            }
            StatusCode::BAD_REQUEST => Err(anyhow!(
                "Bad request. Check your request parameters. {}",
                response.text().await?
            )),
            StatusCode::UNAUTHORIZED => Err(anyhow!("Unauthorized. Check your authorization key.")),
            _ => {
                let error_message = format!("Unexpected status code: {:?}", response.text().await?);
                Err(anyhow!(error_message))
            }
        }
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
