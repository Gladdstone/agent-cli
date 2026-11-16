#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    pub model: String,
    pub max_steps: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: "claude-sonnet-4-5".into(),
            max_steps: 50,
        }
    }
}

