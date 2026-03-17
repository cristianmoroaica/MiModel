# UI Overhaul & Session Manager Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract main.rs into 6 focused modules, add dirty-flag rendering with mouse support, replace dual session format with single SessionManager, and improve visual quality of all panels.

**Architecture:** Incremental extraction — each task moves one responsibility out of main.rs into its own module while keeping the app compilable and all tests passing. The session manager rewrite happens mid-sequence after the module boundaries are established. Visual improvements come last, building on the clean architecture.

**Tech Stack:** Rust, ratatui 0.29, crossterm 0.28, serde, toml, regex

**Spec:** `docs/superpowers/specs/2026-03-17-ui-overhaul-session-manager-design.md`

**Execution order:** The chunks MUST be executed in order. Each chunk depends on the previous.

---

## Chunk 1: ClaudeBridge Extraction

Extract Claude CLI interaction into its own module. This is the cleanest extraction — it has no dependencies on other app logic.

### Task 1: Create claude_bridge.rs with ClaudeBridge struct

**Files:**
- Create: `src/claude_bridge.rs`
- Modify: `src/main.rs`
- Modify: `src/tui/mod.rs`

**Context:** Currently main.rs holds `bg_tx/rx`, `stream_tx/rx`, `bg_pid`, `claude_model`, `claude_session_id`, `streaming_text`, `busy` as separate fields on App. The background thread spawn pattern is duplicated 7 times (send_spec_prompt, send_decompose_prompt, send_component_prompt, send_component_feedback, send_assembly_feedback, send_refinement_feedback, handle_ref_command). Each spawns a thread that calls `claude::send_with_phase_prompt` or `claude::send_prompt` and sends results via `bg_tx`.

- [ ] **Step 1: Create ClaudeBridge struct with channel ownership**

Create `src/claude_bridge.rs`:

```rust
//! Claude CLI bridge — manages background threads, streaming, and result channels.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

use crate::claude;
use crate::tui::BackgroundResult;

/// Whether a background task is running.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BusyState {
    Idle,
    Thinking,
    Building,
}

pub struct ClaudeBridge {
    pub bg_tx: Sender<BackgroundResult>,
    pub bg_rx: Receiver<BackgroundResult>,
    pub stream_tx: Sender<String>,
    pub stream_rx: Receiver<String>,
    pub bg_pid: Arc<AtomicU32>,
    pub model: Option<String>,
    pub session_id: Option<String>,
    pub streaming_text: String,
    pub busy: BusyState,
}

impl ClaudeBridge {
    pub fn new(model: Option<String>) -> Self {
        let (bg_tx, bg_rx) = mpsc::channel::<BackgroundResult>();
        let (stream_tx, stream_rx) = mpsc::channel::<String>();
        Self {
            bg_tx, bg_rx, stream_tx, stream_rx,
            bg_pid: Arc::new(AtomicU32::new(0)),
            model,
            session_id: None,
            streaming_text: String::new(),
            busy: BusyState::Idle,
        }
    }

    /// Drain all pending streaming chunks. Returns true if any chunks were received.
    pub fn drain_streaming(&mut self) -> bool {
        let mut had_data = false;
        while let Ok(chunk) = self.stream_rx.try_recv() {
            self.streaming_text.push_str(&chunk);
            had_data = true;
        }
        had_data
    }

    /// Non-blocking check for a background result.
    pub fn try_recv_result(&self) -> Option<BackgroundResult> {
        self.bg_rx.try_recv().ok()
    }

    /// Send SIGTERM to the running background process.
    pub fn cancel(&self) {
        let pid = self.bg_pid.load(Ordering::SeqCst);
        if pid != 0 {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
    }

    /// Spawn a Claude call with a phase-specific system prompt.
    pub fn send_phase_prompt(
        &mut self,
        phase_name: &str,
        prompt: &str,
        images: Vec<PathBuf>,
        ref_context: Option<String>,
    ) {
        self.busy = BusyState::Thinking;
        self.streaming_text.clear();

        let model = self.model.clone();
        let session_id = self.session_id.clone();
        let tx = self.bg_tx.clone();
        let stream_tx = self.stream_tx.clone();
        let bg_pid = Arc::clone(&self.bg_pid);
        let prompt = prompt.to_string();
        let phase = phase_name.to_string();

        std::thread::spawn(move || {
            let result = claude::send_with_phase_prompt(
                &model,
                &phase,
                session_id.as_deref(),
                &prompt,
                &images,
                Some(&stream_tx),
                Some(&bg_pid),
                ref_context.as_deref(),
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

    /// Spawn a raw Claude call (no phase prompt — used for reference research).
    pub fn send_raw_prompt(
        &mut self,
        system_prompt: &str,
        prompt: &str,
        images: Vec<PathBuf>,
        result_name: String,
    ) {
        self.busy = BusyState::Thinking;
        self.streaming_text.clear();

        let model = self.model.clone();
        let tx = self.bg_tx.clone();
        let stream_tx = self.stream_tx.clone();
        let bg_pid = Arc::clone(&self.bg_pid);
        let system = system_prompt.to_string();
        let prompt = prompt.to_string();

        std::thread::spawn(move || {
            let result = claude::send_prompt(
                &model,
                &system,
                None,
                &prompt,
                &images,
                Some(&stream_tx),
                Some(&bg_pid),
            );
            bg_pid.store(0, Ordering::SeqCst);
            let _ = tx.send(BackgroundResult::ReferenceResearch {
                name: result_name,
                result: result.map(|(response, _sid)| response),
            });
        });
    }
}
```

