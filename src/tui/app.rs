use std::io::{self, stdout};

use crossterm::{
    event::{Event as CEvent, EventStream, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};

use tokio::sync::mpsc;

use crate::{
    config,
    core::{
        agent::Agent,
        event::Event,
        message::{ContentBlock, Message, Role},
    },
    provider::qwen::QwenProvider,
    provider::Provider,
    tools::Registry,
};

enum UiEvent {
    Agent(Event),
    Finished(Result<(), String>),
}

struct App {
    messages: Vec<Message>,
    input: String,
    status: String,
    busy: bool,
    scroll: u16,
}

impl App {
    fn new() -> Self {
        Self {
            messages: Vec::new(),
            input: String::new(),
            status: "Ready".into(),
            busy: false,
            scroll: 0,
        }
    }

    fn submit(&mut self, prompt: String) {
        self.messages.push(Message {
            role: Role::User,
            content: vec![ContentBlock::Text(prompt)],
        });

        self.busy = true;
        self.status = "Thinking...".into();
        self.scroll = 0;
    }

    fn handle_agent_event(&mut self, event: Event) {
        match event {
            Event::TextDelta(text) => {
                let is_assistant = self
                    .messages
                    .last()
                    .map(|message| message.role == Role::Assistant)
                    .unwrap_or(false);

                if !is_assistant {
                    self.messages.push(Message {
                        role: Role::Assistant,
                        content: vec![ContentBlock::Text(text)],
                    });
                } else if let Some(message) = self.messages.last_mut() {
                    match message.content.last_mut() {
                        Some(ContentBlock::Text(existing)) => {
                            existing.push_str(&text);
                        }
                        _ => {
                            message.content.push(ContentBlock::Text(text));
                        }
                    }
                }

                self.scroll = 0;
            }

            Event::ToolStart { name, input, .. } => {
                self.status = format!(
                    "Running tool: {}({})",
                    name,
                    summarize(&input)
                );
            }

            Event::ToolEnd { is_error, .. } => {
                self.status = if is_error {
                    "Tool execution failed".into()
                } else {
                    "Tool completed".into()
                };
            }

            Event::Usage(_) => {}

            Event::StepLimit => {
                self.status = "Step limit reached".into();
            }

            Event::Done => {
                self.status = "Turn completed".into();
            }
        }
    }

    fn render(&self, frame: &mut ratatui::Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(1),
                Constraint::Length(3),
            ])
            .split(frame.area());

        // Conversation
        let lines = render_messages(&self.messages);

        let content_height = chunks[0].height.saturating_sub(2) as usize;

        let max_scroll = lines.len().saturating_sub(content_height);

        let scroll = max_scroll
            .saturating_sub(self.scroll as usize) as u16;

        let conversation = Paragraph::new(Text::from(lines))
            .block(
                Block::default()
                    .title(" Conversation ")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0));

        frame.render_widget(conversation, chunks[0]);

        // Status
        let status = Paragraph::new(self.status.as_str())
            .style(Style::default().fg(Color::Yellow));

        frame.render_widget(status, chunks[1]);

        // Input
        let input_title = if self.busy {
            " Input (agent working) "
        } else {
            " Input "
        };

        let input = Paragraph::new(self.input.as_str())
            .block(
                Block::default()
                    .title(input_title)
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(input, chunks[2]);

        if !self.busy {
            frame.set_cursor_position((
                chunks[2].x + self.input.len() as u16 + 1,
                chunks[2].y + 1,
            ));
        }
    }
}

