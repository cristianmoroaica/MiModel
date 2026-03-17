# Phase Enforcement via MCP Tools Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace prompt-based phase rails with structural enforcement using per-phase MCP tools. Claude can only perform actions through scoped tools — no code generation outside Component phase.

**Architecture:** A Python MCP server (`mcp/server.py`) implements phase-scoped tools over stdin/stdout JSON-RPC. The Rust app generates per-phase MCP config at runtime, spawns Claude CLI with `--tools "" --strict-mcp-config --mcp-config`, and parses `tool_use` blocks from stream-json output to update UI state. Build tools run CadQuery directly in the MCP server process.

**Tech Stack:** Rust (ratatui TUI), Python (MCP server + CadQuery builds), Claude CLI with MCP support

**Spec:** `docs/superpowers/specs/2026-03-17-phase-enforcement-mcp-tools-design.md`

---

## Chunk 1: MCP Server

### Task 1: Create the MCP server with Spec phase tools

**Files:**
- Create: `mcp/server.py`

The MCP server implements JSON-RPC over stdin/stdout. No external Python dependencies — just the standard library + cadquery (already installed for builds).

- [ ] **Step 1: Create mcp/ directory and server.py skeleton**

```python
#!/usr/bin/env python3
"""MiModel MCP server — per-phase tool definitions for Claude CLI."""
import argparse
import json
import sys
import os

# ── JSON-RPC helpers ──

def send_response(id, result):
    msg = json.dumps({"jsonrpc": "2.0", "id": id, "result": result})
    sys.stdout.write(msg + "\n")
    sys.stdout.flush()

def send_error(id, code, message):
    msg = json.dumps({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
    sys.stdout.write(msg + "\n")
    sys.stdout.flush()

# ── Tool definitions per phase ──

SPEC_TOOLS = [
    {
        "name": "ask_question",
        "description": "Ask the user one clarifying question about their design requirements.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "question": {"type": "string", "description": "The question to ask"}
            },
            "required": ["question"]
        }
    },
    {
        "name": "record_spec_field",
        "description": "Record a specification data point. Call this for each dimension, constraint, feature, or component reference discovered.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "category": {"type": "string", "enum": ["dimension", "constraint", "feature", "component"], "description": "Category of the spec field"},
                "key": {"type": "string", "description": "Field name (e.g. 'case_diameter')"},
                "value": {"type": "string", "description": "Field value (e.g. '38.0')"},
                "unit": {"type": "string", "description": "Unit (e.g. 'mm', 'degrees', 'count')"}
            },
            "required": ["category", "key", "value"]
        }
    },
    {
        "name": "mark_spec_complete",
        "description": "Signal that the specification is complete and ready for decomposition. Only call this when all necessary dimensions, constraints, and features have been recorded.",
        "inputSchema": {
            "type": "object",
            "properties": {},
            "required": []
        }
    },
]

DECOMPOSE_TOOLS = [
    {
        "name": "ask_clarification",
        "description": "Ask the user a clarifying question about the component decomposition.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "question": {"type": "string", "description": "The question to ask"}
            },
            "required": ["question"]
        }
    },
    {
        "name": "propose_component_tree",
        "description": "Submit a component decomposition for the user to review.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "components": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "string"},
                            "name": {"type": "string"},
                            "description": {"type": "string"},
                            "depends_on": {"type": "array", "items": {"type": "string"}},
                            "assembly_op": {"type": "string", "enum": ["base", "union", "cut", "intersect"]}
                        },
                        "required": ["id", "name", "assembly_op"]
                    },
                    "description": "List of components with dependencies"
                }
            },
            "required": ["components"]
        }
    },
]

COMPONENT_TOOLS = [
    {
        "name": "ask_clarification",
        "description": "Ask the user a clarifying question about the current component.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "question": {"type": "string", "description": "The question to ask"}
            },
            "required": ["question"]
        }
    },
    {
        "name": "submit_cadquery_code",
        "description": "Submit CadQuery Python code for a component. The code will be built and the result displayed in the 3D viewer. All tunable parameters should be UPPERCASE constants at the top.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "component_id": {"type": "string", "description": "ID of the component being built"},
                "code": {"type": "string", "description": "Complete CadQuery Python code. Must assign final shape to 'result' variable."}
            },
            "required": ["component_id", "code"]
        }
    },
    {
        "name": "request_approval",
        "description": "After a successful build, ask the user to approve the component or provide feedback.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "component_id": {"type": "string", "description": "ID of the component"},
                "summary": {"type": "string", "description": "Brief summary of what was built"}
            },
            "required": ["component_id", "summary"]
        }
    },
]

ASSEMBLY_TOOLS = [
    {
        "name": "ask_clarification",
        "description": "Ask the user a clarifying question about the assembly.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "question": {"type": "string", "description": "The question to ask"}
            },
            "required": ["question"]
        }
    },
    {
        "name": "submit_assembly_code",
        "description": "Submit CadQuery assembly code that combines approved components.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "code": {"type": "string", "description": "CadQuery assembly code. Component STEPs are in components/<id>/result.step"}
            },
            "required": ["code"]
        }
    },
]

REFINEMENT_TOOLS = [
    {
        "name": "ask_clarification",
        "description": "Ask the user a clarifying question about the refinement.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "question": {"type": "string", "description": "The question to ask"}
            },
            "required": ["question"]
        }
    },
    {
        "name": "update_parameter",
        "description": "Update a parameter value in the current model.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Parameter name"},
                "old_value": {"type": "string", "description": "Current value"},
                "new_value": {"type": "string", "description": "New value"}
            },
            "required": ["name", "old_value", "new_value"]
        }
    },
    {
        "name": "submit_code_patch",
        "description": "Submit modified CadQuery code with parameter changes applied.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "code": {"type": "string", "description": "Updated CadQuery code"}
            },
            "required": ["code"]
        }
    },
]

PHASE_TOOLS = {
    "spec": SPEC_TOOLS,
    "decompose": DECOMPOSE_TOOLS,
    "component": COMPONENT_TOOLS,
    "assembly": ASSEMBLY_TOOLS,
    "refinement": REFINEMENT_TOOLS,
}

# ── Spec field accumulator ──

spec_fields = []

# ── Build helper ──

def run_cadquery_build(code, output_dir, label="build"):
    """Execute CadQuery code and export STL+STEP. Returns result dict."""
    os.makedirs(output_dir, exist_ok=True)
    code_path = os.path.join(output_dir, "code.py")
    stl_path = os.path.join(output_dir, "result.stl")
    step_path = os.path.join(output_dir, "result.step")

    # Write .building signal for the Rust app
    building_flag = os.path.join(session_dir, ".building") if session_dir else None
    if building_flag:
        open(building_flag, "w").close()

    try:
        # Write code to file
        with open(code_path, "w") as f:
            f.write(code)

        # Execute in a subprocess to isolate crashes
        import subprocess
        export_code = code + f"""
import cadquery as cq
cq.exporters.export(result, "{stl_path}")
cq.exporters.export(result, "{step_path}")

# Print bounding box
bb = result.val().BoundingBox()
print(f"DIMS:{{bb.xlen:.2f}}x{{bb.ylen:.2f}}x{{bb.zlen:.2f}}")
"""
        proc = subprocess.run(
            [sys.executable, "-c", export_code],
            capture_output=True, text=True, timeout=60
        )

        if proc.returncode != 0:
            return {"success": False, "error": proc.stderr[-2000:] if proc.stderr else "Unknown build error"}

        dims = "unknown"
        for line in proc.stdout.splitlines():
            if line.startswith("DIMS:"):
                dims = line[5:]

        # Also copy to working.stl/working.step in session root
        if session_dir:
            import shutil
            working_stl = os.path.join(session_dir, "working.stl")
            working_step = os.path.join(session_dir, "working.step")
            if os.path.exists(stl_path):
                shutil.copy2(stl_path, working_stl)
            if os.path.exists(step_path):
                shutil.copy2(step_path, working_step)

        return {"success": True, "dimensions": dims, "stl_path": stl_path, "step_path": step_path}

    except subprocess.TimeoutExpired:
        return {"success": False, "error": "Build timed out after 60 seconds"}
    except Exception as e:
        return {"success": False, "error": str(e)}
    finally:
        if building_flag and os.path.exists(building_flag):
            os.remove(building_flag)

# ── Tool call handlers ──

def handle_tool_call(name, arguments):
    """Execute a tool call and return the MCP result content."""

    # Questions — just acknowledge, Rust app reads tool_use from stream
    if name in ("ask_question", "ask_clarification"):
        q = arguments.get("question", "")
        return [{"type": "text", "text": f"Question delivered to user: {q}"}]

    # Record spec field
    if name == "record_spec_field":
        category = arguments.get("category", "")
        key = arguments.get("key", "")
        value = arguments.get("value", "")
        unit = arguments.get("unit", "")
        spec_fields.append({"category": category, "key": key, "value": value, "unit": unit})
        # Return all fields so Claude can track completeness
        summary = "\n".join(f"  [{f['category']}] {f['key']} = {f['value']} {f['unit']}" for f in spec_fields)
        return [{"type": "text", "text": f"Recorded. Current spec ({len(spec_fields)} fields):\n{summary}"}]

    # Mark spec complete
    if name == "mark_spec_complete":
        summary = "\n".join(f"  [{f['category']}] {f['key']} = {f['value']} {f['unit']}" for f in spec_fields)
        return [{"type": "text", "text": f"Spec marked complete with {len(spec_fields)} fields. Awaiting user confirmation to advance.\n{summary}"}]

    # Propose component tree
    if name == "propose_component_tree":
        components = arguments.get("components", [])
        tree_str = "\n".join(f"  {c['id']}: {c.get('name', c['id'])} [{c.get('assembly_op', 'union')}]" for c in components)
        return [{"type": "text", "text": f"Component tree proposed ({len(components)} components). Awaiting user review.\n{tree_str}"}]

    # Submit code (component, assembly, refinement)
    if name == "submit_cadquery_code":
        component_id = arguments.get("component_id", "unknown")
        code = arguments.get("code", "")
        output_dir = os.path.join(session_dir, "components", component_id) if session_dir else "/tmp/mimodel_build"
        result = run_cadquery_build(code, output_dir, label=component_id)
        if result["success"]:
            return [{"type": "text", "text": f"Build successful! Dimensions: {result['dimensions']}mm. Model displayed in 3D viewer. Call request_approval when ready."}]
        else:
            return [{"type": "text", "text": f"Build failed:\n{result['error']}\n\nFix the code and try again."}]

    if name == "submit_assembly_code":
        code = arguments.get("code", "")
        output_dir = os.path.join(session_dir, "assembly") if session_dir else "/tmp/mimodel_build"
        result = run_cadquery_build(code, output_dir, label="assembly")
        if result["success"]:
            return [{"type": "text", "text": f"Assembly built! Dimensions: {result['dimensions']}mm. Model displayed in viewer."}]
        else:
            return [{"type": "text", "text": f"Assembly build failed:\n{result['error']}"}]

    if name == "submit_code_patch":
        code = arguments.get("code", "")
        output_dir = os.path.join(session_dir, "refinement") if session_dir else "/tmp/mimodel_build"
        result = run_cadquery_build(code, output_dir, label="refinement")
        if result["success"]:
            return [{"type": "text", "text": f"Refinement built! Dimensions: {result['dimensions']}mm."}]
        else:
            return [{"type": "text", "text": f"Refinement build failed:\n{result['error']}"}]

    # Request approval
    if name == "request_approval":
        summary = arguments.get("summary", "")
        return [{"type": "text", "text": f"Approval requested. User reviewing model. Summary: {summary}"}]

    # Update parameter
    if name == "update_parameter":
        name_p = arguments.get("name", "")
        old = arguments.get("old_value", "")
        new = arguments.get("new_value", "")
        return [{"type": "text", "text": f"Parameter updated: {name_p} changed from {old} to {new}"}]

    return [{"type": "text", "text": f"Unknown tool: {name}"}]

# ── Main loop ──

session_dir = None

def main():
    global session_dir
    parser = argparse.ArgumentParser()
    parser.add_argument("--phase", required=True, choices=PHASE_TOOLS.keys())
    parser.add_argument("--session-dir", default=None)
    args = parser.parse_args()
    session_dir = args.session_dir

    tools = PHASE_TOOLS[args.phase]

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except json.JSONDecodeError:
            continue

        method = req.get("method", "")
        id = req.get("id")

        if method == "initialize":
            send_response(id, {
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "mimodel", "version": "0.1.0"}
            })
        elif method == "notifications/initialized":
            pass
        elif method == "tools/list":
            send_response(id, {"tools": tools})
        elif method == "tools/call":
            tool_name = req.get("params", {}).get("name", "")
            arguments = req.get("params", {}).get("arguments", {})
            content = handle_tool_call(tool_name, arguments)
            send_response(id, {"content": content})
        else:
            if id is not None:
                send_response(id, {})

if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Test the MCP server manually**

Run: `echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | python3 mcp/server.py --phase spec`
Expected: JSON response with protocolVersion

- [ ] **Step 3: Test with Claude CLI**

```bash
cat > /tmp/mimodel_test_mcp.json << 'EOF'
{"mcpServers":{"mimodel":{"command":"python3","args":["mcp/server.py","--phase","spec","--session-dir","/tmp/mimodel_test"]}}}
EOF
timeout 30 claude --tools "" --strict-mcp-config --mcp-config /tmp/mimodel_test_mcp.json \
  --dangerously-skip-permissions --disallowedTools LSP \
  --output-format stream-json --model haiku \
  -p "I want to design a box that is 100x80x50mm with 2mm walls" 2>/dev/null | \
  python3 -c "
