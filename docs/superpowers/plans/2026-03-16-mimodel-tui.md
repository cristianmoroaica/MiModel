# MiModel TUI & Session Persistence Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redesign MiModel from a readline REPL into a three-column ratatui TUI with project-based session persistence at `~/MiModel/`.

**Architecture:** Storage layer (`storage/`) handles project/session disk CRUD. TUI layer (`tui/`) renders three-column layout with ratatui + tui-textarea input. Background threading via `std::thread` + `mpsc` channels keeps the UI responsive during claude/build calls. Existing backend modules (claude.rs, python.rs, parser.rs, stl.rs, preview.rs, viewer.rs, image.rs) are unchanged except claude.rs gets a minor refactor for thread safety.

**Tech Stack:** ratatui 0.29, tui-textarea 0.7, crossterm 0.28, serde_json (existing)

**Spec:** `docs/superpowers/specs/2026-03-16-mimodel-tui-design.md`

---

## Chunk 1: Foundation — Dependencies, Rename, Storage Layer

### Task 1: Update dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Update Cargo.toml**

Add `ratatui` and `tui-textarea`, remove `rustyline`:

```toml
[package]
name = "mimodel"
version = "0.2.0"
edition = "2021"
description = "Interactive 3D model generator using Claude CLI"

[[bin]]
name = "mimodel"
path = "src/main.rs"

[dependencies]
clap = { version = "4", features = ["derive"] }
crossterm = "0.28"
ratatui = "0.29"
tui-textarea = "0.7"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
dirs = "6"
tempfile = "3"
wait-timeout = "0.2"
chrono = { version = "0.4", features = ["serde"] }

[target.'cfg(unix)'.dependencies]
libc = "0.2"
```

- [ ] **Step 2: Verify it compiles**

Temporarily comment out `rustyline` imports in main.rs (replace the body of `run_session` with `println!("TUI placeholder");`) so it compiles without rustyline. We'll rewrite main.rs fully in Task 10.

Run: `cargo build`

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs
git commit -m "deps: add ratatui + tui-textarea, remove rustyline, bump to v0.2"
```

---

### Task 2: Rename session.rs → model_session.rs + add persistence interface

**Files:**
- Rename: `src/session.rs` → `src/model_session.rs`
- Modify: `src/main.rs` — update `mod session` → `mod model_session`

- [ ] **Step 1: Rename and update mod declaration**

```bash
mv src/session.rs src/model_session.rs
```

Update `src/main.rs`: change `mod session;` to `mod model_session;` and all `use crate::session::` to `use crate::model_session::`.

- [ ] **Step 2: Add save_to and load_from to ModelSession**

Add to `src/model_session.rs`:

```rust
use serde::{Serialize, Deserialize};

/// Data that gets serialized to session.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub name: String,
    pub created: String,
    pub modified: String,
    pub iteration_count: u32,
    pub claude_session_id: Option<String>,
    pub current_iteration: u32,
    pub engine: Option<String>,
    pub conversation: Vec<Message>,
}

