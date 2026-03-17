# Phase Enforcement via MCP Tools

**Date:** 2026-03-17
**Status:** Design

## Problem

Claude generates code and builds models during Spec phase despite prompt instructions and post-hoc code-block stripping. The system auto-advances phases without user consent. These are prompt-based rails that Claude can ignore. The result: dramatically lower output quality because the careful phase-by-phase workflow gets bypassed.

## Solution

Replace prompt-based rails with structural enforcement using MCP tool-use. Each phase exposes only the tools appropriate for that phase. Claude literally cannot generate a build during Spec because no build tool exists. All actions go through structured tool calls that the app controls.

## Architecture

```
User types prompt
  → Rust app spawns: claude --tools "" --strict-mcp-config --mcp-config <phase>.json
  → Claude CLI spawns: python3 mcp/server.py --phase <phase> --session-dir <dir>
  → Claude can ONLY call tools defined for that phase
  → MCP server executes tool calls (builds for Component phase, acknowledgments for others)
  → Rust app parses tool_use blocks from stream-json output
  → Rust app updates UI (spec panel, viewer, conversation)
```

Key flags:
- `--tools ""` — disables most built-in Claude tools (no Bash, Edit, Read, Write). LSP may persist; add `--disallowedTools LSP` to fully lock down.
- `--strict-mcp-config` — only loads our MCP server, ignores all other MCP configs
- `--mcp-config <file>` — loads our phase-specific server config

**Verified by testing:** stream-json output DOES include `tool_use` content blocks with `name` and `input` fields. The Rust app can parse these directly. MCP tool names are prefixed as `mcp__<server>__<tool>` (e.g. `mcp__mimodel__record_spec_field`).

**`--dangerously-skip-permissions`:** Keep this flag. With `--tools ""` disabling built-in tools and `--strict-mcp-config` limiting to our server, there are no permissions to gate. The flag prevents any residual permission prompts.

## Tool Definitions Per Phase

### Spec Phase

| Tool | Parameters | Description |
|------|-----------|-------------|
| `ask_question` | `question: str` | Ask the user one clarifying question |
| `record_spec_field` | `category: str, key: str, value: str, unit: str` | Record a spec data point. Category: "dimension", "constraint", "feature", "component" |
| `mark_spec_complete` | (none) | Signal that the specification is complete |

### Decompose Phase

| Tool | Parameters | Description |
|------|-----------|-------------|
| `ask_clarification` | `question: str` | Ask about decomposition |
| `propose_component_tree` | `components: [{id, name, depends_on, assembly_op}]` | Submit a structured component tree for user review (JSON array, not TOML string — avoids TOML-in-JSON serialization issues) |

### Component Phase

| Tool | Parameters | Description |
|------|-----------|-------------|
| `ask_clarification` | `question: str` | Discuss the component |
| `submit_cadquery_code` | `component_id: str, code: str` | Submit CadQuery code. Server builds it, returns dimensions or error. App opens result in f3d. |
| `request_approval` | `component_id: str, summary: str` | After successful build, ask user to approve or give feedback |

### Assembly Phase

| Tool | Parameters | Description |
|------|-----------|-------------|
| `ask_clarification` | `question: str` | Discuss assembly |
| `submit_assembly_code` | `code: str` | Submit assembly script. Server builds, returns result. |

### Refinement Phase

| Tool | Parameters | Description |
|------|-----------|-------------|
| `ask_clarification` | `question: str` | Discuss refinement |
| `update_parameter` | `name: str, old_value: str, new_value: str` | Tweak a parameter value |
| `submit_code_patch` | `code: str` | Submit modified code. Server builds, returns result. |

## MCP Server

### Single Python script: `mcp/server.py`

Takes `--phase` and `--session-dir` arguments. Implements the MCP protocol over stdin/stdout. Exposes only the tools for the specified phase.

**For non-build tools** (`ask_question`, `record_spec_field`, `mark_spec_complete`, `propose_component_tree`, `request_approval`, `update_parameter`): the server returns a meaningful response so Claude has context for its next action. Examples:
- `record_spec_field` returns all fields recorded so far (so Claude knows when the spec is complete)
- `propose_component_tree` returns "Proposed N components. Awaiting user review."
- `mark_spec_complete` returns "Spec marked complete. Awaiting user confirmation to advance."
- `update_parameter` returns the full current parameter set

The Rust app also parses the tool_use block from stream-json to extract the structured data and update the UI.

