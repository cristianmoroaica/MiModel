#!/usr/bin/env python3
"""MiModel MCP server — per-phase tool definitions for Claude CLI.

Implements JSON-RPC over stdin/stdout (MCP protocol). Each phase exposes
only the tools appropriate for that phase. Build tools execute CadQuery
directly and return results.
"""
import argparse
import json
import os
import sys

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
        "description": "Submit CadQuery assembly code that combines approved components. Component STEPs are in components/<id>/result.step.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "code": {"type": "string", "description": "CadQuery assembly code"}
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

def run_cadquery_build(code, output_dir, session_root=None, label="build"):
    """Execute CadQuery code and export STL+STEP. Returns result dict."""
    import subprocess
    import shutil

    os.makedirs(output_dir, exist_ok=True)
    code_path = os.path.join(output_dir, "code.py")
    stl_path = os.path.join(output_dir, "result.stl")
    step_path = os.path.join(output_dir, "result.step")

    # Write .building signal for the Rust app
    building_flag = os.path.join(session_root, ".building") if session_root else None
    if building_flag:
        open(building_flag, "w").close()

    try:
        with open(code_path, "w") as f:
            f.write(code)

        # Build in subprocess to isolate crashes
        export_code = code + f"""

# ── Auto-export ──
import cadquery as cq
cq.exporters.export(result, "{stl_path}")
cq.exporters.export(result, "{step_path}")
bb = result.val().BoundingBox()
print(f"DIMS:{{bb.xlen:.2f}}x{{bb.ylen:.2f}}x{{bb.zlen:.2f}}")
"""
        proc = subprocess.run(
            [sys.executable, "-c", export_code],
            capture_output=True, text=True, timeout=60
        )

        if proc.returncode != 0:
            error = proc.stderr[-2000:] if proc.stderr else "Unknown build error"
            return {"success": False, "error": error}

        dims = "unknown"
        for line in proc.stdout.splitlines():
            if line.startswith("DIMS:"):
                dims = line[5:]

        # Copy to working.stl/working.step in session root
        if session_root:
            for src, name in [(stl_path, "working.stl"), (step_path, "working.step")]:
                if os.path.exists(src):
                    shutil.copy2(src, os.path.join(session_root, name))

        return {"success": True, "dimensions": dims, "stl_path": stl_path, "step_path": step_path}

    except subprocess.TimeoutExpired:
        return {"success": False, "error": "Build timed out after 60 seconds"}
    except Exception as e:
        return {"success": False, "error": str(e)}
    finally:
        if building_flag and os.path.exists(building_flag):
            os.remove(building_flag)

# ── Tool call handlers ──

def handle_tool_call(name, arguments, session_dir):
    """Execute a tool call and return the MCP result content."""

    if name in ("ask_question", "ask_clarification"):
        q = arguments.get("question", "")
        return [{"type": "text", "text": f"Question delivered to user: {q}"}]

    if name == "record_spec_field":
        category = arguments.get("category", "")
        key = arguments.get("key", "")
        value = arguments.get("value", "")
        unit = arguments.get("unit", "")
        spec_fields.append({"category": category, "key": key, "value": value, "unit": unit})
        summary = "\n".join(
            f"  [{f['category']}] {f['key']} = {f['value']} {f['unit']}"
            for f in spec_fields
        )
        return [{"type": "text", "text": f"Recorded. Current spec ({len(spec_fields)} fields):\n{summary}"}]

    if name == "mark_spec_complete":
        summary = "\n".join(
            f"  [{f['category']}] {f['key']} = {f['value']} {f['unit']}"
            for f in spec_fields
        )
        return [{"type": "text", "text": f"Spec marked complete with {len(spec_fields)} fields. Awaiting user confirmation to advance.\n{summary}"}]

    if name == "propose_component_tree":
        components = arguments.get("components", [])
        tree_str = "\n".join(
            f"  {c['id']}: {c.get('name', c['id'])} [{c.get('assembly_op', 'union')}]"
            for c in components
        )
        return [{"type": "text", "text": f"Component tree proposed ({len(components)} components). Awaiting user review.\n{tree_str}"}]

    if name == "submit_cadquery_code":
        component_id = arguments.get("component_id", "unknown")
        code = arguments.get("code", "")
        output_dir = os.path.join(session_dir, "components", component_id) if session_dir else "/tmp/mimodel_build"
        result = run_cadquery_build(code, output_dir, session_root=session_dir, label=component_id)
        if result["success"]:
            return [{"type": "text", "text": f"Build successful! Dimensions: {result['dimensions']}mm. Model displayed in 3D viewer. Call request_approval when ready."}]
        else:
            return [{"type": "text", "text": f"Build failed:\n{result['error']}\n\nFix the code and try again."}]

    if name == "submit_assembly_code":
        code = arguments.get("code", "")
        output_dir = os.path.join(session_dir, "assembly") if session_dir else "/tmp/mimodel_build"
        result = run_cadquery_build(code, output_dir, session_root=session_dir, label="assembly")
        if result["success"]:
            return [{"type": "text", "text": f"Assembly built! Dimensions: {result['dimensions']}mm. Model displayed in viewer."}]
        else:
            return [{"type": "text", "text": f"Assembly build failed:\n{result['error']}"}]

    if name == "submit_code_patch":
        code = arguments.get("code", "")
        output_dir = os.path.join(session_dir, "refinement") if session_dir else "/tmp/mimodel_build"
        result = run_cadquery_build(code, output_dir, session_root=session_dir, label="refinement")
        if result["success"]:
            return [{"type": "text", "text": f"Refinement built! Dimensions: {result['dimensions']}mm."}]
        else:
            return [{"type": "text", "text": f"Refinement build failed:\n{result['error']}"}]

    if name == "request_approval":
        summary = arguments.get("summary", "")
        return [{"type": "text", "text": f"Approval requested. User reviewing model. Summary: {summary}"}]

    if name == "update_parameter":
        pname = arguments.get("name", "")
        old = arguments.get("old_value", "")
        new = arguments.get("new_value", "")
        return [{"type": "text", "text": f"Parameter updated: {pname} changed from {old} to {new}"}]

    return [{"type": "text", "text": f"Unknown tool: {name}"}]

# ── Main loop ──

def main():
    parser = argparse.ArgumentParser(description="MiModel MCP server")
    parser.add_argument("--phase", required=True, choices=PHASE_TOOLS.keys())
    parser.add_argument("--session-dir", default=None)
    args = parser.parse_args()

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
            content = handle_tool_call(tool_name, arguments, args.session_dir)
            send_response(id, {"content": content})
        else:
            if id is not None:
                send_response(id, {})

if __name__ == "__main__":
    main()