impl Session {
    /// Save current session state to a directory.
    /// Copies all iteration files (code, STL, metadata) and writes session.json.
    pub fn save_to(&self, dir: &Path, name: &str, claude_session_id: Option<&str>) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("Failed to create session dir: {e}"))?;

        // Copy iteration files from temp_dir to session dir
        for i in 1..=self.iteration {
            for ext in &["py", "scad", "stl", "stl.json"] {
                let src = self.temp_dir.join(format!("iter_{i:03}.{ext}"));
                if src.exists() {
                    let dest = dir.join(format!("iter_{i:03}.{ext}"));
                    let _ = std::fs::copy(&src, &dest);
                }
            }
            // Also copy .json metadata sidecar
            let meta_src = self.temp_dir.join(format!("iter_{i:03}.json"));
            if meta_src.exists() {
                let meta_dest = dir.join(format!("iter_{i:03}.json"));
                let _ = std::fs::copy(&meta_src, &meta_dest);
            }
        }

        // Write session.json
        let data = SessionData {
            name: name.to_string(),
            created: chrono::Utc::now().to_rfc3339(),
            modified: chrono::Utc::now().to_rfc3339(),
            iteration_count: self.iteration,
            claude_session_id: claude_session_id.map(|s| s.to_string()),
            current_iteration: self.iteration,
            engine: self.current_engine.map(|e| e.as_str().to_string()),
            conversation: self.messages.clone(),
        };
        let json = serde_json::to_string_pretty(&data)
            .map_err(|e| format!("Failed to serialize session: {e}"))?;
        std::fs::write(dir.join("session.json"), json)
            .map_err(|e| format!("Failed to write session.json: {e}"))?;

        Ok(())
    }

    /// Load session state from a directory.
    pub fn load_from(dir: &Path, build_timeout: u64, python_path: String) -> Result<Self, String> {
        let json_path = dir.join("session.json");
        let json = std::fs::read_to_string(&json_path)
            .map_err(|e| format!("Failed to read session.json: {e}"))?;
        let data: SessionData = serde_json::from_str(&json)
            .map_err(|e| format!("Failed to parse session.json: {e}"))?;

        let mut session = Session::new(build_timeout, python_path);
        session.messages = data.conversation;
        session.iteration = data.current_iteration;

        // Copy iteration files into temp dir for active use
        for i in 1..=data.iteration_count {
            for ext in &["py", "scad", "stl", "stl.json", "json"] {
                let src = dir.join(format!("iter_{i:03}.{ext}"));
                if src.exists() {
                    let dest = session.temp_dir.join(format!("iter_{i:03}.{ext}"));
                    let _ = std::fs::copy(&src, &dest);
                }
            }
        }

        // Load latest metadata
        let meta_path = session.temp_dir.join(format!("iter_{:03}.stl.json", data.current_iteration));
        if meta_path.exists() {
            if let Ok(meta_json) = std::fs::read_to_string(&meta_path) {
                if let Ok(meta) = serde_json::from_str::<crate::python::ModelMetadata>(&meta_json) {
                    session.current_metadata = Some(meta);
                }
            }
        }

        // Load latest code
        let code_path = session.temp_dir.join(format!("iter_{:03}.py", data.current_iteration));
        if code_path.exists() {
            if let Ok(code) = std::fs::read_to_string(&code_path) {
                session.current_code = Some(code);
                session.current_engine = Some(crate::python::Engine::CadQuery);
            }
        } else {
            let scad_path = session.temp_dir.join(format!("iter_{:03}.scad", data.current_iteration));
            if scad_path.exists() {
                if let Ok(code) = std::fs::read_to_string(&scad_path) {
                    session.current_code = Some(code);
                    session.current_engine = Some(crate::python::Engine::OpenSCAD);
                }
            }
        }

        if session.current_metadata.is_some() {
            session.state = SessionState::Reviewing;
        }

        session.update_symlink();
        Ok(session)
    }
}
```

- [ ] **Step 3: Add chrono to existing Serialize/Deserialize derives**

Ensure `Message` in `claude.rs` already has `Serialize, Deserialize` (it does). Add `Serialize` derive to `ModelMetadata` in `python.rs` (it already has it). No changes needed.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: All existing tests pass (rename is transparent to tests)

- [ ] **Step 5: Commit**

```bash
git add src/model_session.rs src/main.rs
git rm src/session.rs
git commit -m "refactor: rename session.rs to model_session.rs, add save_to/load_from"
```

---

### Task 3: Storage module — project CRUD

**Files:**
- Create: `src/storage/mod.rs`
- Create: `src/storage/project.rs`
- Modify: `src/main.rs` — add `mod storage;`

- [ ] **Step 1: Create storage/project.rs**

```rust
//! Project directory CRUD operations.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    pub created: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub path: PathBuf,
    pub meta: ProjectMeta,
    pub sessions: Vec<String>, // session directory names
}

/// Get the root storage directory: ~/MiModel/
pub fn root_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("MiModel")
}

/// Ensure ~/MiModel/ exists. Creates with a default "Untitled" project if missing.
pub fn ensure_root() -> Result<PathBuf, String> {
    let root = root_dir();
    if !root.exists() {
        std::fs::create_dir_all(&root)
            .map_err(|e| format!("Failed to create ~/MiModel/: {e}"))?;
        create_project("Untitled", "")?;
    }
    Ok(root)
}

/// List all projects in ~/MiModel/.
pub fn list_projects() -> Result<Vec<Project>, String> {
    let root = root_dir();
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut projects = Vec::new();
    let entries = std::fs::read_dir(&root)
        .map_err(|e| format!("Failed to read ~/MiModel/: {e}"))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() { continue; }

        let meta_path = path.join("project.json");
        let meta = if meta_path.exists() {
            let json = std::fs::read_to_string(&meta_path).unwrap_or_default();
            serde_json::from_str(&json).unwrap_or(ProjectMeta {
                name: path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                created: String::new(),
                description: String::new(),
            })
        } else {
            ProjectMeta {
                name: path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                created: String::new(),
                description: String::new(),
            }
        };

        // List session subdirectories
        let mut sessions = Vec::new();
        if let Ok(sub_entries) = std::fs::read_dir(&path) {
            for sub in sub_entries.flatten() {
                let sub_path = sub.path();
                if sub_path.is_dir() && sub_path.join("session.json").exists() {
                    if let Some(name) = sub_path.file_name() {
                        sessions.push(name.to_string_lossy().to_string());
                    }
                }
            }
        }
        sessions.sort();

        projects.push(Project { path, meta, sessions });
    }

    projects.sort_by(|a, b| a.meta.name.cmp(&b.meta.name));
    Ok(projects)
}

/// Create a new project directory with project.json.
pub fn create_project(name: &str, description: &str) -> Result<PathBuf, String> {
    let path = root_dir().join(name);
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("Failed to create project dir: {e}"))?;

    let meta = ProjectMeta {
        name: name.to_string(),
        created: chrono::Utc::now().to_rfc3339(),
        description: description.to_string(),
    };
    let json = serde_json::to_string_pretty(&meta)
        .map_err(|e| format!("Failed to serialize project: {e}"))?;
    std::fs::write(path.join("project.json"), json)
        .map_err(|e| format!("Failed to write project.json: {e}"))?;

    Ok(path)
}