- [ ] **Step 2: Add `mod claude_bridge;` to main.rs**

After `mod claude;` line.

- [ ] **Step 3: Move BusyState from tui/mod.rs to claude_bridge.rs**

In `src/tui/mod.rs`, remove the `BusyState` enum. Update imports across the codebase to use `crate::claude_bridge::BusyState` instead of `crate::tui::BusyState`.

- [ ] **Step 4: Replace App fields with ClaudeBridge**

In App struct, replace these fields:
```rust
    bg_tx: mpsc::Sender<BackgroundResult>,
    bg_rx: mpsc::Receiver<BackgroundResult>,
    stream_rx: mpsc::Receiver<String>,
    stream_tx: mpsc::Sender<String>,
    bg_pid: Arc<AtomicU32>,
    // ... claude_model, claude_session_id, streaming_text
```

With:
```rust
    claude: claude_bridge::ClaudeBridge,
```

Update `App::new()` to create `ClaudeBridge::new(config.claude_model.clone())`.

- [ ] **Step 5: Update all send_*_prompt methods to use self.claude**

Replace each duplicated thread-spawn pattern with a call to `self.claude.send_phase_prompt()` or `self.claude.send_raw_prompt()`. For example, `send_spec_prompt` becomes:

```rust
fn send_spec_prompt(&mut self, text: &str, images: Vec<PathBuf>) {
    let ref_context = self.build_ref_context();
    let prompt = if self.claude.session_id.is_some() {
        if let Some(ref ctx) = ref_context {
            format!("[Reference context]\n{}\n\n{}", ctx, text)
        } else {
            text.to_string()
        }
    } else {
        text.to_string()
    };
    self.claude.send_phase_prompt("spec", &prompt, images, ref_context);
}
```

Repeat for all 7 send methods. The reference research in `handle_ref_command` uses `send_raw_prompt`.

- [ ] **Step 6: Update event loop to use ClaudeBridge methods**

Replace the manual `stream_rx.try_recv()` drain loop and `bg_rx.try_recv()` check with:
```rust
let had_stream = app.claude.drain_streaming();
if had_stream { /* scroll_to_bottom, set dirty */ }

if let Some(result) = app.claude.try_recv_result() {
    app.handle_bg_result(result);
}
```

Replace `self.cleanup()` SIGTERM logic with `self.claude.cancel()`.

- [ ] **Step 7: Update handle_bg_result to read session_id from ClaudeBridge**

When a `ClaudeResponse` arrives with a `session_id`, store it on `self.claude.session_id` instead of `self.claude_session_id`.

- [ ] **Step 8: Build and test**

Run: `cargo build 2>&1 | rg "^error" || echo "BUILD OK"`
Run: `cargo test 2>&1 | rg "^test result"`
Expected: BUILD OK, all tests pass.

- [ ] **Step 9: Commit**

```bash
git add src/claude_bridge.rs src/main.rs src/tui/mod.rs
git commit -m "refactor: extract ClaudeBridge from main.rs — channels, streaming, thread spawn"
```

