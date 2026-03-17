use crate::claude;
use crate::tui::BackgroundResult;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

/// Whether a background task is running.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BusyState {
    Idle,
    Thinking,
    Building,
}

/// An MCP tool call emitted by Claude during streaming.
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub input: serde_json::Value,
}

/// Owns all Claude CLI interaction state: channels, PID tracking,
/// model selection, session continuity, and streaming text buffer.
pub struct ClaudeBridge {
    // Channels
    bg_tx: mpsc::Sender<BackgroundResult>,
    bg_rx: mpsc::Receiver<BackgroundResult>,
    stream_tx: mpsc::Sender<String>,
    stream_rx: mpsc::Receiver<String>,
    pub tool_tx: mpsc::Sender<ToolCall>,
    pub tool_rx: mpsc::Receiver<ToolCall>,
    bg_pid: Arc<AtomicU32>,

    // State
    pub model: Option<String>,
    pub session_id: Option<String>,
    pub streaming_text: String,
    pub busy: BusyState,
}

impl ClaudeBridge {
    /// Create a new bridge with channels and initial state.
    pub fn new(model: Option<String>) -> Self {
        let (bg_tx, bg_rx) = mpsc::channel::<BackgroundResult>();
        let (stream_tx, stream_rx) = mpsc::channel::<String>();
        let (tool_tx, tool_rx) = mpsc::channel::<ToolCall>();
        let bg_pid = Arc::new(AtomicU32::new(0));

        ClaudeBridge {
            bg_tx,
            bg_rx,
            stream_tx,
            stream_rx,
            tool_tx,
            tool_rx,
            bg_pid,
            model,
            session_id: None,
            streaming_text: String::new(),
            busy: BusyState::Idle,
        }
    }

    /// Drain stream_rx via try_recv loop, appending to streaming_text.
    /// Returns true if any chunks were received.
    pub fn drain_streaming(&mut self) -> bool {
        let mut got = false;
        while let Ok(chunk) = self.stream_rx.try_recv() {
            self.streaming_text.push_str(&chunk);
            got = true;
        }
        got
    }

    /// Drain tool_rx via try_recv loop, returning all pending tool calls.
    pub fn drain_tool_calls(&self) -> Vec<ToolCall> {
        let mut calls = Vec::new();
        while let Ok(tc) = self.tool_rx.try_recv() {
            calls.push(tc);
        }
        calls
    }

    /// Non-blocking check for a completed background result.
    pub fn try_recv_result(&self) -> Option<BackgroundResult> {
        self.bg_rx.try_recv().ok()
    }