**For build tools** (`submit_cadquery_code`, `submit_assembly_code`, `submit_code_patch`): the server executes the build directly in Python:
1. Write code to a temp file in the session directory
2. Import and run CadQuery
3. Export STL and STEP to the session directory (working.stl, working.step)
4. Return build result: success with dimensions (bounding box, feature list) or error with traceback
5. Claude sees the result and can iterate or request approval

Build output paths:
- Component: `<session_dir>/components/<component_id>/code.py`, `result.stl`, `result.step`
- Assembly: `<session_dir>/assembly/assembly.py`, `working.stl`, `working.step`

Build state signaling: the MCP server writes `<session_dir>/.building` before starting a CadQuery build and deletes it after completion. The Rust app polls for this file to switch from `BusyState::Thinking` to `BusyState::Building` during builds.

Assembly context: for `submit_assembly_code`, the MCP server scans `<session_dir>/components/*/result.stl` to discover approved component artifacts. No manifest file needed — convention-based directory scan.

### Component build-review cycle

```
1. Claude calls submit_cadquery_code(component_id, code)
2. MCP server builds → writes STL to session dir → returns dimensions or error
3. Rust app detects new STL (via tool_use parse) → refreshes f3d viewer
4. If success: Claude calls request_approval(component_id, summary)
5. User sees model in f3d, types feedback or "approve"
6. If feedback: Claude iterates with another submit_cadquery_code
7. If approve: app records component as done, moves to next
```

### MCP protocol implementation

Use the `mcp` Python package (pip installable) for protocol handling. The server implements:
- `list_tools` — returns tool schemas for the current phase
- `call_tool` — executes the tool and returns result

Minimal dependencies: `mcp` package, `cadquery` (already installed for builds).

## MCP Config Files

Stored in repo at `mcp/`:

```
mcp/
  server.py              — MCP server script
  requirements.txt       — mcp package dependency
```

Config is generated at runtime by the Rust app. For each Claude call, the app writes a temp JSON file:

```json
{
  "mcpServers": {
    "mimodel": {
      "command": "python3",
      "args": ["/path/to/mcp/server.py", "--phase", "spec", "--session-dir", "/path/to/session"],
      "env": {}
    }
  }
}
```

The path to `server.py` is resolved relative to the mimodel binary (same as `prompts/` directory resolution).

Session continuity: the MCP config uses the same `--session-dir` across resumed calls. The tool list must remain stable within a resumed Claude session. If the phase changes (and thus the tool list changes), the Rust app resets `claude_session_id` to start a fresh Claude session.

## Changes to Rust App

### claude.rs — Add MCP flags to send_prompt

Add parameters to `send_prompt`:
- `mcp_config_path: Option<&Path>` — path to generated MCP config JSON
- `disable_builtin_tools: bool` — when true, adds `--tools ""`  and `--strict-mcp-config`

When both are set:
```rust
cmd.arg("--tools").arg("");
cmd.arg("--strict-mcp-config");
cmd.arg("--mcp-config").arg(mcp_config_path);
```

### claude.rs — Parse tool_use blocks from stream-json

Extend the stream-json parser to extract tool_use blocks alongside text:

```rust
for block in content_array {
    match block.get("type").and_then(|t| t.as_str()) {
        Some("text") => { /* existing text streaming */ }
        Some("tool_use") => {
            let tool_call = ToolCall {
                name: block.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
                input: block.get("input").cloned().unwrap_or(serde_json::Value::Null),
            };
            if let Some(tx) = tool_tx {
                let _ = tx.send(tool_call);
            }
        }
        _ => {}
    }
}
```

New channel: `tool_tx/tool_rx` on `ClaudeBridge` for structured tool calls.

### claude_bridge.rs — Add tool channel

```rust
pub struct ClaudeBridge {
    // ... existing fields
    pub tool_tx: Sender<ToolCall>,
    pub tool_rx: Receiver<ToolCall>,
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub input: serde_json::Value,
}
```

### claude_bridge.rs — Generate MCP config

Add method `generate_mcp_config(&self, phase: Phase, session_dir: &Path) -> PathBuf` that writes the temp JSON file and returns its path.

### main.rs — Unified tool dispatch

Replace per-phase response handlers with a single tool dispatch:

