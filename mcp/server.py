#!/usr/bin/env python3
"""MiModel MCP server — per-phase tool definitions for Claude CLI.

Implements JSON-RPC over stdin/stdout (MCP protocol). Each phase exposes
only the tools appropriate for that phase. Writing a .py file to a build
directory (components/, assembly/, refinement/) auto-triggers a CadQuery build.
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

# ── Shared tool definitions ──

WRITE_FILE_TOOL = {
    "name": "write_file",
    "description": (
        "Write content to a file in the session directory. "
        "When writing a .py file to a build directory (components/<id>/, assembly/, refinement/), "
        "the code is automatically executed as CadQuery, the STL is built, and _buffer.stl is "
        "updated so the 3D viewer auto-reloads. Build results (dimensions or errors) are returned. "
        "Use paths like 'components/body/code.py', 'assembly/code.py', 'refinement/code.py'."
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "Relative path within the session directory"},
            "content": {"type": "string", "description": "File content to write"}
        },
        "required": ["path", "content"]
    }
}

OPEN_VIEWER_TOOL = {
    "name": "open_viewer",
    "description": "Open the current model in the 3D viewer (f3d). Use this when the user asks to see or view the model.",
    "inputSchema": {
        "type": "object",
        "properties": {},
        "required": []
    }
}

READ_FILE_TOOL = {
    "name": "read_file",
    "description": "Read a file from the session directory. Use relative paths like 'components/body/code.py', 'refinement/code.py', 'spec.toml', etc. Only text files can be read (not binary STL/STEP).",
    "inputSchema": {
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "Relative path within the session directory"}
        },
        "required": ["path"]
    }
}

LIST_FILES_TOOL = {
    "name": "list_files",
    "description": "List files in the session directory (or a subdirectory). Shows the project file tree so you can find code, specs, and build artifacts.",
    "inputSchema": {
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "Relative subdirectory path (empty string or '.' for session root)", "default": "."}
        },
        "required": []
    }
}

SCREENSHOT_VIEWER_TOOL = {
    "name": "screenshot_viewer",
    "description": "Capture a screenshot of the f3d 3D viewer window and return it as an image. Use this to visually verify your build results — check geometry, holes, chamfers, proportions, etc.",
    "inputSchema": {
        "type": "object",
        "properties": {},
        "required": []
    }
}

IMPORT_STEP_TOOL = {
    "name": "import_step",
    "description": (
        "Import an existing .step/.stp file into the session. Analyzes the geometry "
        "(dimensions, face types, holes, topology) and generates a starter code.py. "
        "The model is loaded into the viewer. Use the analysis to recreate it as parametric CadQuery code."
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "source_path": {"type": "string", "description": "Absolute path to the .step/.stp file on disk"},
            "target_dir": {"type": "string", "description": "Target subdirectory in session (e.g. 'components/body', 'refinement')", "default": "imported"}
        },
        "required": ["source_path"]
    }
}

# ── Phase-specific tool definitions ──

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
    WRITE_FILE_TOOL,
    OPEN_VIEWER_TOOL,
    READ_FILE_TOOL,
    LIST_FILES_TOOL,
    SCREENSHOT_VIEWER_TOOL,
    IMPORT_STEP_TOOL,
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
    WRITE_FILE_TOOL,
    OPEN_VIEWER_TOOL,
    READ_FILE_TOOL,
    LIST_FILES_TOOL,
    SCREENSHOT_VIEWER_TOOL,
    IMPORT_STEP_TOOL,
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
    WRITE_FILE_TOOL,
    OPEN_VIEWER_TOOL,
    READ_FILE_TOOL,
    LIST_FILES_TOOL,
    SCREENSHOT_VIEWER_TOOL,
    IMPORT_STEP_TOOL,
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

# ── STEP import + analysis ──

def analyze_step(step_path):
    """Load a STEP file and extract geometry analysis for parametric reconstruction."""
    import subprocess

    analysis_code = f"""
import cadquery as cq
import json
from collections import Counter

shape = cq.importers.importStep("{step_path}")
solid = shape.val()
bb = solid.BoundingBox()

# Topology
faces = shape.faces().vals()
edges = shape.edges().vals()
vertices = shape.vertices().vals()