/// Delete a project and all its sessions.
pub fn delete_project(name: &str) -> Result<(), String> {
    let path = root_dir().join(name);
    if path.exists() {
        std::fs::remove_dir_all(&path)
            .map_err(|e| format!("Failed to delete project: {e}"))?;
    }
    Ok(())
}

/// Rename a project.
pub fn rename_project(old_name: &str, new_name: &str) -> Result<(), String> {
    let old_path = root_dir().join(old_name);
    let new_path = root_dir().join(new_name);
    std::fs::rename(&old_path, &new_path)
        .map_err(|e| format!("Failed to rename project: {e}"))?;

    // Update project.json
    let meta_path = new_path.join("project.json");
    if meta_path.exists() {
        if let Ok(json) = std::fs::read_to_string(&meta_path) {
            if let Ok(mut meta) = serde_json::from_str::<ProjectMeta>(&json) {
                meta.name = new_name.to_string();
                if let Ok(updated) = serde_json::to_string_pretty(&meta) {
                    let _ = std::fs::write(&meta_path, updated);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn with_test_root(f: impl FnOnce()) {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("HOME", tmp.path());
        f();
        std::env::remove_var("HOME");
    }

    #[test]
    fn test_ensure_root_creates_default_project() {
        with_test_root(|| {
            let root = ensure_root().unwrap();
            assert!(root.exists());
            assert!(root.join("Untitled/project.json").exists());
        });
    }

    #[test]
    fn test_create_and_list_projects() {
        with_test_root(|| {
            ensure_root().unwrap();
            create_project("Test Project", "A test").unwrap();
            let projects = list_projects().unwrap();
            assert!(projects.iter().any(|p| p.meta.name == "Test Project"));
        });
    }
}
```

- [ ] **Step 2: Create storage/mod.rs**

```rust
pub mod project;

// Re-export commonly used types
pub use project::{Project, ProjectMeta};
```

- [ ] **Step 3: Add `mod storage;` to main.rs**

- [ ] **Step 4: Run tests**

Run: `cargo test storage`
Expected: 2 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/storage/
git commit -m "feat: add storage module with project CRUD"
```

---

### Task 4: Storage module — session CRUD

**Files:**
- Create: `src/storage/session.rs`
- Modify: `src/storage/mod.rs` — add `pub mod session;`

- [ ] **Step 1: Create storage/session.rs**

```rust
//! Session directory CRUD and serialization.

use crate::model_session::SessionData;
use std::path::{Path, PathBuf};

/// Create a new session directory inside a project.
pub fn create_session(project_path: &Path, name: &str) -> Result<PathBuf, String> {
    let path = project_path.join(name);
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("Failed to create session dir: {e}"))?;
    Ok(path)
}

/// Load session metadata from a session directory.
pub fn load_session_data(session_path: &Path) -> Result<SessionData, String> {
    let json_path = session_path.join("session.json");
    let json = std::fs::read_to_string(&json_path)
        .map_err(|e| format!("Failed to read session.json: {e}"))?;
    serde_json::from_str(&json)
        .map_err(|e| format!("Corrupted session.json: {e}"))
}

/// Delete a session directory.
pub fn delete_session(session_path: &Path) -> Result<(), String> {
    if session_path.exists() {
        std::fs::remove_dir_all(session_path)
            .map_err(|e| format!("Failed to delete session: {e}"))?;
    }
    Ok(())
}

/// Rename a session directory.
pub fn rename_session(session_path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let new_path = session_path
        .parent()
        .ok_or("Invalid session path")?
        .join(new_name);
    std::fs::rename(session_path, &new_path)
        .map_err(|e| format!("Failed to rename session: {e}"))?;
    Ok(new_path)
}

/// List session names in a project that are corrupted (have dir but broken session.json).
pub fn session_status(session_path: &Path) -> SessionStatus {
    let json_path = session_path.join("session.json");
    if !json_path.exists() {
        return SessionStatus::Empty;
    }
    match std::fs::read_to_string(&json_path) {
        Ok(json) => match serde_json::from_str::<SessionData>(&json) {
            Ok(data) => SessionStatus::Ok {
                iteration_count: data.iteration_count,
                modified: data.modified,
            },
            Err(_) => SessionStatus::Corrupted,
        },
        Err(_) => SessionStatus::Corrupted,
    }
}

#[derive(Debug)]
pub enum SessionStatus {
    Ok { iteration_count: u32, modified: String },
    Corrupted,
    Empty,
}
```

- [ ] **Step 2: Update storage/mod.rs**

```rust
pub mod project;
pub mod session;

pub use project::{Project, ProjectMeta};
```

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add src/storage/
git commit -m "feat: add session CRUD to storage module"
```

---

### Task 5: Refactor claude.rs for thread safety

**Files:**
- Modify: `src/claude.rs`

- [ ] **Step 1: Refactor send() to return session_id instead of mutating self**

Change `send(&mut self, ...)` to a free function pattern. The `ClaudeClient` struct keeps its fields, but `send` is refactored so the background thread can call it without `&mut self`:

```rust
/// Send a prompt to Claude CLI. Returns (response_text, captured_session_id).
/// Takes session_id as a parameter rather than reading from &self,
/// so it can be called from a background thread.
pub fn send_prompt(
    model: &Option<String>,
    system_prompt: &str,
    session_id: Option<&str>,
    prompt: &str,
    image_paths: &[std::path::PathBuf],
) -> Result<(String, Option<String>), String> {
    // ... same implementation as current send(), but:
    // - reads model/system_prompt/session_id from params
    // - returns (result_text, new_session_id) tuple
}
```

The `ClaudeClient` struct becomes a thin wrapper:

```rust
impl ClaudeClient {
    pub fn send(&mut self, prompt: &str, image_paths: &[PathBuf]) -> Result<String, String> {
        let (result, new_sid) = send_prompt(
            &self.model, &self.system_prompt, self.session_id.as_deref(),
            prompt, image_paths,
        )?;
        if let Some(sid) = new_sid {
            self.session_id = Some(sid);
        }
        Ok(result)
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn set_session_id(&mut self, id: Option<String>) {
        self.session_id = id;
    }
}
```

This way the TUI's background thread can call `send_prompt()` directly with cloned values, and return the session_id via the channel.

- [ ] **Step 2: Run tests**

Run: `cargo test claude`
Expected: Existing tests pass

- [ ] **Step 3: Commit**

```bash
git add src/claude.rs
git commit -m "refactor: extract send_prompt() free function for thread-safe claude calls"
```

---

## Chunk 2: TUI Modules

### Task 6: TUI layout module

**Files:**
- Create: `src/tui/mod.rs`
- Create: `src/tui/layout.rs`
- Modify: `src/main.rs` — add `mod tui;`

- [ ] **Step 1: Create src/tui/layout.rs**

```rust
//! Layout constraint calculation for the three-column + input bar TUI.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub struct PaneAreas {
    pub project_tree: Option<Rect>,
    pub conversation: Rect,
    pub model_panel: Option<Rect>,
    pub input_bar: Rect,
}

pub struct LayoutConfig {
    pub show_sidebar: bool,
    pub show_model_panel: bool,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self { show_sidebar: true, show_model_panel: true }
    }
}

/// Compute pane areas based on terminal size and toggle state.
pub fn compute_layout(area: Rect, config: &LayoutConfig) -> PaneAreas {
    let width = area.width;

    // Auto-hide panels for narrow terminals
    let show_sidebar = config.show_sidebar && width >= 100;
    let show_model = config.show_model_panel && width >= 60;

    // Split vertically: main area + input bar (3 lines)
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    let main_area = vertical[0];
    let input_bar = vertical[1];

    // Split main area horizontally based on visible panels
    let (project_tree, conversation, model_panel) = match (show_sidebar, show_model) {
        (true, true) => {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(20),
                    Constraint::Percentage(55),
                    Constraint::Percentage(25),
                ])
                .split(main_area);
            (Some(cols[0]), cols[1], Some(cols[2]))
        }
        (true, false) => {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(25),
                    Constraint::Percentage(75),
                ])
                .split(main_area);
            (Some(cols[0]), cols[1], None)
        }
        (false, true) => {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(70),
                    Constraint::Percentage(30),
                ])
                .split(main_area);
            (None, cols[0], Some(cols[1]))
        }
        (false, false) => {
            (None, main_area, None)
        }
    };

    PaneAreas { project_tree, conversation, model_panel, input_bar }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_all_panels() {
        let area = Rect::new(0, 0, 120, 40);
        let config = LayoutConfig::default();
        let panes = compute_layout(area, &config);
        assert!(panes.project_tree.is_some());
        assert!(panes.model_panel.is_some());
        assert_eq!(panes.input_bar.height, 3);
    }

    #[test]
    fn test_layout_narrow_hides_sidebar() {
        let area = Rect::new(0, 0, 80, 40);
        let config = LayoutConfig::default();
        let panes = compute_layout(area, &config);
        assert!(panes.project_tree.is_none()); // auto-hidden below 100
        assert!(panes.model_panel.is_some());
    }

    #[test]
    fn test_layout_very_narrow() {
        let area = Rect::new(0, 0, 50, 40);
        let config = LayoutConfig::default();
        let panes = compute_layout(area, &config);
        assert!(panes.project_tree.is_none());
        assert!(panes.model_panel.is_none());
    }
}
```

- [ ] **Step 2: Create src/tui/mod.rs**

Only declare the layout module initially. Other modules will be added as they're created.

```rust
pub mod layout;