## Chunk 2: Dirty-Flag Rendering & Mouse Support

### Task 2: Add dirty flag and optimize render loop

**Files:**
- Modify: `src/main.rs` (event loop)

- [ ] **Step 1: Add `dirty: bool` field to App, initialize to `true`**

- [ ] **Step 2: Restructure event loop**

Change from:
```
render → drain_stream → check_bg → poll_events → spinner
```
To:
```
drain_stream → check_bg → render_if_dirty → poll_events → spinner_if_busy
```

Only call `terminal.draw()` when `dirty == true`. After draw, set `dirty = false`.
Set `dirty = true` after: any key/mouse/paste event, any stream drain, any bg result, spinner tick.

Cap spinner to every 5th tick (10fps): `if tick_count % 5 == 0 { spinner_frame += 1; dirty = true; }`

- [ ] **Step 3: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 4: Commit**

```bash
git commit -am "perf: dirty-flag rendering — skip re-render when nothing changed"
```

### Task 3: Add mouse support

**Files:**
- Modify: `src/main.rs` (terminal init, event loop)
- Modify: `src/tui/layout.rs` (expose PanelRects)
- Modify: `src/tui/mod.rs` (Focus enum)

- [ ] **Step 1: Enable mouse capture**

In terminal init (around `enable_raw_mode`), add `crossterm::event::EnableMouseCapture`.
In terminal cleanup, add `DisableMouseCapture`.

- [ ] **Step 2: Add Focus::RightPanel variant**

In `src/tui/mod.rs`, add `RightPanel` to the `Focus` enum.

- [ ] **Step 3: Store panel Rects from render**

Add `panel_rects: PanelRects` field to App. Define `PanelRects` struct in `src/tui/layout.rs`:
```rust
#[derive(Default, Clone)]
pub struct PanelRects {
    pub project_tree: Rect,
    pub conversation: Rect,
    pub right_panel: Rect,
    pub input: Rect,
}
```

Populate it during `render()` from the layout computation results.

- [ ] **Step 4: Handle mouse events in event loop**

Add to the event match:
```rust
Event::Mouse(mouse) => {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let col = mouse.column;
            let row = mouse.row;
            if app.panel_rects.project_tree.contains(Position::new(col, row)) {
                app.focus = Focus::ProjectTree;
            } else if app.panel_rects.conversation.contains(Position::new(col, row)) {
                app.focus = Focus::Conversation;
            } else if app.panel_rects.right_panel.contains(Position::new(col, row)) {
                app.focus = Focus::RightPanel;
            } else if app.panel_rects.input.contains(Position::new(col, row)) {
                app.focus = Focus::Input;
            }
            dirty = true;
        }
        MouseEventKind::ScrollUp => {
            let col = mouse.column;
            let row = mouse.row;
            if app.panel_rects.conversation.contains(Position::new(col, row)) {
                app.conversation.scroll_up(3);
            }
            // Add right_panel scroll when implemented
            dirty = true;
        }
        MouseEventKind::ScrollDown => {
            let col = mouse.column;
            let row = mouse.row;
            if app.panel_rects.conversation.contains(Position::new(col, row)) {
                app.conversation.scroll_down(3);
            }
            dirty = true;
        }
        _ => {}
    }
}
```

- [ ] **Step 5: Update Tab cycling to include RightPanel**

Tab: Input → Conversation → RightPanel → ProjectTree → Input.
Shift+Tab: reverse.

- [ ] **Step 6: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 7: Manual test — click panels, scroll with wheel**

Run: `cargo run`, verify click-to-focus and scroll-on-hover work.

- [ ] **Step 8: Commit**

```bash
git commit -am "feat: mouse support — click-to-focus, scroll-on-hover, tab cycle with RightPanel"
```

## Chunk 3: Session Manager

### Task 4: Create session_manager.rs, delete LegacySession

**Files:**
- Create: `src/session_manager.rs`
- Modify: `src/main.rs`
- Modify: `src/model_session.rs`
- Modify: `src/storage/session.rs`

**This is the riskiest task.** It deletes LegacySession and replaces the dual conversation state with a single owner. Take it step by step.

- [ ] **Step 1: Create SessionManager struct**

Create `src/session_manager.rs`:

