#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    pub model: String,
    pub max_steps: u32,
    pub max_tokens: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: "qwen2.5-coder:7b".into(),
            max_steps: 50,
            max_tokens: 8192,
        }
    }
}