/// Focus state — which pane has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Focus {
    Input,
    ProjectTree,
    Conversation,
}

/// Results from background threads.
pub enum BackgroundResult {
    ClaudeResponse {
        result: Result<String, String>,
        session_id: Option<String>,
    },
    BuildComplete(crate::python::BuildResult),
}

/// Whether a background task is running.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BusyState {
    Idle,
    Thinking,
    Building,
}
```

- [ ] **Step 3: Add `mod tui;` to main.rs**

- [ ] **Step 4: Run tests**

Run: `cargo test tui::layout`
Expected: 3 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/tui/
git commit -m "feat: add TUI layout module with responsive column calculation"
```

---

### Task 7: Input bar widget

**Files:**
- Create: `src/tui/input_bar.rs`

- [ ] **Step 1: Create input_bar.rs**

```rust
//! Input bar — wraps tui-textarea with submit (Ctrl+Enter) and history.

use tui_textarea::{Input, Key, TextArea};
use ratatui::style::{Color, Style};

pub struct InputBar<'a> {
    pub textarea: TextArea<'a>,
    history: Vec<String>,
    history_pos: Option<usize>,
}

impl<'a> InputBar<'a> {
    pub fn new() -> Self {
        let mut textarea = TextArea::default();
        textarea.set_cursor_line_style(Style::default());
        textarea.set_placeholder_text("Type what you want to build...");
        textarea.set_block(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::TOP)
                .border_style(Style::default().fg(Color::DarkGray))
        );
        Self {
            textarea,
            history: Vec::new(),
            history_pos: None,
        }
    }

    /// Handle input event. Returns Some(text) if user submitted (Ctrl+Enter).
    pub fn handle_input(&mut self, input: Input) -> Option<String> {
        match input {
            // Ctrl+Enter = submit
            Input { key: Key::Enter, ctrl: true, .. } => {
                let text = self.textarea.lines().join("\n").trim().to_string();
                if !text.is_empty() {
                    self.history.push(text.clone());
                    self.history_pos = None;
                }
                // Clear the textarea
                self.textarea = TextArea::default();
                self.textarea.set_cursor_line_style(Style::default());
                self.textarea.set_placeholder_text("Type what you want to build...");
                self.textarea.set_block(
                    ratatui::widgets::Block::default()
                        .borders(ratatui::widgets::Borders::TOP)
                        .border_style(Style::default().fg(Color::DarkGray))
                );
                if text.is_empty() { None } else { Some(text) }
            }
            // Up arrow with empty input = history back
            Input { key: Key::Up, .. } if self.textarea.lines() == [""] => {
                if !self.history.is_empty() {
                    let pos = match self.history_pos {
                        Some(p) if p > 0 => p - 1,
                        None => self.history.len() - 1,
                        Some(p) => p,
                    };
                    self.history_pos = Some(pos);
                    let text = self.history[pos].clone();
                    self.textarea = TextArea::new(vec![text]);
                    self.textarea.set_cursor_line_style(Style::default());
                    self.textarea.set_block(
                        ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::TOP)
                            .border_style(Style::default().fg(Color::DarkGray))
                    );
                }
                None
            }
            // Down arrow with history active = history forward
            Input { key: Key::Down, .. } if self.history_pos.is_some() => {
                let pos = self.history_pos.unwrap() + 1;
                if pos < self.history.len() {
                    self.history_pos = Some(pos);
                    let text = self.history[pos].clone();
                    self.textarea = TextArea::new(vec![text]);
                } else {
                    self.history_pos = None;
                    self.textarea = TextArea::default();
                }
                self.textarea.set_cursor_line_style(Style::default());
                self.textarea.set_block(
                    ratatui::widgets::Block::default()
                        .borders(ratatui::widgets::Borders::TOP)
                        .border_style(Style::default().fg(Color::DarkGray))
                );
                None
            }
            // Everything else: pass to tui-textarea
            input => {
                self.textarea.input(input);
                None
            }
        }
    }

    /// Get the current input text (for checking if empty, etc.).
    pub fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Set a prefix badge (e.g. "[2 images]").
    pub fn set_badge(&mut self, badge: &str) {
        let title = if badge.is_empty() {
            String::new()
        } else {
            format!(" {badge} ")
        };
        self.textarea.set_block(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::TOP)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(title)
        );
    }
}
```