import sys, json
for line in sys.stdin:
    try:
        obj = json.loads(line.strip())
        t = obj.get('type','')
        if t == 'assistant':
            for b in obj.get('message',{}).get('content',[]):
                bt = b.get('type','')
                if bt == 'text': print(f'TEXT: {b[\"text\"][:150]}')
                elif bt == 'tool_use': print(f'TOOL: {b[\"name\"]} → {json.dumps(b[\"input\"])[:200]}')
        elif t == 'result': print(f'RESULT: {obj[\"result\"][:200]}')
    except: pass
"
```

Expected: Claude calls `ask_question` or `record_spec_field` tools, NOT freeform code.

- [ ] **Step 4: Commit**

```bash
git add mcp/
git commit -m "feat(mcp): create phase-scoped MCP server with all tool definitions"
```

## Chunk 2: Rust-Side Tool Parsing

### Task 2: Add tool_use parsing to stream-json handler

**Files:**
- Modify: `src/claude.rs`
- Modify: `src/claude_bridge.rs`

- [ ] **Step 1: Add ToolCall struct to claude_bridge.rs**

```rust
/// A structured tool call parsed from Claude's stream-json output.
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub input: serde_json::Value,
}
```

Add a tool channel to ClaudeBridge:

```rust
pub struct ClaudeBridge {
    // ... existing fields
    tool_tx: mpsc::Sender<ToolCall>,
    tool_rx: mpsc::Receiver<ToolCall>,
}
```

Initialize in `new()`:
```rust
let (tool_tx, tool_rx) = mpsc::channel::<ToolCall>();
```

Add drain method:
```rust
pub fn drain_tool_calls(&self) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    while let Ok(tc) = self.tool_rx.try_recv() {
        calls.push(tc);
    }
    calls
}
```

- [ ] **Step 2: Add tool_tx parameter to send_prompt in claude.rs**

Add `on_tool: Option<&std::sync::mpsc::Sender<super::claude_bridge::ToolCall>>` parameter to `send_prompt()`. In the stream-json parsing loop, alongside the existing text extraction, add:

```rust
if let Some("tool_use") = block.get("type").and_then(|t| t.as_str()) {
    if let Some(tool_tx) = on_tool {
        let tc = crate::claude_bridge::ToolCall {
            name: block.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
            input: block.get("input").cloned().unwrap_or(serde_json::Value::Null),
        };
        let _ = tool_tx.send(tc);
    }
}
```

Also add `on_tool` parameter to `send_with_phase_prompt` and pass through.

- [ ] **Step 3: Add MCP config parameters to send_prompt**

Add `mcp_config: Option<&std::path::Path>` and `disable_builtin_tools: bool` parameters. When set:

```rust
if disable_builtin_tools {
    cmd.arg("--tools").arg("");
    cmd.arg("--strict-mcp-config");
    cmd.arg("--disallowedTools").arg("LSP");
}
if let Some(config_path) = mcp_config {
    cmd.arg("--mcp-config").arg(config_path);
}
```

- [ ] **Step 4: Update all call sites**

Update `send_with_phase_prompt` to accept and pass through the new parameters.
Update `ClaudeBridge::send_phase_prompt` and `send_raw_prompt` to pass `Some(&self.tool_tx)` for `on_tool`.
For MCP config: `send_phase_prompt` passes them through, `send_raw_prompt` passes `None` (ref research doesn't use MCP).

- [ ] **Step 5: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 6: Commit**

```bash
git add src/claude.rs src/claude_bridge.rs
git commit -m "feat: parse tool_use blocks from stream-json, add MCP config flags"
```

## Chunk 3: MCP Config Generation & Phase Integration

### Task 3: Generate MCP config and wire into phase dispatch

**Files:**
- Modify: `src/claude_bridge.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add MCP config generation to ClaudeBridge**

