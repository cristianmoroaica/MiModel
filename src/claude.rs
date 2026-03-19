//! Claude CLI client — spawns `claude` subprocess for each interaction.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Locate the legacy system prompt file (prompts/legacy.md).
/// Walks up from cwd and binary dir to find the project root.
fn find_system_prompt() -> Result<PathBuf, String> {
    let starts: Vec<PathBuf> = [
        std::env::current_dir().ok(),
        std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())),
    ]
    .into_iter()
    .flatten()
    .collect();

    for start in &starts {
        let mut dir = start.as_path();
        loop {
            let candidate = dir.join("prompts/legacy.md");
            if candidate.exists() {
                return Ok(candidate);
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => break,
            }
        }
    }
    Err("prompts/legacy.md not found. Run from within the MiModel project.".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// JSON output from `claude --output-format json`
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ClaudeJsonOutput {
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    is_error: Option<bool>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(rename = "type")]
    #[serde(default)]
    event_type: Option<String>,
    /// For assistant events, contains {"content": [{"text": "..."}]}
    #[serde(default)]
    message: Option<serde_json::Value>,
}

pub struct ClaudeClient {
    model: Option<String>,
    /// Captured from first response — used with --resume on subsequent calls.
    session_id: Option<String>,
    system_prompt: String,
}

impl ClaudeClient {
    pub fn new(model: Option<String>) -> Result<Self, String> {
        let system_prompt_path = find_system_prompt()?;
        let system_prompt = std::fs::read_to_string(&system_prompt_path)
            .map_err(|e| format!("Failed to read system prompt: {e}"))?;

        Ok(Self {
            model,
            session_id: None,
            system_prompt,
        })
    }

    /// Send a prompt to Claude CLI and return the response text.
    /// Thin wrapper around `send_prompt()` that updates `session_id`.
    pub fn send(&mut self, prompt: &str, image_paths: &[PathBuf]) -> Result<String, String> {
        let (result, new_sid) = send_prompt(
            &self.model,
            &self.system_prompt,
            self.session_id.as_deref(),
            prompt,
            image_paths,
            None,
            None,
            None,
            None,
            false,
        )?;
        if let Some(sid) = new_sid {
            self.session_id = Some(sid);
        }
        Ok(result)
    }

    /// Reset the session (for "new" command) — drops session_id so next
    /// call creates a fresh session with --system-prompt.
    pub fn reset(&mut self) {
        self.session_id = None;
    }

    /// Get the current claude session ID.
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Set the claude session ID (e.g. when resuming a saved session).
    pub fn set_session_id(&mut self, id: Option<String>) {
        self.session_id = id;
    }
}