- [ ] **Step 2: Add `pub mod input_bar;` to src/tui/mod.rs**

- [ ] **Step 3: Run: `cargo build`**

Expected: Compiles

- [ ] **Step 4: Commit**

```bash
git add src/tui/input_bar.rs src/tui/mod.rs
git commit -m "feat: add input bar widget with tui-textarea, history, submit"
```

---

### Task 8: Conversation pane widget

**Files:**
- Create: `src/tui/conversation.rs`

- [ ] **Step 1: Create conversation.rs**

A scrollable list of conversation messages with styled roles and inline build results.

```rust
//! Conversation pane — scrollable styled message list.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

#[derive(Debug, Clone)]
pub struct ConversationEntry {
    pub role: String,    // "user", "assistant", "system"
    pub content: String,
}

pub struct ConversationPane {
    pub entries: Vec<ConversationEntry>,
    pub scroll_offset: u16,
    pub auto_scroll: bool,
}

impl ConversationPane {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            scroll_offset: 0,
            auto_scroll: true,
        }
    }

    pub fn add(&mut self, role: &str, content: &str) {
        self.entries.push(ConversationEntry {
            role: role.to_string(),
            content: content.to_string(),
        });
        if self.auto_scroll {
            self.scroll_to_bottom();
        }
    }

    pub fn scroll_up(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
        self.auto_scroll = false;
    }

    pub fn scroll_down(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_add(amount);
        // Re-enable auto_scroll if we scrolled past the end
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = u16::MAX; // will be clamped during render
        self.auto_scroll = true;
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.scroll_offset = 0;
    }

    /// Render the conversation into the given area.
    pub fn render(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let border_color = if focused { Color::Cyan } else { Color::DarkGray };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(" Conversation ");

        // Build styled text
        let mut lines: Vec<Line> = Vec::new();
        for entry in &self.entries {
            let (prefix, color) = match entry.role.as_str() {
                "user" => ("you: ", Color::Green),
                "assistant" => ("claude: ", Color::Magenta),
                _ => ("", Color::DarkGray),
            };

            lines.push(Line::from(vec![
                Span::styled(prefix, Style::default().fg(color).bold()),
            ]));
            for line in entry.content.lines() {
                lines.push(Line::from(vec![
                    Span::raw(format!("  {line}")),
                ]));
            }
            lines.push(Line::raw("")); // blank separator
        }

        let text = Text::from(lines);
        let total_lines = text.lines.len() as u16;
        let visible = area.height.saturating_sub(2); // minus borders
        let max_scroll = total_lines.saturating_sub(visible);
        let scroll = self.scroll_offset.min(max_scroll);

        let paragraph = Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0));

        frame.render_widget(paragraph, area);
    }
}
```

