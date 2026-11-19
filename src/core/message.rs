use serde::{Deserialize, Serialize};
use serde_json::{Value};

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
        input: Value,
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
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role: role,
            content: vec![ContentBlock::Text(content.into())],
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text(content.into())],
        }
    }

    pub fn user_text(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text(content.into())],
        }
    }

    // pub fn assistant(content: impl Into<String>) -> Self {
    //     Self::new(
    //         Role::Assistant,
    //         content.into(),
    //     )
    // }
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

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl Usage {
    pub fn add(&mut self, other: Usage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
    }
}

// mod serde_json_placeholder {
//     use serde::{Deserialize, Serialize};

//     #[derive(Debug, Deserialize, Clone, PartialEq, Serialize)]
//     pub struct Value;
// }