# Classify faces
face_types = Counter()
cylinders = []
planes = []
for f in faces:
    gt = f.geomType()
    face_types[gt] += 1
    if gt == "CYLINDER":
        # Get radius from the surface
        try:
            surf = f._geomAdaptor()
            from OCP.BRepAdaptor import BRepAdaptor_Surface
            from OCP.GeomAbs import GeomAbs_Cylinder
            adaptor = BRepAdaptor_Surface(f.wrapped)
            if adaptor.GetType() == GeomAbs_Cylinder:
                cyl = adaptor.Cylinder()
                r = cyl.Radius()
                loc = cyl.Location()
                cylinders.append({{"radius": round(r, 3), "x": round(loc.X(), 2), "y": round(loc.Y(), 2), "z": round(loc.Z(), 2)}})
        except:
            pass
    elif gt == "PLANE":
        try:
            center = f.Center()
            normal = f.normalAt(center)
            area = f.Area()
            planes.append({{"area": round(area, 2), "normal": [round(normal.x, 3), round(normal.y, 3), round(normal.z, 3)]}})
        except:
            pass

# Detect likely holes (pairs of same-radius cylinders)
hole_radii = Counter()
for c in cylinders:
    hole_radii[c["radius"]] += 1
likely_holes = [{{
    "diameter": round(r * 2, 3),
    "count": count,
    "positions": [c for c in cylinders if c["radius"] == r]
}} for r, count in hole_radii.items() if count >= 1]

