use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::message::{ContentBlock, Message};

pub fn execute_tool(
    name: &str,
    args: &Value,
) -> Result<String, String> {
    match name {
        "get_time" => {
            Ok("Tool executed: time lookup requested".into())
        }

        "echo" => {
            Ok(args["message"]
                .as_str()
                .unwrap_or("")
                .to_string())
        }

        _ => Err(format!("Unknown tool: {}", name)),
    }
}



#[derive(Serialize)]
pub struct ChatRequest<'a> {
    model: &'a String,
    messages: Vec<Message>,
    temperature: f32,
    max_tokens: u32,
}

impl<'a> ChatRequest<'a> {
    pub fn new(model: &'a String, messages: Vec<Message>, temperature: f32, max_tokens: u32) -> Self {
        ChatRequest {
            model: model,
            messages:messages,
            temperature: temperature,
            max_tokens: max_tokens,
        }
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

pub struct QwenClient {
    client: Client,
    endpoint: String,
    model: String,
}

impl QwenClient {
    pub fn new(endpoint: &str, model: &str) -> Self {
        Self {
            client: Client::new(),
            endpoint: endpoint.into(),
            model: model.into(),
        }
    }

    pub fn endpoint(&self) -> &String {
        return &self.endpoint;
    }

    pub async fn generate(
        &self,
        messages: &[Message],
    ) -> Result<String> {
    // ) -> Result<()> {
        let request = ChatRequest {
            model: &self.model,
            messages: messages.to_vec(),
            temperature: 0.7,
            max_tokens: 1024,
        };

        println!("{}", serde_json::to_string_pretty(&request)?);

        let response = self.client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?
            .error_for_status()?;

        let result: ChatResponse = response
            .json()
            .await
            .context("Invalid inference response")?;

        let answer = result.choices
            .first()
            .context("No model response")?
            .message.content.clone();

        // println!("Status: {}", response.status());
        // println!("Response: {}", response.text().await?);

        Ok(answer)
        // Ok(())
    }
}