```rust
//! Session lifecycle manager — single source of truth for session state.

use crate::model_session::PhaseSession;
use crate::phase::Phase;
use crate::storage::session::ConversationEntry;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct SessionManager {
    pub active_dir: Option<PathBuf>,
    pub active_name: Option<String>,
    pub project_idx: Option<usize>,
    pub phase_session: Option<PhaseSession>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            active_dir: None,
            active_name: None,
            project_idx: None,
            phase_session: None,
        }
    }

    /// Get conversations for the current phase.
    pub fn conversations(&self, phase: Phase) -> &[ConversationEntry] {
        self.phase_session
            .as_ref()
            .and_then(|ps| ps.conversations.get(&phase.label().to_string()))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Add a message to the current phase's conversation. Does NOT auto-save.
    pub fn add_message(&mut self, phase: Phase, role: &str, content: &str) {
        if let Some(ref mut ps) = self.phase_session {
            let key = phase.label().to_string();
            ps.conversations
                .entry(key)
                .or_insert_with(Vec::new)
                .push(ConversationEntry {
                    role: role.to_string(),
                    content: content.to_string(),
                });
        }
    }

    /// Save the current session to disk.
    pub fn save(&mut self, phase: Phase) {
        if let Some(ref mut ps) = self.phase_session {
            ps.phase = phase;
            if let Err(e) = ps.save() {
                eprintln!("Warning: session save failed: {e}");
            }
        }
    }

    /// Create a new session in the given directory.
    pub fn create(&mut self, dir: PathBuf, build_timeout: u64, python_path: String) {
        self.phase_session = Some(PhaseSession::new(dir.clone(), build_timeout, python_path));
        self.active_dir = Some(dir);
    }

    /// Load an existing session.
    pub fn load(&mut self, dir: &Path, build_timeout: u64, python_path: String) -> Result<(), String> {
        let ps = PhaseSession::load(dir, build_timeout, python_path)?;
        self.active_dir = Some(dir.to_path_buf());
        self.phase_session = Some(ps);
        Ok(())
    }

    /// Reset for a new session.
    pub fn reset(&mut self) {
        self.active_dir = None;
        self.active_name = None;
        self.phase_session = None;
    }

    /// Whether a session is active.
    pub fn is_active(&self) -> bool {
        self.phase_session.is_some()
    }
}
```

- [ ] **Step 2: Add `mod session_manager;` to main.rs**

- [ ] **Step 3: Delete LegacySession code**

In `src/model_session.rs`:
- Delete `LegacySession` struct and entire `impl LegacySession` block
- Delete `Session` type alias if present
- Delete `SessionState` enum
- Keep `PhaseSession` and all its methods

In `src/storage/session.rs`:
- Delete `is_legacy_session_json()`
- Delete `load_session_data()` and `SessionData` references
- Delete `session_status()` and `SessionStatus` enum
- Keep `PhaseSessionData`, `ConversationEntry`, `ClaudeSessionMap`
- Keep `create_session`, `delete_session`, `rename_session`

In `src/storage/project.rs`:
- Update `list_projects()` to remove `is_legacy_session_json` call — all sessions with `session.json` are phase sessions now

- [ ] **Step 4: Replace App session fields with SessionManager**

Replace these App fields:
```rust
    session: Session,
    active_session_name: Option<String>,
    active_session_dir: Option<PathBuf>,
    phase_session: Option<PhaseSession>,
    active_project_idx: Option<usize>,
```

With:
```rust
    session: session_manager::SessionManager,
```

Update `App::new()` accordingly.

- [ ] **Step 5: Update submit_prompt to use SessionManager**

Replace `self.session.add_user_message()` with `self.session.add_message(self.phase, "user", &text)`.
Replace `self.session.add_assistant_message()` with `self.session.add_message(self.phase, "assistant", &response)`.
Replace `self.save_phase_session()` with `self.session.save(self.phase)`.
Remove `sync_conversations_to_phase_session()` entirely.

- [ ] **Step 6: Update ConversationPane to render from SessionManager**

In the render function, instead of using `self.conversation.entries`, pass `self.session.conversations(self.phase)` to the conversation pane render. The ConversationPane no longer owns entries — it takes a slice.

Modify `src/tui/conversation.rs`:
- Change `render()` to accept `&[ConversationEntry]` parameter instead of using `self.entries`
- Keep scroll_offset and auto_scroll as state on the pane
- Remove `entries: Vec<ConversationEntry>` field
- Remove `add()` method
- Add `add_system()` for inline system messages that DON'T go to the session (transient UI messages like "Press Ctrl+C again to quit")