// Render existing Message and ContentBlock structures.
fn render_messages(messages: &[Message]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for message in messages {
        let (label, color) = match message.role {
            Role::User => ("You", Color::Cyan),
            Role::Assistant => ("Agent", Color::Green),
        };

        lines.push(Line::from(Span::styled(
            format!("── {label} ──"),
            Style::default()
                .fg(color)
                .add_modifier(Modifier::BOLD),
        )));

        for block in &message.content {
            match block {
                ContentBlock::Text(text) => {
                    for line in text.lines() {
                        lines.push(Line::raw(line.to_string()));
                    }
                }

                ContentBlock::ToolUse {
                    name,
                    input,
                    ..
                } => {
                    lines.push(Line::from(Span::styled(
                        format!("[tool call] {name}"),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )));

                    lines.push(Line::raw(format!("{input:#}")));
                }

                ContentBlock::ToolResult {
                    content,
                    is_error,
                    ..
                } => {
                    let color = if *is_error {
                        Color::Red
                    } else {
                        Color::DarkGray
                    };

                    let label = if *is_error {
                        "[tool error]"
                    } else {
                        "[tool result]"
                    };

                    lines.push(Line::from(Span::styled(
                        label,
                        Style::default().fg(color),
                    )));

                    for line in content.lines() {
                        lines.push(Line::raw(format!("  {line}")));
                    }
                }
            }
        }

        lines.push(Line::raw(""));
    }

    lines
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

pub async fn run(config: config::Config) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;

    let agent = Agent {
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

    enable_raw_mode()?;

    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_ui(&mut terminal, agent).await;

    // Restore terminal regardless of whether the UI returned an error.
    disable_raw_mode()?;

    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::cursor::Show
    )?;

    terminal.show_cursor()?;

    result
}

async fn run_ui(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    agent: Agent<QwenProvider>,
) -> anyhow::Result<()> {
    let mut app = App::new();

    let (prompt_tx, mut prompt_rx) =
        mpsc::unbounded_channel::<String>();

    let (ui_tx, mut ui_rx) =
        mpsc::unbounded_channel::<UiEvent>();

    // Agent worker owns persistent conversation history.
    tokio::spawn(async move {
        let mut history: Vec<Message> = Vec::new();

        while let Some(prompt) = prompt_rx.recv().await {
            let (agent_tx, mut agent_rx) =
                mpsc::unbounded_channel::<Event>();

            let event_tx = ui_tx.clone();

            // Forward agent events to the UI.
            let forwarder = tokio::spawn(async move {
                while let Some(event) = agent_rx.recv().await {
                    if event_tx.send(UiEvent::Agent(event)).is_err() {
                        break;
                    }
                }
            });

            let result = agent
                .run_turn(&mut history, prompt, &agent_tx)
                .await;

            drop(agent_tx);
            let _ = forwarder.await;

            let result = result
                .map(|_| ())
                .map_err(|e| e.to_string());

            if ui_tx.send(UiEvent::Finished(result)).is_err() {
                break;
            }
        }
    });

    let mut events = EventStream::new();
    let mut running = true;

    while running {
        terminal.draw(|frame| app.render(frame))?;

        tokio::select! {
            // Keyboard input
            event = events.next() => {
                if let Some(Ok(CEvent::Key(key))) = event {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                            running = false;
                        }

                        (KeyCode::Enter, _) if !app.busy => {
                            let prompt = app.input.trim().to_string();

                            if !prompt.is_empty() {
                                app.input.clear();
                                app.submit(prompt.clone());

                                if prompt_tx.send(prompt).is_err() {
                                    app.busy = false;
                                    app.status = "Agent unavailable".into();
                                }
                            }
                        }

                        (KeyCode::Char(c), _) if !app.busy => {
                            app.input.push(c);
                        }

                        (KeyCode::Backspace, _) if !app.busy => {
                            app.input.pop();
                        }

                        (KeyCode::Up, _) => {
                            app.scroll = app.scroll.saturating_add(1);
                        }

                        (KeyCode::Down, _) => {
                            app.scroll = app.scroll.saturating_sub(1);
                        }

                        _ => {}
                    }
                }
            }

            // Agent events
            Some(event) = ui_rx.recv() => {
                match event {
                    UiEvent::Agent(event) => {
                        app.handle_agent_event(event);
                    }

                    UiEvent::Finished(result) => {
                        app.busy = false;

                        match result {
                            Ok(()) => {
                                app.status = "Ready".into();
                            }

                            Err(error) => {
                                app.status = format!("Error: {error}");
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