```rust
/// Generate a temporary MCP config JSON for the given phase and session dir.
/// Returns the path to the temp file.
pub fn generate_mcp_config(&self, phase_name: &str, session_dir: Option<&Path>) -> Result<PathBuf, String> {
    let server_path = find_mcp_server()?;
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
                "command": "python3",
                "args": args
            }
        }
    });

    let tmp_path = std::env::temp_dir().join(format!("mimodel_mcp_{}.json", std::process::id()));
    std::fs::write(&tmp_path, config.to_string())
        .map_err(|e| format!("Failed to write MCP config: {e}"))?;
    Ok(tmp_path)
}

/// Locate mcp/server.py relative to the binary or cwd.
fn find_mcp_server() -> Result<PathBuf, String> {
    let candidates = [
        std::env::current_dir().ok().map(|d| d.join("mcp/server.py")),
        std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("mcp/server.py"))),
    ];
    for candidate in candidates.into_iter().flatten() {
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("mcp/server.py not found".to_string())
}
```

- [ ] **Step 2: Update send_phase_prompt to use MCP**

Add `mcp_config: Option<PathBuf>` parameter to `send_phase_prompt`. When Some, pass to `send_prompt` alongside `disable_builtin_tools: true`.

- [ ] **Step 3: Wire MCP config in phase dispatch**