Note: Use `crate::storage::session::ConversationEntry` as the canonical type. Delete `tui::conversation::ConversationEntry`.

- [ ] **Step 7: Update all callers of conversation.add()**

Every `self.conversation.add("system", ...)` becomes either:
- `self.session.add_message(self.phase, "system", ...)` — for persistent messages (spec data, Claude responses)
- `self.conversation.add_system(...)` — for transient UI messages ("Press Ctrl+C again to quit", "Researching...")

Go through each caller and decide which bucket it belongs to.

- [ ] **Step 8: Update session load/save paths**

Replace `load_phase_session()` with `self.session.load()`.
Replace `save_phase_session()` with `self.session.save(self.phase)`.
Replace session dir/name access with `self.session.active_dir` / `self.session.active_name`.

- [ ] **Step 9: Update project tree session listing**

Add a `phase_session_status()` function in `storage/session.rs` that reads `PhaseSessionData` and returns phase name + message count. Use this in the project tree display instead of the deleted `session_status()`.

- [ ] **Step 10: Build and test**

Run: `cargo build 2>&1 | rg "^error" || echo "BUILD OK"`
Run: `cargo test`

Fix compilation errors one at a time. This task will likely require multiple fix passes as dead code paths surface.

- [ ] **Step 11: Delete stale tests that reference LegacySession**

Remove or update tests in `model_session.rs` and `storage/session.rs` that use `Session::new()`, `LegacySession`, etc.

- [ ] **Step 12: Run full test suite**

Run: `cargo test`
Expected: All tests pass (count will decrease as legacy tests are deleted).

- [ ] **Step 13: Commit**

```bash
git add -A
git commit -m "refactor: replace LegacySession with SessionManager — single session format, conversation ownership"
```

## Chunk 4: Input Field Fixes

### Task 5: Fix newline insertion and multi-line expansion

**Files:**
- Modify: `src/tui/input_bar.rs`
- Modify: `src/tui/layout.rs`
- Modify: `src/main.rs` (event handling for input)

- [ ] **Step 1: Fix `\` + Return newline**

In the input key handler (main.rs, where Enter is handled for submission), check if the input text ends with `\`:
```rust
let text = self.input_bar.text();
if text.ends_with('\\') {
    // Strip trailing backslash and insert newline
    self.input_bar.textarea.delete_char(); // remove the \
    self.input_bar.textarea.insert_newline();
    return;
}
// ... normal submit
```

- [ ] **Step 2: Add `input_height` to LayoutConfig**

In `src/tui/layout.rs`, add `pub input_height: u16` to `LayoutConfig` with default `3`.

Update `compute_layout()` to use `config.input_height` instead of hardcoded input area height.

- [ ] **Step 3: Compute dynamic input height**

In main.rs, before calling `compute_layout()`, compute input height based on newline count:
```rust
let newlines = self.input_bar.textarea.lines().len();
self.layout_config.input_height = (newlines as u16 + 2).clamp(3, 7); // +2 for borders
```

- [ ] **Step 4: Add phase-aware placeholder**

In `src/tui/input_bar.rs`, add a method `set_placeholder(&mut self, text: &str)`. Use `tui_textarea::TextArea::set_placeholder_text()` if available, otherwise render placeholder text when the buffer is empty.

Set placeholder based on phase:
```rust
match phase {
    Phase::Spec => "Describe what you want to build...",
    Phase::Decompose => "Describe changes to the component tree...",
    Phase::Component => "Feedback, 'approve', or 'undo'...",
    Phase::Assembly => "Assembly instructions or feedback...",
    Phase::Refinement => "Parameter changes or feedback...",
}
```

- [ ] **Step 5: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 6: Commit**

```bash
git commit -am "fix: input field — newline insertion, multi-line expansion, phase placeholders"
```

## Chunk 5: Right Panel Tabs

### Task 6: Create tabbed right panel widget

**Files:**
- Create: `src/tui/right_panel.rs`
- Modify: `src/tui/mod.rs`
- Modify: `src/main.rs` (render, key handling)

- [ ] **Step 1: Create RightPanel with tab state**

```rust
//! Right panel — tabbed container for Spec, Refs, and Model views.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Tabs, Wrap};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RightTab {
    Spec,
    Refs,
    Model,
}

