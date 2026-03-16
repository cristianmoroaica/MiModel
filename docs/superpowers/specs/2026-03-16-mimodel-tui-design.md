# MiModel TUI & Session Persistence Design

**Date:** 2026-03-16
**Status:** Draft

## Overview

Redesign MiModel from a readline REPL into a three-column ratatui TUI with project-based session persistence. Sessions are auto-saved to `~/MiModel/` with full iteration history (code, STL, metadata, images, conversation). The existing backend modules (claude.rs, python.rs, parser.rs, stl.rs, preview.rs, viewer.rs, image.rs) remain unchanged.

## Goals

- Three-column TUI: project tree | conversation | model panel + full-width input bar
- Project-based session organization — projects contain multiple sessions
- Full persistence — every iteration's code, STL, metadata, images, and conversation saved
- Non-blocking UI — claude and build calls run in background threads
- Keyboard-driven navigation with sensible defaults

## Non-Goals

- Mouse support (may be added later, not in this iteration)
- Theming/color customization
- Collaborative/multi-user features
- Cloud sync

## TUI Layout

```
┌─ Projects (20%) ──┬─ Conversation (55%) ──────────────────┬─ Model (25%) ───┐
│                   │                                       │                 │
│ ▼ Train Station   │  you: a mounting bracket for RPi 4    │ 92.0 × 62.0 × 6│
│   ├─ enclosure    │  with 4 screw holes matching the      │                 │
│   ├─ servo mount  │  Pi's mounting pattern                │ Features:       │
│   └─ arduino ◀    │                                       │  4× M2.5 holes  │
│                   │  claude: I'll create a mounting       │  4× standoffs   │
│ ▶ Drone Parts     │  bracket for the RPi 4...             │  fillet 2mm     │
│                   │                                       │                 │
│ ▶ Misc            │  ✓ Built successfully                 │ Preview:        │
│                   │  92.0 × 62.0 × 6.5 mm                │  ⣀⣠⣤⣤⣤⣤⣄⣀      │
│                   │  - 4× M2.5 holes                     │  ⣿⣿⣿○⣿○⣿⣿⣿     │
│                   │  - 4× standoffs                      │  ⣿⣿⣿⣿⣿⣿⣿⣿⣿     │
│                   │                                       │  ⣿⣿⣿○⣿○⣿⣿⣿     │
│                   │  you: add rounded corners, 3mm        │  ⠈⠙⠛⠛⠛⠛⠋⠁      │
│                   │                                       │                 │
│                   │  claude: Added 3mm fillets...         │ Iterations: 2   │
│                   │                                       │ Engine: cadquery │
│ + New Project     │  ✓ Built successfully                 │ Watertight: yes  │
│                   │  92.0 × 62.0 × 6.5 mm                │                 │
│                   │                                       │ [s]how [e]xport │
├───────────────────┴───────────────────────────────────────┴─────────────────┤
│ > add ventilation slots on the top face, 2mm wide                          │
└────────────────────────────────────────────────────────────────────────────┘
```

### Column Details

- **Left column (20%)** — Project tree. Projects are collapsible groups containing sessions. Active session highlighted. `+ New Project` at the bottom. Focus with Tab, navigate with arrow keys.
- **Center column (55%)** — Scrollable conversation history. User prompts and Claude responses with build results (dimensions, features) shown inline. Auto-scrolls to bottom on new content.
- **Right column (25%)** — Current model state: dimensions, features list, braille preview, iteration count, engine, watertight status. Updates after each build. Shows "[s]how [e]xport" shortcuts.
- **Bottom bar (full width, 3 lines)** — Input area using `tui-textarea`. Multi-line via Enter, submit via Ctrl+Enter. Prompt history via Up/Down when input is empty.

### Column Toggle

- `Ctrl+L` toggles the left sidebar. When hidden, conversation and model panel expand.
- `Ctrl+R` toggles the right panel. When hidden, conversation expands.
- Both hidden = fullscreen conversation + input bar.

## Keybindings

### Global

| Key | Action |
|-----|--------|
| `Ctrl+Enter` | Send prompt |
| `Enter` | New line in input |
| `Ctrl+N` | New session in current project |
| `Ctrl+P` | New project |
| `Ctrl+S` | Export current STL |
| `Ctrl+O` | Open in external viewer (f3d) |
| `Ctrl+Z` | Undo last iteration |
| `Ctrl+L` | Toggle left sidebar |
| `Ctrl+R` | Toggle right panel |
| `Tab` | Cycle focus: input → project tree → conversation → input |
| `Esc` | Return focus to input |
| `q` | Quit (only when input bar is focused and empty) |

### Project Tree (when focused)

| Key | Action |
|-----|--------|
| `Up/Down` | Move selection |
| `Enter` | Open session / toggle project collapse |
| `d` | Delete (with confirmation) |
| `r` | Rename |

### Conversation (when focused)

| Key | Action |
|-----|--------|
| `Up/Down` / `j/k` | Scroll |
| `c` | Copy current code to clipboard |
| `PageUp/PageDown` | Page scroll |