    /// Send SIGTERM to the background Claude subprocess (if any).
    pub fn cancel(&self) {
        let pid = self.bg_pid.load(Ordering::SeqCst);
        if pid != 0 {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
    }

    /// Spawn a background thread that calls `claude::send_with_phase_prompt`
    /// and sends the result via bg_tx.
    ///
    /// This replaces the duplicated thread-spawn pattern across all send methods.
    /// When `mcp_config` is provided, MCP tool flags are passed to the CLI and
    /// tool_use content blocks are forwarded via `tool_tx`.
    pub fn send_phase_prompt(
        &mut self,
        phase_name: &str,
        prompt: &str,
        images: &[PathBuf],
        ref_context: Option<&str>,
        mcp_config: Option<PathBuf>,
    ) {
        self.busy = BusyState::Thinking;
        self.streaming_text.clear();

        let model = self.model.clone();
        let session_id = self.session_id.clone();
        let tx = self.bg_tx.clone();
        let stream_tx = self.stream_tx.clone();
        let tool_tx = self.tool_tx.clone();
        let bg_pid = Arc::clone(&self.bg_pid);
        let phase_name = phase_name.to_string();
        let prompt = prompt.to_string();
        let images = images.to_vec();
        let ref_context = ref_context.map(|s| s.to_string());
        let has_mcp = mcp_config.is_some();

        std::thread::spawn(move || {
            let result = claude::send_with_phase_prompt(
                &model,
                &phase_name,
                session_id.as_deref(),
                &prompt,
                &images,
                Some(&stream_tx),
                Some(&bg_pid),
                ref_context.as_deref(),
                if has_mcp { Some(&tool_tx) } else { None },
                mcp_config.as_deref(),
                has_mcp,
            );
            bg_pid.store(0, Ordering::SeqCst);
            match result {
                Ok((response, new_sid)) => {
                    let _ = tx.send(BackgroundResult::ClaudeResponse {
                        result: Ok(response),
                        session_id: new_sid.or(session_id),
                    });
                }
                Err(e) => {
                    let _ = tx.send(BackgroundResult::ClaudeResponse {
                        result: Err(e),
                        session_id: None,
                    });
                }
            }
        });
    }

    /// Spawn a background thread that calls `claude::send_prompt` directly
    /// (no phase system prompt). Used for `/ref` research.
    pub fn send_raw_prompt(
        &mut self,
        system_prompt: &str,
        prompt: &str,
        images: &[PathBuf],
        result_name: &str,
    ) {
        self.busy = BusyState::Thinking;
        self.streaming_text.clear();

        let model = self.model.clone();
        let tx = self.bg_tx.clone();
        let stream_tx = self.stream_tx.clone();
        let bg_pid = Arc::clone(&self.bg_pid);
        let system_prompt = system_prompt.to_string();
        let prompt = prompt.to_string();
        let images = images.to_vec();
        let result_name = result_name.to_string();

        std::thread::spawn(move || {
            let result = claude::send_prompt(
                &model,
                &system_prompt,
                None,
                &prompt,
                &images,
                Some(&stream_tx),
                Some(&bg_pid),
                None, // no tool_tx for raw research prompts
                None, // no MCP config
                false, // don't disable builtin tools
            );
            bg_pid.store(0, Ordering::SeqCst);
            let _ = tx.send(BackgroundResult::ReferenceResearch {
                name: result_name,
                result: result.map(|(response, _sid)| response),
            });
        });
    }
}

/// Generate an MCP config JSON file for the given phase and return its path.
/// The config points the Claude CLI at our MCP server with appropriate args.
pub fn generate_mcp_config(phase_name: &str, session_dir: Option<&Path>) -> Result<PathBuf, String> {
    let server_path = find_mcp_server()?;
    let python_cmd = find_cadquery_python(&server_path);
    let mut args = vec![
        server_path.to_string_lossy().to_string(),
        "--phase".to_string(),
        phase_name.to_string(),
    ];
    if let Some(dir) = session_dir {
        args.push("--session-dir".to_string());
        args.push(dir.to_string_lossy().to_string());
    }
    let config = serde_json::json!({
        "mcpServers": {
            "mimodel": {
                "command": python_cmd,
                "args": args
            }
        }
    });
    let tmp_path = std::env::temp_dir().join(format!("mimodel_mcp_{}.json", std::process::id()));
    std::fs::write(&tmp_path, config.to_string())
        .map_err(|e| format!("Failed to write MCP config: {e}"))?;
    Ok(tmp_path)
}

/// Locate the MCP server script (mcp/server.py).
/// Searches cwd, binary dir, and walks up from cwd.
fn find_mcp_server() -> Result<PathBuf, String> {
    let candidates = [
        std::env::current_dir().ok().map(|d| d.join("mcp/server.py")),
        std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("mcp/server.py"))),
    ];
    for c in candidates.into_iter().flatten() {
        if c.exists() { return Ok(c); }
    }
    // Walk up from cwd
    if let Ok(mut dir) = std::env::current_dir() {
        loop {
            let candidate = dir.join("mcp/server.py");
            if candidate.exists() { return Ok(candidate); }
            if !dir.pop() { break; }
        }
    }
    Err("mcp/server.py not found".to_string())
}

/// Find the Python interpreter that has CadQuery+OCP installed.
/// Looks for .venv-cadquery/bin/python3 relative to the project root
/// (same directory tree as mcp/server.py). Falls back to "python3".
fn find_cadquery_python(server_path: &Path) -> String {
    // server_path is like /path/to/project/mcp/server.py
    // project root is the parent of mcp/
    if let Some(project_root) = server_path.parent().and_then(|p| p.parent()) {
        let venv_python = project_root.join(".venv-cadquery/bin/python3");
        if venv_python.exists() {
            return venv_python.to_string_lossy().to_string();
        }
        // Also check .venv/bin/python3
        let venv_python = project_root.join(".venv/bin/python3");
        if venv_python.exists() {
            return venv_python.to_string_lossy().to_string();
        }
    }
    "python3".to_string()
}
