use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Serialize)]
pub enum Role {
    #[serde(alias = "user")]
    User,
    #[serde(alias = "assistant")]
    Assistant,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Serialize)]
pub enum ContentBlock {
    Text(String),
    ToolUse {
        id: String,
        name: String,
        input: serde_json_placeholder::Value,
    },
    ToolResult {
        id: String,
        content: String,
        is_error: bool,
    },
}


#[derive(Debug, Deserialize, Clone, PartialEq, Serialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn new(role: Role, content: String) -> Self {
        Self { role, content }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new(
            Role::User,
            content.into(),
        )
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(
            Role::Assistant,
            content.into(),
        )
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub index: u32,
    pub message: Message,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

mod serde_json_placeholder {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Deserialize, Clone, PartialEq, Serialize)]
    pub struct Value;
}

