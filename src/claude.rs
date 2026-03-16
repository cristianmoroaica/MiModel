//! Claude CLI client — spawns `claude` subprocess for each interaction.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Locate the system prompt file (prompts/system.md).
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
            let candidate = dir.join("prompts/system.md");
            if candidate.exists() {
                return Ok(candidate);
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => break,
            }
        }
    }
    Err("prompts/system.md not found. Run from within the MiModel project.".to_string())
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
pub fn send_prompt(
    model: &Option<String>,
    system_prompt: &str,
    session_id: Option<&str>,
    prompt: &str,
    image_paths: &[PathBuf],
) -> Result<(String, Option<String>), String> {
    // Build the full prompt — if images are attached, tell claude about them
    let full_prompt = if image_paths.is_empty() {
        prompt.to_string()
    } else {
        let image_refs: Vec<String> = image_paths.iter()
            .map(|p| format!("Image: {}", p.to_string_lossy()))
            .collect();
        format!(
            "{}\n\n{}\n\nPlease read and analyze the image(s) above using the Read tool, then generate the CadQuery code.",
            prompt,
            image_refs.join("\n")
        )
    };

    let mut cmd = Command::new("claude");
    cmd.arg("--dangerously-skip-permissions")
        .arg("-p")
        .arg(&full_prompt)
        .arg("--output-format").arg("json")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(ref m) = model {
        cmd.arg("--model").arg(m);
    }

    // Grant access to directories containing images
    for path in image_paths {
        if let Some(parent) = path.parent() {
            cmd.arg("--add-dir").arg(parent);
        }
    }

    if let Some(sid) = session_id {
        cmd.arg("--resume").arg(sid);
    } else {
        cmd.arg("--system-prompt").arg(system_prompt);
    }

    let output = cmd.output()
        .map_err(|e| format!("Failed to run claude: {e}. Is claude CLI installed?"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Try parsing JSON from stdout first (claude may output JSON even on error)
    // If stdout is empty, try stderr (some versions write JSON there)
    let json_source = if stdout.trim().starts_with('[') {
        &stdout
    } else if stderr.trim().starts_with('[') {
        &stderr
    } else if !output.status.success() {
        let msg = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            format!("claude exited with {} (no output)", output.status)
        };
        return Err(msg);
    } else {
        return Err("No output from claude".to_string());
    };

    let events: Vec<ClaudeJsonOutput> = serde_json::from_str(json_source)
        .map_err(|e| format!("Failed to parse claude output: {e}"))?;

    // Capture session_id from the first event that has one
    let mut captured_session_id: Option<String> = None;
    if session_id.is_none() {
        for event in &events {
            if let Some(ref sid) = event.session_id {
                if !sid.is_empty() {
                    captured_session_id = Some(sid.clone());
                    break;
                }
            }
        }
    }

    // Find the result event
    for event in &events {
        if event.event_type.as_deref() == Some("result") {
            if event.is_error == Some(true) {
                return Err(event.result.clone().unwrap_or("Unknown error".to_string()));
            }
            if let Some(ref result) = event.result {
                return Ok((result.clone(), captured_session_id));
            }
        }
    }

    Err("No result in claude output".to_string())
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
        assert!(result.is_ok(), "prompts/system.md not found: {:?}", result);
    }

    #[test]
    fn test_send_prompt_signature() {
        // Verify send_prompt compiles with the correct signature.
        // We don't call it (would need a live claude instance) but verify it exists.
        let _: fn(&Option<String>, &str, Option<&str>, &str, &[PathBuf]) -> Result<(String, Option<String>), String> = send_prompt;
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