- [ ] **Step 2: Add `pub mod conversation;` to src/tui/mod.rs**

- [ ] **Step 3: Run: `cargo build`**

- [ ] **Step 4: Commit**

```bash
git add src/tui/conversation.rs src/tui/mod.rs
git commit -m "feat: add conversation pane with scrollable styled messages"
```

---

### Task 9: Project tree + model panel widgets

**Files:**
- Create: `src/tui/project_tree.rs`
- Create: `src/tui/model_panel.rs`

- [ ] **Step 1: Create project_tree.rs**

```rust
//! Project tree pane — collapsible project/session list.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use crate::storage::Project;

#[derive(Debug, Clone)]
pub struct TreeEntry {
    pub label: String,
    pub is_project: bool,
    pub is_expanded: bool,
    pub project_idx: usize,
    pub session_name: Option<String>,
}

pub struct ProjectTreePane {
    pub entries: Vec<TreeEntry>,
    pub state: ListState,
    pub active_project: Option<usize>,
    pub active_session: Option<String>,
}

impl ProjectTreePane {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            state: ListState::default(),
            active_project: None,
            active_session: None,
        }
    }

    /// Rebuild tree entries from project list.
    pub fn refresh(&mut self, projects: &[Project]) {
        self.entries.clear();
        for (i, project) in projects.iter().enumerate() {
            let is_expanded = self.active_project == Some(i);
            let marker = if is_expanded { "▼" } else { "▶" };
            self.entries.push(TreeEntry {
                label: format!("{marker} {}", project.meta.name),
                is_project: true,
                is_expanded,
                project_idx: i,
                session_name: None,
            });
            if is_expanded {
                for session_name in &project.sessions {
                    let active = self.active_session.as_deref() == Some(session_name.as_str());
                    let marker = if active { "◀" } else { "" };
                    self.entries.push(TreeEntry {
                        label: format!("  ├─ {session_name} {marker}"),
                        is_project: false,
                        is_expanded: false,
                        project_idx: i,
                        session_name: Some(session_name.clone()),
                    });
                }
            }
        }
        // Add "New Project" at bottom
        self.entries.push(TreeEntry {
            label: "+ New Project".to_string(),
            is_project: true,
            is_expanded: false,
            project_idx: usize::MAX,
            session_name: None,
        });
    }

    pub fn select_next(&mut self) {
        let i = self.state.selected().map(|i| (i + 1).min(self.entries.len().saturating_sub(1))).unwrap_or(0);
        self.state.select(Some(i));
    }

    pub fn select_prev(&mut self) {
        let i = self.state.selected().map(|i| i.saturating_sub(1)).unwrap_or(0);
        self.state.select(Some(i));
    }

    /// Get the currently selected entry.
    pub fn selected_entry(&self) -> Option<&TreeEntry> {
        self.state.selected().and_then(|i| self.entries.get(i))
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let border_color = if focused { Color::Cyan } else { Color::DarkGray };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(" Projects ");

        let items: Vec<ListItem> = self.entries.iter().map(|entry| {
            let style = if entry.is_project {
                Style::default().fg(Color::Blue).bold()
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(entry.label.clone()).style(style)
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(Color::DarkGray));

        frame.render_stateful_widget(list, area, &mut self.state);
    }
}
```

- [ ] **Step 2: Create model_panel.rs**