## Session & Project Storage

### Directory Structure

```
~/MiModel/
├── Train Station/
│   ├── project.json
│   ├── enclosure/
│   │   ├── session.json
│   │   ├── iter_001.py
│   │   ├── iter_001.stl
│   │   ├── iter_001.json
│   │   ├── iter_002.py
│   │   ├── iter_002.stl
│   │   ├── iter_002.json
│   │   └── images/
│   │       └── clipboard_1773672035.png
│   └── servo mount/
│       ├── session.json
│       └── ...
└── Drone Parts/
    ├── project.json
    └── ...
```

### project.json

```json
{
  "name": "Train Station",
  "created": "2026-03-16T15:30:00Z",
  "description": "Electronics for Gara de Nord model"
}
```

### session.json

```json
{
  "name": "enclosure",
  "created": "2026-03-16T15:32:00Z",
  "modified": "2026-03-16T16:10:00Z",
  "iteration_count": 3,
  "claude_session_id": "c55fa6bf-4503-40fd-a530-f81fd2d2fce0",
  "current_iteration": 3,
  "engine": "cadquery",
  "conversation": [
    {"role": "user", "content": "a box enclosure 80x60x30mm..."},
    {"role": "assistant", "content": "```cadquery\nimport cadquery...```\n\nCreated an 80x60x30mm enclosure..."}
  ]
}
```

### iter_NNN.json (per-iteration metadata)

```json
{
  "dimensions": {"x": 80.0, "y": 60.0, "z": 30.0},
  "volume_mm3": 14280.5,
  "triangle_count": 3420,
  "features": ["box 80x60x30mm", "wall thickness 2mm"],
  "watertight": true,
  "engine": "cadquery"
}
```

### Persistence Behavior

- **Auto-save**: every successful build writes code, STL, and metadata to the session directory. session.json is updated with the new conversation state and iteration count.
- **New session**: `Ctrl+N` prompts for a name in the input bar. Creates the session directory under the current project.
- **New project**: `Ctrl+P` prompts for a name. Creates the project directory with project.json.
- **Load session**: clicking a session in the tree loads it — restores conversation, model state, and attempts claude session resume (via `--resume`). If the claude session is stale (expired or pruned), falls back to a fresh claude session with `--system-prompt` and injects the last working code + metadata as context so Claude has the necessary state without full conversation replay.
- **Export**: `Ctrl+S` prompts for a destination path and copies the current STL there.
- **First launch**: if `~/MiModel/` doesn't exist, creates it with a default "Untitled" project.

## Module Architecture

```
src/
├── main.rs              # Rewrite — ratatui app init, terminal setup, event loop
├── tui/
│   ├── mod.rs           # App struct, state, focus, event dispatch
│   ├── layout.rs        # Three-column + input bar constraint calculation
│   ├── project_tree.rs  # Left pane — collapsible project/session tree widget
│   ├── conversation.rs  # Center pane — scrollable styled message list
│   ├── model_panel.rs   # Right pane — dims, features, braille preview, metadata
│   └── input_bar.rs     # Bottom — tui-textarea wrapper, submit/history logic
├── storage/
│   ├── mod.rs           # Public API (list_projects, create_project, load_session, etc.)
│   ├── project.rs       # Project CRUD — create, list, rename, delete directories
│   └── session.rs       # Session serialization — save/load session.json + iteration files
│
│ # Renamed:
├── model_session.rs     # Was session.rs — runtime build state (iterations, undo, build)
│
│ # Unchanged:
├── claude.rs
├── config.rs
├── image.rs
├── parser.rs
├── preview.rs
├── python.rs
├── stl.rs
└── viewer.rs
```

### Module Responsibilities

**`tui/mod.rs` (App):**
- Owns all state: focused pane, project tree state, conversation scroll, model panel data
- Dispatches key events to the focused pane
- Manages background task channels (claude calls, builds)
- Calls storage module on auto-save and session load

**`tui/layout.rs`:**
- Computes ratatui `Layout` constraints based on terminal size and toggle state
- Returns `Rect` for each pane

**`tui/project_tree.rs`:**
- Renders the project/session tree as a `List` widget
- Handles selection, collapse/expand, creation prompts

**`tui/conversation.rs`:**
- Renders conversation as a scrollable styled `Paragraph` or `List`
- User messages in green, claude in purple, build results styled inline
- Auto-scroll to bottom, manual scroll when conversation is focused

**`tui/model_panel.rs`:**
- Renders model metadata, braille preview, iteration info
- Updates when `ModelMetadata` changes

**`tui/input_bar.rs`:**
- Wraps `tui-textarea::TextArea`
- Handles Ctrl+Enter to submit, Up/Down for history when empty
- Passes text to App on submit

**`storage/` module:**
- Pure filesystem operations — no UI, no runtime state
- `list_projects()` → scans `~/MiModel/` for project dirs
- `create_project(name)` → creates dir + project.json
- `list_sessions(project)` → scans project dir for session dirs
- `save_session(path, session_data)` → writes session.json + iteration files
- `load_session(path)` → reads session.json, returns conversation + iteration data

