use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use anyhow::{Context, Result, anyhow};
use serde_json::{json, Value};

#[derive(Debug, Clone, Default)]
pub struct SessionOptions {
    pub model: Option<String>,
    pub cwd: Option<PathBuf>,
    pub system_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    pub permission_mode: Option<String>,
    pub max_turns: Option<u32>,
}


#[derive(Debug, Clone)]
pub struct PermissionRequest {
    pub request_id: String,
    pub tool_name: String,
    pub input: Value,
}


#[derive(Debug, Clone)]
pub enum PermissionDecision {
    Allow,
    AllowWithInput(Value),
    Deny(String),
}

#[derive(Debug, Clone)]
pub struct TurnResult {
    pub text: String,
    pub is_error: bool,
    pub num_turns: u64,
    pub total_cost_usd: Option<f64>,
}


#[derive(Debug, Clone)]
pub enum AgentEvent {
    Init { session_id: String, model: String },
    Text(String),
    Thinking(String),
    ToolUse { id: String, name: String, input: Value },
    ToolResult { id: String, content: String, is_error: bool },
    PermissionRequest(PermissionRequest),
    Result(TurnResult),
    Other(Value),
}

pub struct AnthropicSession {
    pub session_id: String,
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl AnthropicSession {
    pub fn create(opts: &SessionOptions) -> Result<Self> {
        let id = uuid::Uuid::new_v4().to_string();
        Self::spawn(opts, &["--session-id", &id], id.clone())
    }

    pub fn resume(session_id: &str, opts: &SessionOptions) -> Result<Self> {
        Self::spawn(opts, &["--resume", session_id], session_id.to_string())
    }

    fn spawn(opts: &SessionOptions, session_args: &[&str], session_id: String) -> Result<Self> {
        let mut cmd = Command::new("claude");
        cmd.args([
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--permission-prompt-tool",
            "stdio",
        ])
        .args(session_args);

        if let Some(v) = &opts.model {
            cmd.args(["--model", v]);
        }
        if let Some(v) = &opts.system_prompt {
            cmd.args(["--system-prompt", v]);
        }
        if let Some(v) = &opts.append_system_prompt {
            cmd.args(["--append-system-prompt", v]);
        }
        if !opts.allowed_tools.is_empty() {
            cmd.args(["--allowedTools", &opts.allowed_tools.join(",")]);
        }
        if !opts.disallowed_tools.is_empty() {
            cmd.args(["--disallowedTools", &opts.disallowed_tools.join(",")]);
        }
        if let Some(v) = &opts.permission_mode {
            cmd.args(["--permission-mode", v]);
        }
        if let Some(v) = opts.max_turns {
            cmd.args(["--max-turns", &v.to_string()]);
        }
        if let Some(dir) = &opts.cwd {
            cmd.current_dir(dir);
        }

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("failed to spawn `claude` (is it installed and on PATH?)")?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = BufReader::new(child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?);

        Ok(Self {
            session_id,
            child,
            stdin: Some(stdin),
            stdout,
        })
    }

    fn write_line(&mut self, v: &Value) -> Result<()> {
        let stdin = self.stdin.as_mut().ok_or_else(|| anyhow!("session closed"))?;
        writeln!(stdin, "{v}")?;
        stdin.flush()?;
        Ok(())
    }

    pub fn send(&mut self, text: &str) -> Result<()> {
        self.write_line(&json!({
            "type": "user",
            "message": { "role": "user", "content": [{ "type": "text", "text": text }] },
        }))
    }

    pub fn respond_permission(
        &mut self,
        req: &PermissionRequest,
        decision: PermissionDecision,
    ) -> Result<()> {
        let body = match decision {
            PermissionDecision::Allow => {
                json!({ "behavior": "allow", "updatedInput": req.input })
            }
            PermissionDecision::AllowWithInput(input) => {
                json!({ "behavior": "allow", "updatedInput": input })
            }
            PermissionDecision::Deny(message) => {
                json!({ "behavior": "deny", "message": message })
            }
        };
        self.write_line(&json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": req.request_id,
                "response": body,
            },
        }))
    }

    pub fn interrupt(&mut self) -> Result<()> {
        self.write_line(&json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": uuid::Uuid::new_v4().to_string(),
                "request": { "subtype": "interrupt" },
            },
        }))
    }

    pub fn next_event(&mut self) -> Result<Option<Vec<AgentEvent>>> {
        let mut line = String::new();
        loop {
            line.clear();
            if self.stdout.read_line(&mut line)? == 0 {
                return Ok(None);
            }
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let v: Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(%e, line, "non-JSON line from claude");
                    continue;
                }
            };

            let events = self.parse(v);
            if events.is_empty() {
                continue;
            }
            return Ok(Some(events));
        }
    }

    fn parse(&mut self, v: Value) -> Vec<AgentEvent> {
        let ty = v["type"].as_str().unwrap_or("");
        match ty {
            "system" if v["subtype"] == "init" => {
                if let Some(id) = v["session_id"].as_str() {
                    self.session_id = id.to_string();
                }
                vec![AgentEvent::Init {
                    session_id: self.session_id.clone(),
                    model: v["model"].as_str().unwrap_or("").to_string(),
                }]
            }
            "assistant" | "user" => parse_content(&v["message"]["content"]),
            "control_request" if v["request"]["subtype"] == "can_use_tool" => {
                vec![AgentEvent::PermissionRequest(PermissionRequest {
                    request_id: v["request_id"].as_str().unwrap_or("").to_string(),
                    tool_name: v["request"]["tool_name"].as_str().unwrap_or("").to_string(),
                    input: v["request"]["input"].clone(),
                })]
            }
            "result" => vec![AgentEvent::Result(TurnResult {
                text: v["result"].as_str().unwrap_or("").to_string(),
                is_error: v["is_error"].as_bool().unwrap_or(false),
                num_turns: v["num_turns"].as_u64().unwrap_or(0),
                total_cost_usd: v["total_cost_usd"].as_f64(),
            })],
            "control_response" | "stream_event" | "keep_alive" => vec![],
            _ => vec![AgentEvent::Other(v)],
        }
    }

    pub fn run_turn(
        &mut self,
        text: &str,
        mut on_event: impl FnMut(&AgentEvent),
        mut on_permission: impl FnMut(&PermissionRequest) -> PermissionDecision,
    ) -> Result<TurnResult> {
        self.send(text)?;
        while let Some(events) = self.next_event()? {
            for ev in events {
                on_event(&ev);
                match ev {
                    AgentEvent::PermissionRequest(req) => {
                        let d = on_permission(&req);
                        self.respond_permission(&req, d)?;
                    }
                    AgentEvent::Result(r) => return Ok(r),
                    _ => {}
                }
            }
        }
        Err(anyhow!("claude exited before producing a result"))
    }

    pub fn close(mut self) -> Result<()> {
        self.stdin.take();
        self.child.wait()?;
        Ok(())
    }
}

impl Drop for AnthropicSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn parse_content(content: &Value) -> Vec<AgentEvent> {
    if let Some(s) = content.as_str() {
        return vec![AgentEvent::Text(s.to_string())];
    }

    let mut out = Vec::new();
    for b in content.as_array().into_iter().flatten() {
        match b["type"].as_str().unwrap_or("") {
            "text" => out.push(AgentEvent::Text(b["text"].as_str().unwrap_or("").into())),
            "thinking" => out.push(AgentEvent::Thinking(
                b["thinking"].as_str().unwrap_or("").into(),
            )),
            "tool_use" => out.push(AgentEvent::ToolUse {
                id: b["id"].as_str().unwrap_or("").into(),
                name: b["name"].as_str().unwrap_or("").into(),
                input: b["input"].clone(),
            }),
            "tool_result" => out.push(AgentEvent::ToolResult {
                id: b["tool_use_id"].as_str().unwrap_or("").into(),
                content: tool_result_text(&b["content"]),
                is_error: b["is_error"].as_bool().unwrap_or(false),
            }),
            _ => {}
        }
    }
    out
}

fn tool_result_text(c: &Value) -> String {
    match c {
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