/// Send a prompt to Claude CLI. Returns `(response_text, captured_session_id)`.
///
/// Takes all parameters explicitly so it can be called from a background thread
/// without needing a `&mut ClaudeClient`.
///
/// - First call: pass `session_id = None` and a `system_prompt`; claude creates
///   a new session and returns its ID in the response.
/// - Subsequent calls: pass the captured `session_id` with `--resume`; the
///   `system_prompt` is ignored.
/// Send a prompt to Claude CLI with streaming. Text chunks are sent via
/// `on_text` as they arrive so the TUI can display them live.
/// Returns `(full_response_text, captured_session_id)` when complete.
pub fn send_prompt(
    model: &Option<String>,
    system_prompt: &str,
    session_id: Option<&str>,
    prompt: &str,
    image_paths: &[PathBuf],
    on_text: Option<&std::sync::mpsc::Sender<String>>,
    pid_out: Option<&std::sync::Arc<std::sync::atomic::AtomicU32>>,
    on_tool: Option<&std::sync::mpsc::Sender<super::claude_bridge::ToolCall>>,
    mcp_config: Option<&std::path::Path>,
    disable_builtin_tools: bool,
) -> Result<(String, Option<String>), String> {
    let full_prompt = if image_paths.is_empty() {
        prompt.to_string()
    } else {
        let file_refs: Vec<String> = image_paths.iter()
            .map(|p| crate::image::describe_attachment(p))
            .collect();
        format!(
            "{}\n\n{}\n\nPlease read and analyze the attached file(s) using the Read tool, then generate the CadQuery code.",
            prompt,
            file_refs.join("\n")
        )
    };

    let mut cmd = Command::new("claude");
    cmd.arg("--dangerously-skip-permissions")
        .arg("-p")
        .arg(&full_prompt)
        .arg("--output-format").arg("stream-json")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(ref m) = model {
        cmd.arg("--model").arg(m);
    }

    for path in image_paths {
        if let Some(parent) = path.parent() {
            cmd.arg("--add-dir").arg(parent);
        }
    }

    if disable_builtin_tools {
        if image_paths.is_empty() {
            cmd.arg("--tools").arg("");
        } else {
            // Keep Read tool available so Claude can view attached images
            cmd.arg("--allowedTools").arg("Read");
        }
        cmd.arg("--strict-mcp-config");
        cmd.arg("--disallowedTools").arg("LSP");
    }
    if let Some(config_path) = mcp_config {
        cmd.arg("--mcp-config").arg(config_path);
    }

    if let Some(sid) = session_id {
        cmd.arg("--resume").arg(sid);
    } else {
        cmd.arg("--system-prompt").arg(system_prompt);
    }

    let mut child = cmd.spawn()
        .map_err(|e| format!("Failed to run claude: {e}. Is claude CLI installed?"))?;

    // Store PID so the process can be killed on app exit or Ctrl+C
    if let Some(pid_arc) = pid_out {
        pid_arc.store(child.id(), std::sync::atomic::Ordering::SeqCst);
    }

    let stdout = child.stdout.take()
        .ok_or("Failed to capture claude stdout")?;

    // Read stream-json line by line
    use std::io::BufRead;
    let reader = std::io::BufReader::new(stdout);

    let mut full_text = String::new();
    let mut captured_session_id: Option<String> = None;
    let mut last_assistant_text = String::new();
    let mut had_tool_calls = false;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        if line.trim().is_empty() { continue; }

        let event: ClaudeJsonOutput = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => continue,
        };

        // Capture session_id from any event
        if captured_session_id.is_none() {
            if let Some(ref sid) = event.session_id {
                if !sid.is_empty() {
                    captured_session_id = Some(sid.clone());
                }
            }
        }

        // Extract text from assistant messages
        if event.event_type.as_deref() == Some("assistant") {
            if let Some(ref msg) = event.message {
                if let Some(content) = msg.get("content") {
                    if let Some(arr) = content.as_array() {
                        let mut new_text = String::new();
                        for block in arr {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                new_text.push_str(text);
                            }
                            // Parse tool_use content blocks from MCP
                            if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                                if let Some(tool_tx) = on_tool {
                                    let tc = crate::claude_bridge::ToolCall {
                                        name: block.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
                                        input: block.get("input").cloned().unwrap_or(serde_json::Value::Null),
                                    };
                                    let _ = tool_tx.send(tc);
                                    had_tool_calls = true;
                                }
                            }
                        }
                        // Send only the new delta
                        if new_text.len() > last_assistant_text.len() {
                            let delta = &new_text[last_assistant_text.len()..];
                            if let Some(tx) = on_text {
                                let _ = tx.send(delta.to_string());
                            }
                        }
                        last_assistant_text = new_text;
                    }
                }
            }
        }

        // Result event = final
        if event.event_type.as_deref() == Some("result") {
            if event.is_error == Some(true) {
                return Err(event.result.clone().unwrap_or("Unknown error".to_string()));
            }
            if let Some(ref result) = event.result {
                full_text = result.clone();
            }
        }
    }

    let _ = child.wait();

    // Clear PID after process exits
    if let Some(pid_arc) = pid_out {
        pid_arc.store(0, std::sync::atomic::Ordering::SeqCst);
    }

    if full_text.is_empty() && !last_assistant_text.is_empty() {
        full_text = last_assistant_text;
    }

    if full_text.is_empty() && !had_tool_calls {
        return Err("No response from claude".to_string());
    }

    Ok((full_text, captured_session_id))
}