In main.rs, each send method now generates MCP config before calling `self.claude.send_phase_prompt`:

```rust
fn send_spec_prompt(&mut self, text: &str, images: Vec<PathBuf>) {
    let ref_context = self.build_ref_context();
    let session_dir = self.session.active_dir.clone();
    let mcp_config = self.claude.generate_mcp_config("spec", session_dir.as_deref()).ok();

    let prompt = /* ... existing prompt building ... */;
    self.claude.send_phase_prompt("spec", &prompt, &images, ref_context.as_deref(), mcp_config);
}
```

Repeat for all phase send methods: decompose, component, assembly, refinement.

- [ ] **Step 4: Drain tool calls in event loop**

In the main event loop, after draining streaming:

```rust
let tool_calls = app.claude.drain_tool_calls();
for tc in tool_calls {
    app.handle_tool_call(tc);
    app.dirty = true;
}
```

- [ ] **Step 5: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 6: Commit**

```bash
git add src/claude_bridge.rs src/main.rs
git commit -m "feat: generate MCP config per phase, wire tool dispatch into event loop"
```

### Task 4: Implement handle_tool_call dispatcher

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add handle_tool_call method**

```rust
fn handle_tool_call(&mut self, tool: claude_bridge::ToolCall) {
    // Strip mcp__mimodel__ prefix if present
    let name = tool.name.strip_prefix("mcp__mimodel__").unwrap_or(&tool.name);

    match name {
        "ask_question" | "ask_clarification" => {
            if let Some(q) = tool.input.get("question").and_then(|v| v.as_str()) {
                self.session.add_message(self.phase, "assistant", q);
                self.conversation.add("assistant", q);
            }
        }
        "record_spec_field" => {
            let cat = tool.input.get("category").and_then(|v| v.as_str()).unwrap_or("");
            let key = tool.input.get("key").and_then(|v| v.as_str()).unwrap_or("");
            let val = tool.input.get("value").and_then(|v| v.as_str()).unwrap_or("");
            let unit = tool.input.get("unit").and_then(|v| v.as_str()).unwrap_or("");
            let entry = format!("[{}] {} = {} {}", cat, key, val, unit);
            // Append to spec tab
            let mut content = self.right_panel.spec_content.clone();
            if !content.is_empty() { content.push('\n'); }
            content.push_str(&entry);
            self.right_panel.set_spec(&content);
        }
        "mark_spec_complete" => {
            self.conversation.add("system", "Spec complete. Type 'advance' to move to Decompose phase.");
            self.session.add_message(self.phase, "system", "Spec complete. Awaiting user advancement.");
        }
        "propose_component_tree" => {
            if let Some(components) = tool.input.get("components").and_then(|v| v.as_array()) {
                // Convert JSON to display format
                let mut tree_lines = Vec::new();
                for c in components {
                    let id = c.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                    let name = c.get("name").and_then(|v| v.as_str()).unwrap_or(id);
                    let op = c.get("assembly_op").and_then(|v| v.as_str()).unwrap_or("union");
                    tree_lines.push(format!("  {} — {} [{}]", id, name, op));
                }
                self.conversation.add("system",
                    &format!("Component tree proposed:\n{}\nType 'approve' to accept, or describe changes.",
                        tree_lines.join("\n")));
            }
        }
        "submit_cadquery_code" | "submit_assembly_code" | "submit_code_patch" => {
            // Build happened in MCP server — detect new STL and refresh viewer
            if let Some(ref dir) = self.session.active_dir {
                let working_stl = dir.join("working.stl");
                if working_stl.exists() {
                    let _ = self.viewer.update_working_stl(&working_stl);
                    if !self.viewer.is_running() {
                        let _ = self.viewer.show();
                    }
                }
            }
            self.right_panel.set_model("Build complete — check 3D viewer");
        }
        "request_approval" => {
            if let Some(summary) = tool.input.get("summary").and_then(|v| v.as_str()) {
                self.conversation.add("system",
                    &format!("Review model in viewer. {}\nType 'approve' or describe changes.", summary));
            }
        }
        "update_parameter" => {
            let pname = tool.input.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let new_val = tool.input.get("new_value").and_then(|v| v.as_str()).unwrap_or("");
            let mut content = self.right_panel.spec_content.clone();
            content.push_str(&format!("\nUpdated: {} = {}", pname, new_val));
            self.right_panel.set_spec(&content);
        }
        _ => {} // Unknown tool — ignore
    }
}
```

