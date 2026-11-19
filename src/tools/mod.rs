#![allow(dead_code)]

use std::future::Future;
use std::pin::Pin;

use serde_json::Value;
use crate::provider::ToolSpec;

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> Value;
    fn execute<'a>(
        &'a self,
        input: Value,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>>;
}

#[derive(Default)]
pub struct Registry {
    tools: Vec<Box<dyn Tool>>,
}

impl Registry {
    pub fn new(tools: Vec<Box<dyn Tool>>) -> Self {
        Self { tools }
    }

    pub fn default_set() -> Self {
        Self::new(vec![
            // Box::new(read::ReadTool),
            // Box::new(write::WriteTool),
            // Box::new(bash::BashTool::default()),
        ])
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools
            .iter()
            .map(|t| ToolSpec {
                name: t.name().to_string(),
                description: t.description().to_string(),
                input_schema: t.schema(),
            })
            .collect()
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.iter().find(|t| t.name() == name).map(|t| &**t)
    }
}

pub fn truncate(mut s: String, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s;
    }

    let mut end = max_bytes;
    while !s.is_char_boundary(end) {
        end -= 1;
    }

    let omitted = s.len() - end;
    s.truncate(end);
    s.push_str(&format!("\n[output truncated: {omitted} bytes omitted]"));
    s
}