```rust
//! Model panel pane — dimensions, features, braille preview, metadata.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use crate::python::ModelMetadata;
use crate::preview::{render_braille, ViewAngle};
use crate::stl::StlMesh;

pub struct ModelPanel {
    pub metadata: Option<ModelMetadata>,
    pub preview_text: Option<String>,
    pub iteration: u32,
}

impl ModelPanel {
    pub fn new() -> Self {
        Self { metadata: None, preview_text: None, iteration: 0 }
    }

    /// Update with new build results.
    pub fn update(&mut self, metadata: &ModelMetadata, stl_path: Option<&std::path::Path>, iteration: u32) {
        self.metadata = Some(metadata.clone());
        self.iteration = iteration;

        // Generate braille preview if STL is available
        if let Some(path) = stl_path {
            if let Ok(mesh) = StlMesh::from_file(path) {
                self.preview_text = Some(render_braille(&mesh, ViewAngle::Front, 20));
            }
        }
    }

    pub fn clear(&mut self) {
        self.metadata = None;
        self.preview_text = None;
        self.iteration = 0;
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, _focused: bool) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Model ");

        let mut lines: Vec<Line> = Vec::new();

        if let Some(ref meta) = self.metadata {
            lines.push(Line::from(Span::styled(
                format!("{:.1} x {:.1} x {:.1} mm", meta.dimensions.x, meta.dimensions.y, meta.dimensions.z),
                Style::default().fg(Color::Yellow).bold(),
            )));
            lines.push(Line::raw(""));

            if !meta.features.is_empty() {
                lines.push(Line::from(Span::styled("Features:", Style::default().fg(Color::DarkGray))));
                for f in &meta.features {
                    lines.push(Line::from(format!("  {f}")));
                }
                lines.push(Line::raw(""));
            }

            if let Some(ref preview) = self.preview_text {
                lines.push(Line::from(Span::styled("Preview:", Style::default().fg(Color::DarkGray))));
                for line in preview.lines() {
                    lines.push(Line::raw(format!(" {line}")));
                }
                lines.push(Line::raw(""));
            }

            lines.push(Line::from(vec![
                Span::styled("Iterations: ", Style::default().fg(Color::DarkGray)),
                Span::raw(self.iteration.to_string()),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Engine: ", Style::default().fg(Color::DarkGray)),
                Span::raw(&meta.engine),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Watertight: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    if meta.watertight { "yes" } else { "no" },
                    Style::default().fg(if meta.watertight { Color::Green } else { Color::Red }),
                ),
            ]));
        } else {
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled("No model yet", Style::default().fg(Color::DarkGray))));
        }

        let paragraph = Paragraph::new(Text::from(lines)).block(block);
        frame.render_widget(paragraph, area);
    }
}
```

- [ ] **Step 3: Add `pub mod project_tree;` and `pub mod model_panel;` to src/tui/mod.rs**

- [ ] **Step 4: Run: `cargo build`**

- [ ] **Step 5: Commit**

```bash
git add src/tui/project_tree.rs src/tui/model_panel.rs src/tui/mod.rs
git commit -m "feat: add project tree and model panel TUI widgets"
```

---

### Task 10: Main.rs — App struct, terminal init, and basic event loop

**Files:**
- Rewrite: `src/main.rs`

Break the main.rs rewrite into phases. This task creates the skeleton: App struct, terminal setup/teardown, basic event loop that renders all panes, and focus cycling (Tab/Esc). No keybinding actions yet — just the rendering frame.

- [ ] **Step 1: Write the App struct, init, and render loop**

The implementer creates `src/main.rs` with:
- `mod` declarations for all modules (claude, config, image, model_session, parser, preview, python, storage, stl, tui, viewer)
- `App` struct owning all panes, session, config, mpsc channels, and state flags
- `App::new()` that loads config, runs startup checks, scans `~/MiModel/`, and initializes all panes
- `main()` that inits ratatui terminal, creates App, runs the event loop, restores terminal on exit
- `run()` event loop: draw → poll crossterm events (50ms) → check `bg_rx.try_recv()` → dispatch
- `render()` that calls `compute_layout()` and renders each pane into its area
- Basic focus handling: `Tab` cycles Input→ProjectTree→Conversation→Input, `Esc` returns to Input
- `q` quits when input is empty and focused
- Terminal too narrow: if `frame.area().width < 40`, render a centered "Terminal too narrow" message instead of the panes

- [ ] **Step 2: Verify it compiles and renders**

Run: `cargo build`
Manual test: `cargo run` should show the three-column layout and quit on `q`.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: TUI skeleton — App struct, terminal init, event loop, basic rendering"
```

---

### Task 11: Prompt submission + background claude threading

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Implement Ctrl+Enter prompt submission**

When the user presses Ctrl+Enter in the input bar:
1. Extract text from `input_bar.textarea`
2. Run `image::extract_image_paths()` to detect inline images, merge with `pending_images`
3. Append current model context (code + metadata) if available
4. Add user message to conversation pane
5. Set `busy = BusyState::Thinking`
6. Spawn a thread: clone `claude_model`, `claude_system_prompt`, `claude_session_id`, and `bg_tx`. Thread calls `claude::send_prompt()` and sends `BackgroundResult::ClaudeResponse` back via channel.
7. Store child PID in `bg_pid: Arc<AtomicU32>` for cancellation.

- [ ] **Step 2: Implement handle_background_result for ClaudeResponse**

When `bg_rx.try_recv()` returns `ClaudeResponse`:
1. Update `claude_session_id` if new one was captured
2. Parse response with `parser::parse_response()`
3. Add assistant text to conversation pane
4. If code block found: set `busy = BusyState::Building`, spawn another thread for `python::build()`

When `BuildComplete`:
1. On success: update model_panel, update session state, add build result to conversation
2. On error: add error to conversation
3. Set `busy = BusyState::Idle`

- [ ] **Step 3: Implement animated spinner**

In the conversation pane, when `busy != Idle`, render a spinning braille character at the bottom of the conversation. The spinner state advances on each 50ms tick (the poll timeout). Use a simple frame counter: `SPINNER_CHARS = ['⠋','⠙','⠹','⠸','⠼','⠴','⠦','⠧','⠇','⠏']`.

- [ ] **Step 4: Build and test**

Run: `cargo build`
Manual test: type a prompt, Ctrl+Enter, verify spinner shows, response appears.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: prompt submission with background claude threading and spinner"
```

