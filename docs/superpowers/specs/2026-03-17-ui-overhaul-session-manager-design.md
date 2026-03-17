# UI Overhaul & Session Manager Redesign

**Date:** 2026-03-17
**Status:** Design

## Problem

main.rs is a 2,474-line God Object with 80+ App struct fields covering UI, session management, phase logic, Claude interaction, and rendering. The result:

- UI freezes during Claude streaming (render loop blocks on event poll)
- No mouse support (zero mouse event handling)
- Right panel is dead space (not focusable, not scrollable)
- Raw TOML dumps in conversation instead of formatted summaries
- Session state duplicated between UI pane and storage (manual sync, data loss on crash)
- LegacySession format adds dead code paths
- Input field newline (`\` + Return) broken
- Multiple `/ref` commands in one line treated as single query

## Solution

Extract main.rs into 6 focused modules, add dirty-flag rendering, mouse support, tabbed right panel, and replace the dual-session system with a single session manager.

## Module Architecture

### main.rs (~400 lines) — App shell
- `App` struct with split-borrowable sub-structs: `ui: UiState`, `session: SessionManager`, `claude: ClaudeBridge`, `phase: PhaseState`
- This split enables `render(&app.ui, &app.session, frame)` while event handlers take `(&mut app.ui, &mut app.session)` — no borrow checker conflicts
- All module functions are **free functions** (not struct methods) that take explicit sub-struct references
- Event loop: drain channels → handle events → render if dirty
- `dirty: bool` initializes to `true` so the first frame renders immediately
- Startup, shutdown, `seed_references()`

### event_handler.rs (~300 lines) — Input dispatch
- `handle_key_event(app, key)` — global keybindings, then focus-specific routing
- `handle_mouse_event(app, mouse)` — click-to-focus, scroll-on-hover
- `handle_paste_event(app, text)` — bracketed paste, file path extraction
- Stores panel `Rect`s from last render for mouse hit-testing

### session_manager.rs (~300 lines) — Session lifecycle
- `SessionManager` struct owns: `active_dir`, `active_name`, `project_idx`, `phase_session`, `conversations: HashMap<String, Vec<ConversationEntry>>`
- Single format: PhaseSession only. LegacySession deleted.
- Methods: `create()`, `load()`, `save()`, `switch()`, `close()`
- Conversation ownership: session manager is the source of truth. ConversationPane borrows `&[ConversationEntry]` from session during render (split borrow via App sub-structs).
- `add_message(phase, role, content)` appends to the HashMap but does NOT auto-save. System messages are frequent (5-10 during session load) and would cause I/O jank.
- Auto-save triggers: explicit `save()` calls after user prompt submission, Claude response completion, exit, and phase transition. Never on system messages.

### phase_dispatch.rs (~500 lines) — Phase-specific handlers
- `dispatch_prompt(app, phase, text, images)` — routes to per-phase sender
- `handle_phase_response(app, phase, response)` — routes to per-phase handler
- `send_spec_prompt()`, `handle_spec_response()` — with reference detection
- `send_decompose_prompt()`, `handle_decompose_response()`
- Component, Assembly, Refinement handlers
- Phase transition validation (user-initiated only)

### claude_bridge.rs (~200 lines) — Claude CLI interaction
- `ClaudeBridge` struct owns: `bg_tx/rx`, `stream_tx/rx`, `bg_pid`, `claude_model`, `claude_session_id`, `streaming_text`, `busy: BusyState`
- `send(system_prompt, prompt, images, phase_name, ref_context)` — spawns thread, returns immediately
- `drain_streaming()` — drains `stream_rx`, returns `(text_chunks, had_data)`
- `try_recv_result()` — non-blocking check for `BackgroundResult`
- `cancel()` — SIGTERM to bg_pid
- `is_busy()`, `streaming_text()` accessors

### render.rs (~200 lines) — Render orchestration
- `dirty: bool` flag on App — set by any state change, cleared after render
- `render(app, frame)` — delegates to panel widgets, stores panel Rects for hit-testing
- `panel_rects: PanelRects` — struct holding Rect for each panel (used by mouse handler)
- Legend bar, phase indicator, usage stats
- Spinner animation capped at 10fps

## Event Loop

```
loop {
    // 1. Drain streaming chunks (non-blocking batch)
    let (chunks, had_stream) = claude.drain_streaming();
    if had_stream { dirty = true; }

    // 2. Check for background results
    if let Some(result) = claude.try_recv_result() {
        handle_bg_result(result);
        dirty = true;
    }

    // 3. Render only if dirty
    if dirty {
        terminal.draw(|f| render(app, f));
        dirty = false;
    }

    // 4. Poll for events (50ms timeout)
    if crossterm::event::poll(Duration::from_millis(50))? {
        match crossterm::event::read()? {
            Event::Key(key) => { handle_key_event(app, key); dirty = true; }
            Event::Mouse(mouse) => { handle_mouse_event(app, mouse); dirty = true; }
            Event::Paste(text) => { handle_paste_event(app, text); dirty = true; }
            _ => {}
        }
    }

    // 5. Advance spinner (only when busy, capped at 10fps)
    if claude.is_busy() && tick_count % 5 == 0 {
        spinner_frame += 1;
        dirty = true;
    }
    tick_count += 1;

    if app.should_quit { break; }
}
```

Key difference from current: render only happens when `dirty == true`. When idle, CPU usage drops to near zero. During streaming, chunks are batched before a single render.

## Mouse Support

Enable `crossterm::event::EnableMouseCapture` on terminal init, `DisableMouseCapture` on cleanup.

### Click-to-focus
- `MouseEventKind::Down(MouseButton::Left)` triggers hit-test against `panel_rects`
- If click lands in project tree → `focus = ProjectTree`
- If click lands in conversation → `focus = Conversation`
- If click lands in right panel → `focus = RightPanel`
- If click lands in input → `focus = Input`

### Scroll-on-hover
- `MouseEventKind::ScrollUp/ScrollDown` triggers hit-test against `panel_rects`
- Scrolls whichever panel the cursor is over, regardless of current focus
- Works for conversation, right panel, and project tree

### Panel hit-testing
`PanelRects` is populated during each render call. The mouse handler uses it to determine which panel a coordinate falls in. No per-widget click handling (no clickable buttons/items) — just focus and scroll.

Note: after a terminal resize, PanelRects is one frame stale (~50ms). This is acceptable — the next render updates them. No premature layout recalculation needed.

## Session Manager

### Single format
Delete `LegacySession`, `Session`, `SessionData`, `is_legacy_session_json()`, `session_status()`, `load_session()` (legacy path). Any existing legacy `session.json` files simply won't load.

### Conversation ownership
Currently conversations are duplicated:
- `ConversationPane.entries: Vec<ConversationEntry>` (UI)
- `PhaseSession.conversations: HashMap<String, Vec<ConversationEntry>>` (storage)
- Manual sync via `sync_conversations_to_phase_session()` on key events

New design:
- `SessionManager.conversations: HashMap<String, Vec<ConversationEntry>>` is the source of truth
- `ConversationPane` receives `&[ConversationEntry]` during render — it's a view, not an owner
- `session_manager.add_message(phase, role, content)` writes to the HashMap AND triggers auto-save
- No sync needed — one write path

### ConversationEntry
Reuse the existing `storage::session::ConversationEntry { role: String, content: String }` as the canonical type. The TUI's `tui::conversation::ConversationEntry` is deleted — the render function takes the storage type directly.

### Auto-save triggers
- `session_manager.add_message()` — saves after each message
- `session_manager.save()` — explicit save (phase transition, exit)
- On exit: `session_manager.save()` called before cleanup

### Session resume
When loading a session, `SessionManager::load()` reads `session.json`, restores phase, conversations, component states, spec. The conversation pane renders directly from `session_manager.conversations[current_phase]`.

### Session metadata for project tree
Replace `session_status()` (which reads legacy `SessionData`) with a new `phase_session_status()` that reads `PhaseSessionData` and returns phase name and message count for display in the project tree.

### Build orchestration
After deleting LegacySession, build mechanics (temp directory, `python::build()`, iteration tracking, STL management) move to `phase_dispatch.rs`. The `SessionManager` provides the session directory; `phase_dispatch` handles:
- Creating temp build dirs
- Calling `python::build()`
- Recording iterations on `ComponentState`
- Updating `working.stl` / `working.step`

## Visual Improvements

### Conversation panel
- **System messages as compact banners**: single-line gray background with `ⓘ` prefix, not full conversation entries. Examples: "Loaded 6 references: NEMA23, DM556-S, ...", "Detected component: NEMA23"
- **Markdown rendering** (minimal viable set): bold (`**text**` → bold style), bullet lists (`- item` → indented), inline code (`` `code` `` → dim/colored style). Tables deferred to later iteration — they require column width calculation that's complex in proportional terminals. Use an existing crate if available (e.g. `tui-markdown`), otherwise implement with regex.
- **Reference results summarized**: instead of raw TOML dump, show "Saved reference 'NEMA23' (57.2×57.2×56mm body, 6.35mm shaft)" one-liner. Full details in Refs tab.

### Right panel tabs
The right panel becomes a tabbed container with 3 tabs:

**Spec tab** — structured key-value display extracted from the spec conversation. Categories: PURPOSE, DIMENSIONS, COMPONENTS, CONSTRAINTS. Updated as spec Q&A progresses.

**Refs tab** — active reference summaries. Shows each loaded reference with name, key dimensions, and category. Scrollable list.

**Model tab** — build output. Dimensions, feature list, STL path, braille preview. Same content as current ModelPanel but in a tab.

All tabs are focusable (keyboard: Tab cycles to right panel, then left/right arrow switches tabs). All tabs are scrollable (j/k when focused, mouse wheel on hover).

### Focus system
Currently 3 focus targets: Input, ProjectTree, Conversation. Add RightPanel as a 4th:

```rust
enum Focus {
    Input,
    ProjectTree,
    Conversation,
    RightPanel,
}
```

Tab cycles forward: Input → Conversation → RightPanel → ProjectTree → Input.
Shift+Tab cycles reverse: Input → ProjectTree → RightPanel → Conversation → Input.
Mouse click sets focus directly.
Esc always returns to Input.

## Input Field

### Newline fix
Current `\` + Return handler strips the backslash but doesn't insert newline. Fix: detect trailing `\` in the tui-textarea buffer, remove it, insert `\n`. Keep Ctrl+Enter as alternative.

### Multi-line expansion
When input buffer contains newlines, the input bar height grows from 3 to up to 7 lines (5 content + 2 border). Conversation area shrinks proportionally. Layout recomputed via `compute_layout()` with dynamic input height.

### Phase-aware placeholder
Show contextual placeholder text in the input when empty:
- Spec: "Describe what you want to build..."
- Decompose: "Describe changes to the component tree..."
- Component: "Feedback, 'approve', or 'undo'..."
- Assembly: "Assembly instructions or feedback..."
- Refinement: "Parameter changes or feedback..."

### Command hints
When input starts with `/`, show a single-line hint above the input bar:
- `/ref <name>` — research or load a reference
- `/ref list` — show all references
- `/ref remove <name>` — remove a reference

## Multi-/ref Parsing

### Split multiple commands
When input contains multiple `/ref` tokens, split on the `/ref` boundary:
```
"/ref nema23, /ref Arduino Nano, /ref DM556-S"
→ ["nema23", "Arduino Nano", "DM556-S"]
```

### Batch processing
Process sequentially via a queue. For research (new components), spawn one Claude call that researches ALL components in a single prompt (more efficient than 6 separate calls).

### Batch save
Instead of per-item yes/no confirmation, show:
```
Found 6 components: NEMA23, Arduino Nano, DM556-S, AC-DC2412, LM2596, DS3231
Save all? (yes / no / pick)
```
- `yes` — save all to library
- `no` — discard all
- `pick` — show numbered list, user types space-separated numbers to keep (e.g. `1 3 5`). This reuses the existing modal input pattern (`ref_confirm_pending` takes precedence over normal input dispatch, same as `delete_pending`, `rename_pending` etc.)

### Results routing
Reference research results go to the Refs tab in the right panel. Conversation shows only a compact banner: "Saved 6 references to ~/MiModel/references/"

## Code Organization

### New files
- `src/event_handler.rs` — key, mouse, paste event handling
- `src/session_manager.rs` — SessionManager struct, PhaseSession lifecycle
- `src/phase_dispatch.rs` — per-phase prompt/response handlers
- `src/claude_bridge.rs` — ClaudeBridge struct, thread management
- `src/render.rs` — render orchestration, dirty tracking, PanelRects
- `src/tui/right_panel.rs` — tabbed right panel widget (Spec/Refs/Model)

### Modified files
- `src/main.rs` — slimmed to App shell (~400 lines)
- `src/tui/mod.rs` — add Focus::RightPanel, update BackgroundResult
- `src/tui/conversation.rs` — accept `&[ConversationEntry]` from session manager, add markdown rendering (bold, tables), compact system messages
- `src/tui/input_bar.rs` — fix newline, multi-line expansion, placeholders, command hints
- `src/tui/layout.rs` — add `input_height: u16` field to `LayoutConfig` (default 3, max 7). `compute_layout()` uses this to size the input area dynamically. Right panel rendered as tab container.

### Deleted files/code
- `LegacySession` / `Session` struct and all methods in `model_session.rs`
- `is_legacy_session_json()` in `storage/session.rs`
- `SessionData` / `session_status()` legacy types
- `load_session()` legacy path in main.rs
- `sync_conversations_to_phase_session()` in main.rs
- `tui::conversation::ConversationEntry` (replaced by storage type)