pub struct RightPanel {
    pub active_tab: RightTab,
    pub scroll_offset: u16,
    spec_content: String,
    refs_content: String,
    model_content: String,
}

impl RightPanel {
    pub fn new() -> Self {
        Self {
            active_tab: RightTab::Spec,
            scroll_offset: 0,
            spec_content: String::new(),
            refs_content: String::new(),
            model_content: String::new(),
        }
    }

    pub fn set_spec(&mut self, content: &str) { self.spec_content = content.to_string(); }
    pub fn set_refs(&mut self, content: &str) { self.refs_content = content.to_string(); }
    pub fn set_model(&mut self, content: &str) { self.model_content = content.to_string(); }

    pub fn next_tab(&mut self) {
        self.active_tab = match self.active_tab {
            RightTab::Spec => RightTab::Refs,
            RightTab::Refs => RightTab::Model,
            RightTab::Model => RightTab::Spec,
        };
        self.scroll_offset = 0;
    }

    pub fn prev_tab(&mut self) {
        self.active_tab = match self.active_tab {
            RightTab::Spec => RightTab::Model,
            RightTab::Refs => RightTab::Spec,
            RightTab::Model => RightTab::Refs,
        };
        self.scroll_offset = 0;
    }

    pub fn scroll_up(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    pub fn scroll_down(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_add(amount);
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let border_color = if focused { Color::Cyan } else { Color::DarkGray };

        // Split area: tabs header (1 line) + content
        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
        ]).split(area);

        // Tab headers
        let tab_titles = vec!["Spec", "Refs", "Model"];
        let selected = match self.active_tab {
            RightTab::Spec => 0,
            RightTab::Refs => 1,
            RightTab::Model => 2,
        };
        let tabs = Tabs::new(tab_titles)
            .select(selected)
            .highlight_style(Style::default().fg(Color::Yellow).bold())
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(tabs, chunks[0]);