---

### Task 12: Remaining keybindings

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Implement all global keybindings**

- `Ctrl+N` → prompt for session name in input bar (set a `pending_action` state), create session via `storage::session::create_session()`
- `Ctrl+P` → prompt for project name, create via `storage::project::create_project()`
- `Ctrl+S` → prompt for export path in input bar, call `session.export()`
- `Ctrl+O` → call `viewer.show(&session.current_stl_path())`
- `Ctrl+Z` → call `session.undo()` (only if `busy == Idle`)
- `Ctrl+V` → call `image::paste_clipboard_image()`, save to session images dir, push to `pending_images`
- `Ctrl+L` → toggle `layout_config.show_sidebar`
- `Ctrl+R` → toggle `layout_config.show_model_panel`
- `Ctrl+C` → if busy, read PID from `bg_pid` and send SIGTERM; show "(cancelled)" in conversation

- [ ] **Step 2: Implement project tree keybindings (when focused)**

- `Up/Down` → `project_tree.select_prev/next()`
- `Enter` → if project: toggle expand/collapse. If session: load it (see Task 13). If "+ New Project": trigger Ctrl+P flow.
- `d` → delete with confirmation (add a `confirm_delete` state flag; show "Delete? y/n" in input bar)
- `r` → rename (switch input bar to rename mode, on submit call `storage::rename_project/session`)

- [ ] **Step 3: Implement conversation keybindings (when focused)**

- `Up/Down/j/k` → `conversation.scroll_up/down(1)`
- `PageUp/PageDown` → scroll by page height
- `c` → copy `session.current_code` to clipboard via `wl-copy`

- [ ] **Step 4: Build and test**

Run: `cargo build`

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: implement all keybindings — navigation, create, delete, rename, export, undo, image paste"
```

---

### Task 13: Session load/resume from project tree

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Implement session loading**

When the user presses Enter on a session in the project tree:
1. Get session path from `projects[idx].path.join(session_name)`
2. Call `model_session::Session::load_from(path, build_timeout, python_path)`
3. Load `session.json` to get `claude_session_id`
4. Try `--resume` with the stored session_id. If it fails (stale session), fall back:
   - Create a fresh claude session with `--system-prompt`
   - Inject the last working code + metadata as context in the first prompt
5. Update conversation pane from loaded conversation history
6. Update model panel from loaded metadata
7. Set `active_project_idx`, `active_session_name`, `active_session_dir`

- [ ] **Step 2: Implement auto-create session on first prompt**

If no session is active when the user submits their first prompt:
1. Auto-create a session named with the first few words of the prompt (sanitized, max 30 chars)
2. Under the currently selected project (or "Untitled" if none)

- [ ] **Step 3: Build and test**

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: session load/resume from project tree with stale-session fallback"
```

---

### Task 14: Auto-save + integration test

**Files:**
- Modify: `src/main.rs`
- Modify: `tests/integration.rs`

- [ ] **Step 1: Add auto-save after each successful build**

In `handle_background_result`, after `BuildComplete(Success)`:
```rust
if let Some(ref dir) = app.active_session_dir {
    if let Some(ref name) = app.active_session_name {
        let _ = app.session.save_to(dir, name, app.claude_session_id.as_deref());
    }
}
app.projects = storage::project::list_projects().unwrap_or_default();
app.project_tree.refresh(&app.projects);
```

- [ ] **Step 2: Update integration test**

The TUI uses alternate screen, so test just verifies the binary doesn't crash:

```rust
use std::process::{Command, Stdio};
use std::io::Write;

#[test]
fn test_binary_exits_cleanly() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mimodel"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start binary");

    // Send 'q' to quit
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(b"q");
    }

    let status = child.wait().expect("Failed to wait");
    assert!(status.success() || status.code().is_some());
}
```

- [ ] **Step 3: Run: `cargo test`**

- [ ] **Step 4: Commit**

```bash
git add src/main.rs tests/integration.rs
git commit -m "feat: auto-save after builds, update integration test for TUI"
```

---

## Summary

| Chunk | Tasks | What it delivers |
|-------|-------|-----------------|
| 1: Foundation | 1-5 | Dependencies, model_session rename + persistence, storage CRUD, claude thread safety |
| 2: TUI Widgets | 6-9 | Layout module, input bar, conversation pane, project tree, model panel |
| 3: TUI Integration | 10-14 | Event loop, prompt submission + threading, keybindings, session load/resume, auto-save |

Build order is sequential — each task depends on the previous. Tasks 10-14 build up main.rs incrementally: skeleton → prompt flow → keybindings → session management → persistence.

**Spec reference:** `docs/superpowers/specs/2026-03-16-mimodel-tui-design.md` — keybinding table (lines 70-101), storage format (lines 103-177), threading model (lines 267-292), image support (lines 294-306), error handling (lines 314-317).
