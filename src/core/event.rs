use serde_json::Value;

use super::message::Usage;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    TextDelta(String),
    ToolStart {
        id: String,
        name: String,
        input: Value,
    },
    ToolEnd {
        id: String,
        output: String,
        is_error: bool,
    },
    Usage(Usage),
    StepLimit,
    Done,
}