- [ ] **Step 2: Add 'advance' command handling in submit_prompt**

In submit_prompt's phase dispatch, when user types "advance":

```rust
if text.trim().eq_ignore_ascii_case("advance") {
    match self.phase {
        Phase::Spec => {
            self.phase = Phase::Decompose;
            self.layout_config.phase = Phase::Decompose;
            self.claude.session_id = None; // Fresh session for new phase
            self.conversation.add("system", "Advanced to Decompose phase.");
            self.session.save(self.phase);
        }
        // ... similar for other phase transitions
        _ => {
            self.conversation.add("system", "Cannot advance from this phase.");
        }
    }
    return;
}
```

- [ ] **Step 3: Poll .building file for BusyState::Building**

In the event loop, after draining tool calls, check for the `.building` sentinel:

```rust
if app.claude.busy == BusyState::Thinking {
    if let Some(ref dir) = app.session.active_dir {
        if dir.join(".building").exists() {
            app.claude.busy = BusyState::Building;
            app.dirty = true;
        }
    }
}
```

- [ ] **Step 4: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: handle_tool_call dispatcher, advance command, build state detection"
```

## Chunk 4: Update Phase Prompts & Cleanup

### Task 5: Update system prompts to describe tools

**Files:**
- Modify: `prompts/spec.md`
- Modify: `prompts/decompose.md`
- Modify: `prompts/component.md`
- Modify: `prompts/assembly.md`
- Modify: `prompts/refinement.md`

- [ ] **Step 1: Rewrite spec.md**

Replace the current content with a tool-oriented prompt:

```markdown
You are helping a user design a 3D model for manufacturing (resin printing, CNC, etc).

