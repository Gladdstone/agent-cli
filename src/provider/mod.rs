#![allow(dead_code)]
pub mod anthropic;
pub mod anthropic_agent;
pub mod qwen;
pub mod sse;

use serde_json::Value;
use crate::core::message::{Message, Usage};

#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub struct Request<'a> {
    pub model: &'a str,
    pub system: Option<&'a str>,
    pub messages: &'a [Message],
    pub tools: &'a [ToolSpec],
    pub max_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delta {
    Text(String),
}

#[derive(Debug, Clone)]
pub struct Response {
    pub message: Message,
    pub stop_reason: Option<String>,
    pub usage: Usage,
}

pub trait Provider {
    async fn stream(
        &self,
        req: &Request<'_>,
        on_delta: &mut (dyn FnMut(Delta) + Send),
    ) -> anyhow::Result<Response>;
}

