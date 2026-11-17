mod config;
mod core;
mod provider;

use clap::{Parser, Subcommand};
use reqwest::Client;
use crate::core::message::{ContentBlock, Message, Role};
use crate::provider::qwen::{ChatRequest, QwenClient};
use crate::provider::sse::Request;

#[derive(Parser)]
#[command(name="agent", version, about="Terminal coding-agent harness")]
struct Cli {
    #[arg(long, global=true)]
    debug: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    Run {
        prompt: String,
    },
    Config,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let reqwest_client = Client::new();
    let qwen_client = QwenClient::new("http://localhost:11434/v1/chat/completions", "qwen2.5-coder:7b");

    let filter = if cli.debug { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| filter.into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let config = config::Config::default();

    match cli.command {
        Some(Command::Run { prompt}) => {
            tracing::debug!(?prompt, "run");
            let message = Message::new(
                Role::User,
                "Test message.".to_string(),
            );
            let val = "qwen2.5-coder:7b".to_string();
            let chat_request = ChatRequest::new(&val, vec![message], 0.7, 1024);
            let req = reqwest_client
                .post(qwen_client.endpoint())
                .header("Content-Type", "application/json")
                .json(&chat_request);
            let sse_request = Request {
                request_builder: req,
                stream: true,
                verbose: true,
                tools: Value::Null,
            };
            sse_request.execute();
            println!("{:#?}", req);
        }
        Some(Command::Config) => println!("{config:#?}"),
        None => println!("TUI not implemented yet. Try `agent --help`."),
    }

    Ok(())
}