```rust
fn handle_tool_call(&mut self, tool: ToolCall) {
    match tool.name.as_str() {
        "ask_question" | "ask_clarification" => {
            if let Some(q) = tool.input.get("question").and_then(|v| v.as_str()) {
                self.conversation.add("assistant", q);
            }
        }
        "record_spec_field" => {
            let category = tool.input.get("category").and_then(|v| v.as_str()).unwrap_or("");
            let key = tool.input.get("key").and_then(|v| v.as_str()).unwrap_or("");
            let value = tool.input.get("value").and_then(|v| v.as_str()).unwrap_or("");
            let unit = tool.input.get("unit").and_then(|v| v.as_str()).unwrap_or("");
            // Update spec panel with structured data
            self.right_panel.append_spec(&format!("{}: {} = {} {}", category, key, value, unit));
        }
        "mark_spec_complete" => {
            self.conversation.add("system", "Spec complete. Type 'advance' to move to Decompose phase.");
            // Do NOT auto-transition — wait for user
        }
        "propose_component_tree" => {
            if let Some(toml) = tool.input.get("toml").and_then(|v| v.as_str()) {
                self.parse_and_display_components(toml);
                self.conversation.add("system", "Component tree proposed. Type 'approve' to accept, or describe changes.");
            }
        }
        "submit_cadquery_code" | "submit_assembly_code" | "submit_code_patch" => {
            // Build already happened in MCP server — detect new STL and refresh viewer
            if let Some(ref dir) = self.session.active_dir {
                let working_stl = dir.join("working.stl");
                if working_stl.exists() {
                    let _ = self.viewer.update_working_stl(&working_stl);
                    if !self.viewer.is_running() {
                        let _ = self.viewer.show();
                    }
                }
            }
        }
        "request_approval" => {
            if let Some(summary) = tool.input.get("summary").and_then(|v| v.as_str()) {
                self.conversation.add("system", &format!("Review model in viewer. {summary}\nType 'approve' or describe changes."));
            }
        }
        "update_parameter" => {
            let name = tool.input.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let new_val = tool.input.get("new_value").and_then(|v| v.as_str()).unwrap_or("");
            self.right_panel.append_spec(&format!("Updated: {} = {}", name, new_val));
        }
        _ => {} // Unknown tool — ignore
    }
}
```

### Event loop — Drain tool channel

In the main event loop, alongside streaming text drain and bg result check:

```rust
while let Ok(tool_call) = app.claude.tool_rx.try_recv() {
    app.handle_tool_call(tool_call);
    app.dirty = true;
}
```

### Phase transitions — User-initiated only

`mark_spec_complete` shows a message but does NOT transition. The user must type `advance` (or a specific command). Same pattern for all phase boundaries:
- Spec → Decompose: user types `advance` after `mark_spec_complete`
- Decompose → Component: user types `approve` after reviewing component tree
- Component → Assembly: all components approved
- Assembly → Refinement: user types `advance` after reviewing assembly

## What Gets Deleted

- `parser::parse_response` code-block extraction — no longer needed (tools enforce structure)
- Code-block stripping in `handle_spec_response` / `handle_decompose_response` — replaced by tool-only actions
- Per-phase send methods can be unified into one `send_phase_prompt` that just varies the MCP config
- `SPEC_COMPLETE` text detection — replaced by `mark_spec_complete` tool
- TOML extraction from freeform response — replaced by `propose_component_tree` tool

## What Stays

- Freeform text rendering in conversation — Claude still outputs explanatory text
- Reference detection in spec responses — runs on Claude's freeform text
- `/ref` command — orthogonal to phase tools
- Session manager — unchanged
- Right panel tabs — spec tab now populated by `record_spec_field` tool calls instead of raw text append

## File Structure

### New files
- `mcp/server.py` — MCP server implementing all phase tools
- `mcp/requirements.txt` — `mcp` package dependency

### Modified files
- `src/claude.rs` — add MCP config flags, parse tool_use blocks
- `src/claude_bridge.rs` — add tool channel, MCP config generation, ToolCall struct
- `src/main.rs` — unified `handle_tool_call` dispatch, remove per-phase response handlers, tool channel drain in event loop
- `prompts/spec.md` (and other phase prompts) — update to describe available tools instead of freeform output instructions

### Deleted code
- Code-block stripping in response handlers
- `SPEC_COMPLETE` text detection
- `parser::parse_response` for code extraction (may keep for backward compat but not called from phase handlers)
- Per-phase send method boilerplate (unified into one path)