You have these tools available:
- ask_question: Ask ONE clarifying question at a time
- record_spec_field: Record a dimension, constraint, feature, or component reference
- mark_spec_complete: Signal that the specification is complete

Your workflow:
1. Ask questions one at a time to understand the design
2. After each answer, record the relevant spec fields
3. Follow this order: purpose/context → dimensions → features → constraints → surface finish
4. When you have enough information, call mark_spec_complete

Rules:
- Do NOT generate any code
- Do NOT suggest materials or print settings
- Record every dimension and constraint as a spec field
- Prefer standard components from the reference library when available
- When you mention an external component, use the REF[component name] notation
```

- [ ] **Step 2: Update decompose.md, component.md, assembly.md, refinement.md**

Each prompt describes the available tools for that phase and the expected workflow. Component.md emphasizes the build-review-approve cycle. Assembly.md describes accessing component artifacts.

- [ ] **Step 3: Commit**

```bash
git add prompts/
git commit -m "docs: rewrite phase prompts for MCP tool-use workflow"
```

### Task 6: Remove old code-block stripping

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Simplify handle_spec_response and handle_decompose_response**

Remove the `parser::parse_response` code-block stripping from these handlers. With MCP tools, Claude's freeform text is just conversation — no code blocks to strip. The handlers now just:
- Add assistant text to conversation
- Run reference detection (keep this)
- No SPEC_COMPLETE text detection (replaced by mark_spec_complete tool)

- [ ] **Step 2: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 3: Full integration test**

Run `cargo run` and verify:
1. Start a new session — Claude uses `ask_question` tool, NOT freeform questions
2. Answer a question — Claude calls `record_spec_field` with structured data
3. Spec panel shows structured key-value data (not raw text)
4. Type "advance" — transitions to Decompose phase
5. Claude uses `propose_component_tree` tool (not freeform TOML)

- [ ] **Step 4: Commit**

```bash
git commit -am "refactor: remove code-block stripping, SPEC_COMPLETE detection — replaced by MCP tools"
```
