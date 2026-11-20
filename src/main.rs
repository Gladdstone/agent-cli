mod config;
mod core;
mod provider;
mod tools;
mod tui;

use std::io::Write;
use clap::{Parser, Subcommand};
use crate::core::agent::Agent;
use crate::core::event::Event;
use crate::provider::qwen::QwenProvider;
use crate::tools::Registry;
use crate::tui::app;

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
            run(config, prompt).await?;
        }
        Some(Command::Config) => println!("{config:#?}"),
        None => app::run(config).await?,
    }

    Ok(())
}

async fn run(config: config::Config, prompt: String) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let agent = Agent {
        // provider: AnthropicProvider::from_env()?,
        provider: QwenProvider::from_env()?,
        tools: Registry::default_set(),
        model: config.model,
        system: Some(format!(
            "You are a coding agent running in a terminal. Current working directory: {}",
            cwd.display()
        )),
        max_steps: config.max_steps,
        max_tokens: config.max_tokens,
    };

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let printer = tokio::spawn(async move {
        let mut stdout = std::io::stdout();
        while let Some(ev) = rx.recv().await {
            match ev {
                Event::TextDelta(t) => {
                    print!("{t}");
                    let _ = stdout.flush();
                }
                Event::ToolStart { name, input, .. } => {
                    eprintln!("[tool] {name}({})", summarize(&input));
                }
                Event::ToolEnd { id, is_error, .. } => {
                    tracing::debug!(id, is_error, "tool end");
                }
                Event::Usage(u) => tracing::debug!(?u, "usage"),
                Event::StepLimit => eprintln!("[step limit reached]"),
                Event::Done => println!(),
            }
        }
    });

    let mut history = Vec::new();
    let result = tokio::select! {
        r = agent.run_turn(&mut history, prompt, &tx) => r,
        _ = tokio::signal::ctrl_c() => Err(anyhow::anyhow!("interrupted")),
    };
    drop(tx);
    let _ = printer.await;
    result
}

fn summarize(input: &serde_json::Value) -> String {
    let s = match input.as_object() {
        Some(o) => o
            .iter()
            .map(|(k, v)| match v.as_str() {
                Some(s) => format!("{k}={s:?}"),
                None => format!("{k}={v}"),
            })
            .collect::<Vec<_>>()
            .join(", "),
        None => input.to_string(),
    };
    match s.char_indices().nth(100) {
        Some((i, _)) => format!("{}..", &s[..i]),
        None => s,
    }
}

