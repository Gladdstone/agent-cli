use futures_util::future::join_all;
use tokio::sync::mpsc::UnboundedSender;

use super::event::Event;
use super::message::{ContentBlock, Message, Role};
use crate::provider::{Delta, Provider, Request};
use crate::tools::{Registry, truncate};

const MAX_TOOL_OUTPUT: usize = 50 * 1024;

pub struct Agent<P: Provider> {
    pub provider: P,
    pub tools: Registry,
    pub model: String,
    pub system: Option<String>,
    pub max_steps: u32,
    pub max_tokens: u32
}

impl<P: Provider> Agent<P> {
    pub async fn run_turn(
        &self,
        history: &mut Vec<Message>,
        user_input: String,
        events: &UnboundedSender<Event>,
    ) -> anyhow::Result<()> {
        history.push(Message::user_text(user_input));
        let specs = self.tools.specs();

        for _ in 0..self.max_steps {
            let resp = {
                let req = Request {
                    model: &self.model,
                    system: self.system.as_deref(),
                    messages: history,
                    tools: &specs,
                    max_tokens: self.max_tokens,
                };
                let mut on_delta = |d: Delta| {
                    let Delta::Text(t) = d;
                    let _ = events.send(Event::TextDelta(t));
                };
                self.provider.stream(&req, &mut on_delta).await?
            };

            let _ = events.send(Event::Usage(resp.usage));
            let calls: Vec<(String, String, serde_json::Value)> = resp
                .message
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolUse { id, name, input } => {
                        Some((id.clone(), name.clone(), input.clone()))
                    }
                    _ => None,
                })
                .collect();
            history.push(resp.message);

            if calls.is_empty() {
                let _ = events.send(Event::Done);
                return Ok(());
            }

            for (id, name, input) in &calls {
                let _ = events.send(Event::ToolStart {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                });
            }

            let results = join_all(calls.into_iter().map(|(id, name, input)| async move {
                let (output, is_error) = match self.tools.get(&name) {
                    None => (format!("unknown tool: {name}"), true),
                    Some(tool) => match tool.execute(input).await {
                        Ok(out) => (truncate(out, MAX_TOOL_OUTPUT), false),
                        Err(e) => (truncate(format!("{e:#}"), MAX_TOOL_OUTPUT), true),
                    },
                };
                (id, output, is_error)
            }))
            .await;

            let mut blocks = Vec::with_capacity(results.len());
            for (id, output, is_error) in results {
                let _ = events.send(Event::ToolEnd {
                    id: id.clone(),
                    output: output.clone(),
                    is_error,
                });
                blocks.push(ContentBlock::ToolResult {
                    id,
                    content: output,
                    is_error,
                });
            }
            history.push(Message {
                role: Role::User,
                content: blocks,
            });
        }

        let _ = events.send(Event::StepLimit);
        let _ = events.send(Event::Done);
        Ok(())
    }
}