        // Content
        let content = match self.active_tab {
            RightTab::Spec => &self.spec_content,
            RightTab::Refs => &self.refs_content,
            RightTab::Model => &self.model_content,
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));

        let paragraph = Paragraph::new(content.as_str())
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));

        frame.render_widget(paragraph, chunks[1]);
    }
}
```

- [ ] **Step 2: Add `pub mod right_panel;` to tui/mod.rs**

- [ ] **Step 3: Replace SpecPanel + ModelPanel usage with RightPanel in App**

Replace `spec_panel` and `model_panel` fields with `right_panel: tui::right_panel::RightPanel`.

Update render function to call `self.right_panel.render()` for the right area.

Update spec response handler to call `self.right_panel.set_spec()`.
Update build result handler to call `self.right_panel.set_model()`.
Update reference loading to call `self.right_panel.set_refs()`.

- [ ] **Step 4: Handle RightPanel focus keys**

When `focus == Focus::RightPanel`:
- Left/Right or h/l: switch tabs
- j/k or Up/Down: scroll
- Mouse scroll on hover also scrolls

- [ ] **Step 5: Add mouse scroll for right panel**

In the mouse scroll handler (from Task 3), add:
```rust
if app.panel_rects.right_panel.contains(Position::new(col, row)) {
    app.right_panel.scroll_down(3); // or scroll_up
}
```

- [ ] **Step 6: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 7: Commit**

```bash
git commit -am "feat: tabbed right panel — Spec/Refs/Model tabs, focusable and scrollable"
```

## Chunk 6: Conversation Visual Improvements

### Task 7: Compact system messages and basic markdown

**Files:**
- Modify: `src/tui/conversation.rs`

- [ ] **Step 1: Style system messages as compact banners**

In the render function, when building styled lines for a "system" role entry, use a dim background style and prefix with `ⓘ`:

```rust
"system" => {
    // Compact single-line banner style
    lines.push(Line::from(vec![
        Span::styled("  ⓘ ", Style::default().fg(Color::DarkGray)),
        Span::styled(&entry.content, Style::default().fg(Color::DarkGray)),
    ]));
    // No blank separator after system messages
}
```

Instead of the current "prefix + content lines + blank line" pattern.

- [ ] **Step 2: Add basic markdown rendering**

Add a helper function `render_markdown(text: &str) -> Vec<Line>` that handles:
- `**bold**` → `Style::default().bold()`
- `` `code` `` → `Style::default().fg(Color::Yellow)`
- `- item` → indented with bullet
- Everything else → plain text

This is a simple state-machine parser, not a full markdown library. Keep it under 80 lines.

Use this when rendering assistant messages instead of raw `Span::raw`.

- [ ] **Step 3: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 4: Commit**

```bash
git commit -am "feat: compact system messages and basic markdown rendering in conversation"
```

## Chunk 7: Multi-/ref Parsing

### Task 8: Parse and batch-process multiple /ref commands

**Files:**
- Modify: `src/main.rs` (handle_ref_command)

- [ ] **Step 1: Split multiple /ref from one input**

In the `/ref` dispatch (before calling `handle_ref_command`), parse for multiple refs:

```rust
if text.starts_with("/ref") {
    // Split on /ref boundaries
    let refs: Vec<&str> = text.split("/ref")
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().trim_start_matches(',').trim())
        .collect();

    if refs.len() > 1 {
        self.handle_multi_ref(refs, images);
    } else {
        self.handle_ref_command(&text, images);
    }
    return;
}
```

- [ ] **Step 2: Implement handle_multi_ref**

```rust
fn handle_multi_ref(&mut self, names: Vec<&str>, images: Vec<PathBuf>) {
    // Separate known (in library) from unknown (need research)
    let mut loaded = Vec::new();
    let mut to_research = Vec::new();

    for name in &names {
        match reference::load_one(name) {
            Ok((comp, slug)) => {
                if !self.active_refs.contains(&slug) {
                    self.active_refs.push(slug.clone());
                }
                loaded.push(comp.identity.name.clone());
            }
            Err(_) => to_research.push(name.to_string()),
        }
    }

    if !loaded.is_empty() {
        self.session.add_message(self.phase, "system",
            &format!("Loaded {} references: {}", loaded.len(), loaded.join(", ")));
    }

    if to_research.is_empty() {
        return;
    }

    // Research all unknown in a single Claude call
    self.session.add_message(self.phase, "system",
        &format!("Researching {} components: {}...", to_research.len(), to_research.join(", ")));

    let research_prompt = format!(
        "Research these components and return a TOML block for EACH one:\n{}\n\n\
         For each component, output a ```toml fenced block with [identity], [dimensions], [constraints], [sources] sections.\n\
         Separate each component's TOML block clearly.",
        to_research.join(", ")
    );

    self.claude.send_raw_prompt(
        "You are a technical reference researcher.",
        &research_prompt,
        images,
        to_research.join(","),
    );
}
```

- [ ] **Step 3: Handle batch research result**

In `handle_bg_result`, when `ReferenceResearch` arrives and `name` contains commas, parse multiple TOML blocks from the response. Present: "Found N components. Save all? (yes/no/pick)".

Store the batch in `ref_confirm_pending` with the raw response. On `yes`, parse and save each. On `pick`, show numbered list.

- [ ] **Step 4: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 5: Commit**

```bash
git commit -am "feat: multi-/ref — parse multiple commands, batch research, batch save"
```

## Chunk 8: Final Integration

### Task 9: Clean up, delete dead code, final test

**Files:**
- Modify: multiple files

- [ ] **Step 1: Delete dead code**

Run `cargo build 2>&1 | rg "warning.*never used"` and delete unused functions/structs that are artifacts of the old architecture.

- [ ] **Step 2: Delete old SpecPanel and ModelPanel if fully replaced**

If RightPanel has fully replaced them, delete `src/tui/spec_panel.rs` and `src/tui/model_panel.rs`, remove their `pub mod` lines from `tui/mod.rs`.

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 4: Manual smoke test**

Run `cargo run` and verify:
1. Click panels to focus them
2. Scroll with mouse wheel on hover
3. Right panel tabs switch with Left/Right when focused
4. Streaming doesn't freeze navigation
5. `\` + Return inserts newline in input
6. Multi-line input expands the input bar
7. `/ref list` shows references
8. Multiple `/ref` commands work
9. Session saves and loads correctly
10. New session resets cleanly

- [ ] **Step 5: Commit**

```bash
git commit -am "chore: clean up dead code after UI overhaul"
```