/// Send a prompt to Claude with a phase-specific system prompt.
/// Unlike `send_prompt()` which takes a system prompt string,
/// this loads the prompt from a file via `prompt_builder::load_phase_system_prompt()`.
///
/// - `phase_name`: "spec", "decompose", "component", "assembly", or "refinement"
/// - `session_id`: if Some, uses --resume (ignores system prompt)
/// - Returns (response_text, captured_session_id)
pub fn send_with_phase_prompt(
    model: &Option<String>,
    phase_name: &str,
    session_id: Option<&str>,
    prompt: &str,
    image_paths: &[PathBuf],
    on_text: Option<&std::sync::mpsc::Sender<String>>,
    pid_out: Option<&std::sync::Arc<std::sync::atomic::AtomicU32>>,
    ref_context: Option<&str>,
    on_tool: Option<&std::sync::mpsc::Sender<super::claude_bridge::ToolCall>>,
    mcp_config: Option<&std::path::Path>,
    disable_builtin_tools: bool,
) -> Result<(String, Option<String>), String> {
    let mut system_prompt = crate::prompt_builder::load_phase_system_prompt(phase_name)?;

    // Inject engineering knowledge for build phases
    if matches!(phase_name, "build" | "component" | "assembly" | "refinement" | "refine" | "lead") {
        system_prompt.push_str(&crate::prompt_builder::load_engineering_knowledge());
    }

    if let Some(ctx) = ref_context {
        system_prompt.push_str("\n\n");
        system_prompt.push_str(ctx);
    }

    // If we have a session_id, try resuming first
    if let Some(sid) = session_id {
        match send_prompt(model, &system_prompt, Some(sid), prompt, image_paths, on_text, pid_out, on_tool, mcp_config, disable_builtin_tools) {
            Ok(result) => return Ok(result),
            Err(e) => {
                // Check for session expiry indicators
                let lower = e.to_lowercase();
                if (lower.contains("session") && (lower.contains("not found") || lower.contains("expired")))
                    || lower.contains("invalid session")
                {
                    // Retry without --resume (fresh session)
                    return send_prompt(model, &system_prompt, None, prompt, image_paths, on_text, pid_out, on_tool, mcp_config, disable_builtin_tools);
                }
                return Err(e);
            }
        }
    }

    send_prompt(model, &system_prompt, None, prompt, image_paths, on_text, pid_out, on_tool, mcp_config, disable_builtin_tools)
}

/// Check that the claude CLI is available.
pub fn check_claude() -> Result<(), String> {
    let output = Command::new("claude")
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|_| "claude CLI not found. Install from https://claude.ai/claude-code".to_string())?;

    if !output.status.success() {
        return Err("claude CLI not working properly".to_string());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_serialization() {
        let msg = Message { role: "user".to_string(), content: "make a box".to_string() };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"role\":\"user\""));
    }

    #[test]
    fn test_system_prompt_found() {
        let result = find_system_prompt();
        assert!(result.is_ok(), "prompts/legacy.md not found: {:?}", result);
    }

    #[test]
    fn test_send_prompt_signature() {
        // Verify send_prompt compiles with the correct signature.
        // We don't call it (would need a live claude instance) but verify it exists.
        let _: fn(
            &Option<String>, &str, Option<&str>, &str, &[PathBuf],
            Option<&std::sync::mpsc::Sender<String>>,
            Option<&std::sync::Arc<std::sync::atomic::AtomicU32>>,
            Option<&std::sync::mpsc::Sender<crate::claude_bridge::ToolCall>>,
            Option<&std::path::Path>,
            bool,
        ) -> Result<(String, Option<String>), String> = send_prompt;
    }

    #[test]
    fn test_send_with_phase_prompt_signature() {
        // Verify send_with_phase_prompt compiles with the correct signature.
        let _: fn(
            &Option<String>, &str, Option<&str>, &str, &[PathBuf],
            Option<&std::sync::mpsc::Sender<String>>,
            Option<&std::sync::Arc<std::sync::atomic::AtomicU32>>,
            Option<&str>,
            Option<&std::sync::mpsc::Sender<crate::claude_bridge::ToolCall>>,
            Option<&std::path::Path>,
            bool,
        ) -> Result<(String, Option<String>), String> = send_with_phase_prompt;
    }

    #[test]
    fn test_client_accessors() {
        // Verify session_id and set_session_id compile correctly.
        // ClaudeClient::new requires prompts/system.md, so only test accessor logic.
        struct MockClient { session_id: Option<String> }
        let mut c = MockClient { session_id: None };
        c.session_id = Some("abc".to_string());
        assert_eq!(c.session_id.as_deref(), Some("abc"));
        c.session_id = None;
        assert!(c.session_id.is_none());
    }
}