result = {{
    "bounding_box": {{
        "x": round(bb.xlen, 2), "y": round(bb.ylen, 2), "z": round(bb.zlen, 2),
        "min": [round(bb.xmin, 2), round(bb.ymin, 2), round(bb.zmin, 2)],
        "max": [round(bb.xmax, 2), round(bb.ymax, 2), round(bb.zmax, 2)],
    }},
    "topology": {{
        "faces": len(faces), "edges": len(edges), "vertices": len(vertices)
    }},
    "face_types": dict(face_types),
    "likely_holes": likely_holes,
    "planes": planes[:20],
}}
print("ANALYSIS:" + json.dumps(result))
"""
    try:
        proc = subprocess.run(
            [sys.executable, "-c", analysis_code],
            capture_output=True, text=True, timeout=30
        )
        if proc.returncode != 0:
            return None, proc.stderr[-1000:]
        for line in proc.stdout.splitlines():
            if line.startswith("ANALYSIS:"):
                return json.loads(line[9:]), None
        return None, "No analysis output"
    except Exception as e:
        return None, str(e)

def handle_import_step(arguments, session_dir):
    """Import a STEP file: copy, analyze, generate wrapper code, build STL."""
    import shutil

    source = arguments.get("source_path", "")
    target_dir_rel = arguments.get("target_dir", "imported")

    if not session_dir:
        return [{"type": "text", "text": "No session directory set."}]
    if not os.path.exists(source):
        return [{"type": "text", "text": f"File not found: {source}"}]
    ext = os.path.splitext(source)[1].lower()
    if ext not in (".step", ".stp"):
        return [{"type": "text", "text": f"Not a STEP file: {source}"}]

    # Copy STEP into session
    target_dir = os.path.join(session_dir, target_dir_rel)
    os.makedirs(target_dir, exist_ok=True)
    dest_step = os.path.join(target_dir, "imported.step")
    shutil.copy2(source, dest_step)

    # Analyze geometry
    analysis, error = analyze_step(dest_step)
    if error:
        return [{"type": "text", "text": f"STEP imported to {target_dir_rel}/imported.step but analysis failed:\n{error}"}]

    # Generate wrapper code.py
    wrapper_code = f'''import cadquery as cq
import os

# ── Imported from: {os.path.basename(source)} ──
# Bounding box: {analysis["bounding_box"]["x"]} x {analysis["bounding_box"]["y"]} x {analysis["bounding_box"]["z"]} mm
# Topology: {analysis["topology"]["faces"]} faces, {analysis["topology"]["edges"]} edges
# Face types: {analysis.get("face_types", {})}

STEP_PATH = os.path.join(os.path.dirname(__file__), "imported.step")

result = cq.importers.importStep(STEP_PATH)
'''
    code_path = os.path.join(target_dir, "code.py")
    with open(code_path, "w") as f:
        f.write(wrapper_code)

    # Build STL + update buffer
    build_result = run_cadquery_build(wrapper_code.replace(
        'os.path.join(os.path.dirname(__file__), "imported.step")',
        f'"{dest_step}"'
    ), target_dir, session_root=session_dir, label="import")

    # Format analysis for Claude
    lines = [
        f"STEP imported: {os.path.basename(source)}",
        f"Copied to: {target_dir_rel}/imported.step",
        f"Wrapper: {target_dir_rel}/code.py",
        "",
        "## Geometry Analysis",
        f"Bounding box: {analysis['bounding_box']['x']} x {analysis['bounding_box']['y']} x {analysis['bounding_box']['z']} mm",
        f"Topology: {analysis['topology']['faces']} faces, {analysis['topology']['edges']} edges, {analysis['topology']['vertices']} vertices",
        f"Face types: {analysis.get('face_types', {})}",
    ]

    if analysis.get("likely_holes"):
        lines.append("")
        lines.append("## Detected Holes")
        for h in analysis["likely_holes"]:
            positions = ", ".join(f"({p['x']},{p['y']},{p['z']})" for p in h["positions"])
            lines.append(f"  {h['count']}x diameter {h['diameter']}mm at {positions}")

    if build_result["success"]:
        lines.append("")
        lines.append(f"Build: OK ({build_result['dimensions']}mm). Viewer updated.")
    else:
        lines.append("")
        lines.append(f"Build failed: {build_result.get('error', 'unknown')}")

    lines.append("")
    lines.append("Use read_file to view the wrapper code, then recreate as parametric CadQuery.")

    return [{"type": "text", "text": "\n".join(lines)}]

# ── Build helper ──

BUILD_DIRS = {"components", "assembly", "refinement", "imported"}

def detect_build_dir(rel_path, session_dir):
    """If rel_path is a .py file inside a build directory, return the output dir.
    Returns (output_dir, label) or (None, None)."""
    if not rel_path.endswith(".py"):
        return None, None
    parts = rel_path.replace("\\", "/").split("/")
    if not parts:
        return None, None
    top = parts[0]
    if top not in BUILD_DIRS:
        return None, None
    # output_dir = session_dir / everything up to the .py file's parent
    parent = os.path.join(session_dir, *parts[:-1]) if len(parts) > 1 else session_dir
    label = parts[1] if top == "components" and len(parts) > 2 else top
    return parent, label

def run_cadquery_build(code, output_dir, session_root=None, label="build"):
    """Execute CadQuery code and export STL+STEP. Returns result dict."""
    import subprocess
    import shutil

    os.makedirs(output_dir, exist_ok=True)
    stl_path = os.path.join(output_dir, "result.stl")
    step_path = os.path.join(output_dir, "result.step")

    # Write .building signal for the Rust app
    building_flag = os.path.join(session_root, ".building") if session_root else None
    if building_flag:
        open(building_flag, "w").close()

    try:
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

        # Copy to _buffer.stl/_buffer.step in session root
        if session_root:
            for src, name in [(stl_path, "_buffer.stl"), (step_path, "_buffer.step")]:
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

# ── Viewer screenshot ──

def capture_viewer_screenshot(session_dir):
    """Capture the f3d window via hyprctl + grim and return as MCP image content."""
    import subprocess
    import base64
    import tempfile

    # Find f3d window geometry via Hyprland IPC
    try:
        result = subprocess.run(
            ["hyprctl", "clients", "-j"],
            capture_output=True, text=True, timeout=3
        )
        if result.returncode != 0:
            return [{"type": "text", "text": "Cannot query windows (hyprctl failed)."}]

        clients = json.loads(result.stdout)
        f3d_window = None
        for client in clients:
            if "f3d" in client.get("class", "").lower() or "f3d" in client.get("title", "").lower():
                f3d_window = client
                break

        if not f3d_window:
            return [{"type": "text", "text": "f3d window not found. Open the viewer first with open_viewer."}]

        x, y = f3d_window["at"]
        w, h = f3d_window["size"]

        if w <= 0 or h <= 0:
            return [{"type": "text", "text": "f3d window has zero size (minimized?)."}]

        # Capture with grim
        screenshot_path = tempfile.mktemp(suffix=".png")
        result = subprocess.run(
            ["grim", "-g", f"{x},{y} {w}x{h}", screenshot_path],
            capture_output=True, text=True, timeout=5
        )
        if result.returncode != 0:
            return [{"type": "text", "text": f"Screenshot failed: {result.stderr.strip()}"}]

        with open(screenshot_path, "rb") as f:
            data = base64.standard_b64encode(f.read()).decode("ascii")

        os.unlink(screenshot_path)

        return [{"type": "image", "data": data, "mimeType": "image/png"}]

    except FileNotFoundError:
        return [{"type": "text", "text": "grim or hyprctl not found. Install grim for Wayland screenshots."}]
    except Exception as e:
        return [{"type": "text", "text": f"Screenshot error: {e}"}]

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

    if name == "request_approval":
        summary = arguments.get("summary", "")
        return [{"type": "text", "text": f"Approval requested. User reviewing model. Summary: {summary}"}]

    if name == "update_parameter":
        pname = arguments.get("name", "")
        old = arguments.get("old_value", "")
        new = arguments.get("new_value", "")
        return [{"type": "text", "text": f"Parameter updated: {pname} changed from {old} to {new}"}]

    if name == "import_step":
        return handle_import_step(arguments, session_dir)

    if name == "screenshot_viewer":
        return capture_viewer_screenshot(session_dir)

    if name == "open_viewer":
        if session_dir:
            buffer_stl = os.path.join(session_dir, "_buffer.stl")
            if os.path.exists(buffer_stl):
                signal = os.path.join(session_dir, ".open_viewer")
                open(signal, "w").close()
                return [{"type": "text", "text": "Opening model in 3D viewer."}]
            else:
                return [{"type": "text", "text": "No model built yet. Write code first."}]
        return [{"type": "text", "text": "No session directory — cannot open viewer."}]

    if name == "write_file":
        rel_path = arguments.get("path", "")
        content = arguments.get("content", "")
        if not session_dir:
            return [{"type": "text", "text": "No session directory set."}]
        # Resolve and validate path stays within session dir
        full_path = os.path.normpath(os.path.join(session_dir, rel_path))
        if not full_path.startswith(os.path.normpath(session_dir)):
            return [{"type": "text", "text": "Path must be within the session directory."}]
        # Write the file
        try:
            os.makedirs(os.path.dirname(full_path), exist_ok=True)
            with open(full_path, "w") as f:
                f.write(content)
        except Exception as e:
            return [{"type": "text", "text": f"Error writing file: {e}"}]
        # Auto-build if it's a .py in a build directory
        output_dir, label = detect_build_dir(rel_path, session_dir)
        if output_dir:
            result = run_cadquery_build(content, output_dir, session_root=session_dir, label=label)
            if result["success"]:
                return [{"type": "text", "text": f"File written: {rel_path}\nBuild successful! Dimensions: {result['dimensions']}mm. STL: {result['stl_path']}. Viewer will auto-reload."}]
            else:
                return [{"type": "text", "text": f"File written: {rel_path}\nBuild failed:\n{result['error']}"}]
        return [{"type": "text", "text": f"File written: {rel_path}"}]

    if name == "read_file":
        rel_path = arguments.get("path", "")
        if not session_dir:
            return [{"type": "text", "text": "No session directory set."}]
        full_path = os.path.normpath(os.path.join(session_dir, rel_path))
        if not full_path.startswith(os.path.normpath(session_dir)):
            return [{"type": "text", "text": "Path must be within the session directory."}]
        if not os.path.exists(full_path):
            return [{"type": "text", "text": f"File not found: {rel_path}"}]
        if not os.path.isfile(full_path):
            return [{"type": "text", "text": f"Not a file: {rel_path}. Use list_files to browse directories."}]
        ext = os.path.splitext(full_path)[1].lower()
        if ext in (".stl", ".step", ".stp", ".png", ".jpg", ".jpeg", ".pdf"):
            return [{"type": "text", "text": f"Cannot read binary file ({ext}). For STL metadata, read {rel_path}.json if it exists."}]
        try:
            with open(full_path, "r") as f:
                content = f.read(100_000)
            return [{"type": "text", "text": content}]
        except Exception as e:
            return [{"type": "text", "text": f"Error reading file: {e}"}]

    if name == "list_files":
        rel_path = arguments.get("path", ".")
        if not session_dir:
            return [{"type": "text", "text": "No session directory set."}]
        full_path = os.path.normpath(os.path.join(session_dir, rel_path))
        if not full_path.startswith(os.path.normpath(session_dir)):
            return [{"type": "text", "text": "Path must be within the session directory."}]
        if not os.path.isdir(full_path):
            return [{"type": "text", "text": f"Not a directory: {rel_path}"}]
        try:
            lines = []
            for root, dirs, files in os.walk(full_path):
                dirs[:] = [d for d in sorted(dirs) if not d.startswith('.')]
                rel_root = os.path.relpath(root, session_dir)
                if rel_root == ".":
                    rel_root = ""
                for fname in sorted(files):
                    if fname.startswith('.') or fname == "session.json":
                        continue
                    fpath = os.path.join(root, fname)
                    size = os.path.getsize(fpath)
                    display_path = os.path.join(rel_root, fname) if rel_root else fname
                    lines.append(f"  {display_path} ({size:,} bytes)")
            if not lines:
                return [{"type": "text", "text": "Directory is empty."}]
            return [{"type": "text", "text": "\n".join(lines)}]
        except Exception as e:
            return [{"type": "text", "text": f"Error listing files: {e}"}]

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
                "serverInfo": {"name": "mimodel", "version": "0.2.0"}
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