**`model_session.rs` (renamed from session.rs):**
- Runtime build state — iterations, undo snapshots, temp files during active editing
- Gains `save_to(dir: &Path)` — copies iteration files to a session directory
- Gains `load_from(dir: &Path)` — restores state from a session directory

## Event Loop & Background Tasks

```
┌──────────────────────────────────────────────────────┐
│                    Event Loop                        │
│  loop {                                              │
│    1. poll crossterm events (50ms timeout)            │
│    2. check background channel (rx.try_recv)          │
│    3. app.update(event_or_result)                     │
│    4. terminal.draw(|f| app.render(f))                │
│  }                                                   │
└──────────────────────────────────────────────────────┘
```

### Background Threading

Claude CLI calls (5-60s) and Python builds (1-60s) must not block the UI.

```rust
enum BackgroundResult {
    ClaudeResponse {
        result: Result<String, String>,
        session_id: Option<String>,  // updated session_id from claude
    },
    BuildComplete(BuildResult),
}
```

**ClaudeClient threading model:** `ClaudeClient::send()` is refactored to take `&self` and return `(Result<String, String>, Option<String>)` where the second value is the captured session_id. The main thread updates `client.session_id` after receiving the result. This avoids needing `&mut self` in the background thread. The client's immutable fields (model, system_prompt) are cheaply cloneable for the thread; session_id is passed as a separate argument.

When the user submits a prompt:
1. App shows "Thinking..." with an animated braille spinner in conversation pane (rotates every 100ms via the 50ms tick)
2. Spawns `std::thread::spawn` with the prompt, image paths, model, system_prompt, and current session_id
3. Thread calls claude CLI, sends result + updated session_id via `mpsc::Sender<BackgroundResult>`
4. Main loop's tick checks `rx.try_recv()` — if result arrives, updates session_id and processes response
5. If response contains code, spawns another thread for the Python build

**Cancellation:** The background thread stores the child process PID in a shared `Arc<AtomicU32>`. On `Ctrl+C` during an in-flight call, the main thread reads the PID and sends SIGTERM. The conversation shows a "(cancelled)" marker. The session_id remains valid (Claude CLI handles interrupted sessions gracefully).

**Ctrl+Z during in-flight:** disabled. Undo only works when no background task is running.

## Image Support

Existing image functionality (clipboard paste, inline path detection) is preserved in the TUI.

**Keybinding:** `Ctrl+V` pastes an image from the Wayland clipboard (via `wl-paste`). The image is saved to the session's `images/` directory and a notification appears in the conversation: "Attached image (142KB)". The image path is passed to `ClaudeClient::send()` along with the next prompt.

**Inline paths:** Image file paths typed in the input bar (e.g., `~/photos/sketch.png`) are auto-detected by `image::extract_image_paths()` before sending. Detected paths are shown as "[image attached]" in the conversation.

**Pending images indicator:** When images are queued, the input bar shows a small badge: `[2 images]` to the left of the cursor.

**Conversation rendering:** Attached images show as `[image: filename.png]` in the conversation history. No inline preview (terminal limitation).

**Persistence:** Images are copied to the session's `images/` subdirectory on attach, so they survive across sessions.

## Terminal Size Constraints

Minimum terminal width: 100 columns. Below this, the left sidebar auto-hides. Below 60 columns, the right panel also hides, leaving just conversation + input. Below 40 columns, a "Terminal too narrow" message is shown.

## Error Handling (Storage)

- **Permission errors:** shown as a notification in the conversation pane, operation skipped
- **Corrupted session.json:** logged to stderr, session shown as "(corrupted)" in the project tree, not loadable
- **Missing iteration files:** session loads with available iterations, missing ones shown as gaps
- **Disk full:** auto-save fails gracefully with a warning, session continues in memory

## Dependencies

| Crate | Purpose | Status |
|-------|---------|--------|
| `ratatui` | TUI framework | Add (was listed but removed during CLI refactor) |
| `crossterm` | Terminal backend | Already in Cargo.toml |
| `tui-textarea` | Input widget | New |
| `serde` / `serde_json` | Serialization | Already in Cargo.toml |
| `dirs` | Home directory | Already in Cargo.toml |
| `tempfile` | Temp dirs for builds | Already in Cargo.toml |
| `rustyline` | **Remove** — replaced by tui-textarea | Remove |

## Startup Flow

1. Initialize terminal (raw mode, alternate screen)
2. Load config from `~/.config/mimodel/config.toml`
3. Check claude CLI and Python availability
4. Scan `~/MiModel/` for projects and sessions (create if missing)
5. If a session was active last time, restore it. Otherwise show empty state.
6. Enter event loop

## First Launch

If `~/MiModel/` doesn't exist:
1. Create `~/MiModel/`
2. Create a default project: `~/MiModel/Untitled/project.json`
3. Start with an empty session in the Untitled project
