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

# ── Web fetch + Reference library tools ──

import time
import re as _re
import urllib.request
import urllib.error

def _references_dir():
    """Return the global references directory: ~/MiModel/references/"""
    return os.path.join(os.path.expanduser("~"), "MiModel", "references")

FETCH_URL_TOOL = {
    "name": "fetch_url",
    "description": (
        "Fetch content from a URL. Use this to verify component specs against manufacturer "
        "datasheets and documentation. Returns text content extracted from the page. "
        "Works with HTML pages, plain text, and PDF documents (text is extracted from PDFs "
        "automatically, up to 20 pages)."
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "url": {"type": "string", "description": "The URL to fetch"},
            "max_length": {"type": "integer", "description": "Max characters to return (default 10000)", "default": 10000}
        },
        "required": ["url"]
    }
}

LIST_REFERENCES_TOOL = {
    "name": "list_references",
    "description": (
        "List all component reference datasheets in the global library. "
        "Returns names, categories, and key dimensions for each reference. "
        "Use this to find available reference data before building."
    ),
    "inputSchema": {
        "type": "object",
        "properties": {},
        "required": []
    }
}

READ_REFERENCE_TOOL = {
    "name": "read_reference",
    "description": (
        "Read a specific component reference datasheet from the global library. "
        "Returns the full TOML spec with identity, dimensions, constraints, and mounting info. "
        "Use the filename (without .toml) or a search term to find the reference."
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Reference filename without .toml (e.g. 'nema_23_stepper_motor_standard_frame') or a search term (e.g. 'nema 23', 'arduino')"
            }
        },
        "required": ["query"]
    }
}

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
    "description": (
        "Render engineering views of the current model: front, top, right (orthographic) "
        "plus one isometric view. Orthographic views show exact proportions and feature "
        "positions. Use this after every build to verify spatial correctness — hole positions, "
        "pocket alignment, symmetry. Coordinate system: +X=right, +Y=forward, +Z=up."
    ),
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

BUILD_TOOLS = [
    {
        "name": "ask_clarification",
        "description": "Ask the user a clarifying question about the design or build.",
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
        "description": "After verifying the build against goal.md, ask the user to approve or provide feedback.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "summary": {"type": "string", "description": "Brief summary of what was built, including key dimensions"}
            },
            "required": ["summary"]
        }
    },
    WRITE_FILE_TOOL,
    OPEN_VIEWER_TOOL,
    READ_FILE_TOOL,
    LIST_FILES_TOOL,
    SCREENSHOT_VIEWER_TOOL,
    IMPORT_STEP_TOOL,
    LIST_REFERENCES_TOOL,
    READ_REFERENCE_TOOL,
    FETCH_URL_TOOL,
]

REFINE_TOOLS = [
    {
        "name": "ask_clarification",
        "description": "Ask the user a clarifying question about the aesthetic refinement.",
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
    LIST_REFERENCES_TOOL,
    READ_REFERENCE_TOOL,
    FETCH_URL_TOOL,
]

PHASE_TOOLS = {
    "spec": SPEC_TOOLS,
    "build": BUILD_TOOLS,
    "refine": REFINE_TOOLS,
    # Backward compat aliases
    "decompose": BUILD_TOOLS,
    "component": BUILD_TOOLS,
    "assembly": BUILD_TOOLS,
    "refinement": REFINE_TOOLS,
}

# ── Spec field accumulator ──

spec_fields = []

# ── Goal document generation ──

def generate_goal_document(fields):
    """Generate goal.md from recorded spec fields.
    Organizes into functional checks (dimensions, components, constraints)
    and visual requirements (features, surface finish)."""
    dimensions = [f for f in fields if f["category"] == "dimension"]
    constraints = [f for f in fields if f["category"] == "constraint"]
    features = [f for f in fields if f["category"] == "feature"]
    components = [f for f in fields if f["category"] == "component"]

    lines = ["# Design Goal", ""]

    # Components to accommodate
    if components:
        lines.append("## Components to Accommodate")
        for c in components:
            unit = f" {c['unit']}" if c["unit"] else ""
            lines.append(f"- {c['key']}: {c['value']}{unit}")
        lines.append("")

    # Functional requirements — these are checked FIRST
    lines.append("## Functional Requirements (verify FIRST)")
    lines.append("These must be correct before any visual check.")
    lines.append("")
    if dimensions:
        for d in dimensions:
            unit = d["unit"] or "mm"
            lines.append(f"- [ ] {d['key']}: {d['value']} {unit}")
    if constraints:
        for c in constraints:
            unit = f" {c['unit']}" if c["unit"] else ""
            lines.append(f"- [ ] {c['key']}: {c['value']}{unit}")
    if not dimensions and not constraints:
        lines.append("- (no dimensions/constraints recorded)")
    lines.append("")

    # Visual / feature requirements — checked SECOND
    lines.append("## Visual & Feature Requirements (verify SECOND)")
    lines.append("Check these after functional requirements pass.")
    lines.append("")
    if features:
        for f in features:
            unit = f" {f['unit']}" if f["unit"] else ""
            lines.append(f"- [ ] {f['key']}: {f['value']}{unit}")
    else:
        lines.append("- (no specific visual requirements recorded)")
    lines.append("")

    # Verification protocol
    lines.append("## Verification Protocol")
    lines.append("After EVERY build, perform this check:")
    lines.append("")
    lines.append("### Step 1: Read build results")
    lines.append("- Compare bounding box to expected overall dimensions")
    lines.append("- Check topology (face/edge count) — does it match expected complexity?")
    lines.append("- Verify cylindrical features match expected holes/bosses")
    lines.append("")
    lines.append("### Step 2: Functional scan (screenshot_viewer)")
    lines.append("- Can each referenced component physically fit? (pocket sizes, clearances)")
    lines.append("- Are all mounting/bolt holes present and correctly positioned?")
    lines.append("- Do moving parts have clearance? (slots, channels)")
    lines.append("- Are wall thicknesses adequate for the manufacturing method?")
    lines.append("")
    lines.append("### Step 3: Visual scan")
    lines.append("- Does the overall shape match the user's description?")
    lines.append("- Are chamfers, fillets, and surface features present?")
    lines.append("- Are proportions correct (not too thin/thick)?")
    lines.append("- Is the design clean and manufacturable?")
    lines.append("")

    return "\n".join(lines)

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
        # Write code.py so it can be read back later
        code_path = os.path.join(output_dir, "code.py")
        with open(code_path, "w") as f:
            f.write(code)

        # Inject session paths so code can find component STEPs via __file__
        # and os.path.dirname(__file__) resolves to the output_dir
        preamble = f"""
import os as _os
__file__ = "{code_path}"
_SESSION_DIR = "{session_root or output_dir}"
"""
        export_code = preamble + code + f"""

# ── Auto-export + spatial analysis ──
import cadquery as cq
from collections import Counter
cq.exporters.export(result, "{stl_path}")
cq.exporters.export(result, "{step_path}")
solid = result.val()
bb = solid.BoundingBox()
print(f"DIMS:{{bb.xlen:.2f}}x{{bb.ylen:.2f}}x{{bb.zlen:.2f}}")
# Spatial position (min/max coordinates)
print(f"BBOX:min({{bb.xmin:.2f}},{{bb.ymin:.2f}},{{bb.zmin:.2f}}) max({{bb.xmax:.2f}},{{bb.ymax:.2f}},{{bb.zmax:.2f}})")
# Center of mass
try:
    com = solid.Center()
    print(f"CENTER:{{com.x:.2f}},{{com.y:.2f}},{{com.z:.2f}}")
except:
    pass
# Topology for validation
faces = result.faces().vals()
edges = result.edges().vals()
ft = Counter(f.geomType() for f in faces)
ft_str = ", ".join(f"{{k}}:{{v}}" for k, v in sorted(ft.items()))
print(f"TOPO:{{len(faces)}}f {{len(edges)}}e | {{ft_str}}")
# Detect cylindrical features with positions
cyls = [f for f in faces if f.geomType() == "CYLINDER"]
if cyls:
    try:
        from OCP.BRepAdaptor import BRepAdaptor_Surface
        from OCP.GeomAbs import GeomAbs_Cylinder
        features = []
        for f in cyls:
            a = BRepAdaptor_Surface(f.wrapped)
            if a.GetType() == GeomAbs_Cylinder:
                cyl = a.Cylinder()
                r = round(cyl.Radius(), 2)
                loc = cyl.Location()
                ax = cyl.Axis().Direction()
                features.append((r, round(loc.X(),1), round(loc.Y(),1), round(loc.Z(),1),
                                 round(ax.X(),1), round(ax.Y(),1), round(ax.Z(),1)))
        if features:
            # Group by radius
            from itertools import groupby
            features.sort(key=lambda f: f[0])
            parts = []
            for r, group in groupby(features, key=lambda f: f[0]):
                items = list(group)
                positions = " ".join(f"@({{x}},{{y}},{{z}})" for _, x, y, z, *_ in items)
                axis = items[0][4:]
                ax_str = f"along({{axis[0]}},{{axis[1]}},{{axis[2]}})" if axis != (0,0,1) and axis != (0.0,0.0,1.0) else "vertical"
                parts.append(f"{{len(items)}}x d{{r*2}}mm {{ax_str}} {{positions}}")
            print(f"HOLES:" + " | ".join(parts))
    except:
        pass
"""
        proc = subprocess.run(
            [sys.executable, "-c", export_code],
            capture_output=True, text=True, timeout=60
        )

        if proc.returncode != 0:
            error = proc.stderr[-2000:] if proc.stderr else "Unknown build error"
            return {"success": False, "error": error}

        dims = "unknown"
        topo = ""
        holes = ""
        bbox = ""
        center = ""
        for line in proc.stdout.splitlines():
            if line.startswith("DIMS:"):
                dims = line[5:]
            elif line.startswith("BBOX:"):
                bbox = line[5:]
            elif line.startswith("CENTER:"):
                center = line[7:]
            elif line.startswith("TOPO:"):
                topo = line[5:]
            elif line.startswith("HOLES:"):
                holes = line[6:]

        # Copy to _buffer.stl/_buffer.step in session root
        if session_root:
            for src, name in [(stl_path, "_buffer.stl"), (step_path, "_buffer.step")]:
                if os.path.exists(src):
                    shutil.copy2(src, os.path.join(session_root, name))

        return {"success": True, "dimensions": dims, "topology": topo, "holes": holes, "bbox": bbox, "center": center, "stl_path": stl_path, "step_path": step_path}

    except subprocess.TimeoutExpired:
        return {"success": False, "error": "Build timed out after 60 seconds"}
    except Exception as e:
        return {"success": False, "error": str(e)}
    finally:
        if building_flag and os.path.exists(building_flag):
            os.remove(building_flag)

# ── Model scan (headless f3d rendering) ──

# Engineering views: all 6 orthographic faces + 2 isometric.
# Dead-on orthographic projection for accurate proportions.
# HDRI ambient (-f) + tone mapping (-t) + ambient occlusion (-q) reveals
# surface detail (debossed text, standoffs, pockets) without tilting camera.
SCAN_VIEWS = [
    {"label": "front",      "args": ["--camera-direction", "+Y", "--camera-orthographic"],              "desc": "Front view (XZ plane)"},
    {"label": "back",       "args": ["--camera-direction", "-Y", "--camera-orthographic"],              "desc": "Back view (XZ plane)"},
    {"label": "right",      "args": ["--camera-direction", "-X", "--camera-orthographic"],              "desc": "Right view (YZ plane)"},
    {"label": "left",       "args": ["--camera-direction", "+X", "--camera-orthographic"],              "desc": "Left view (YZ plane)"},
    {"label": "top",        "args": ["--camera-direction", "-Z", "--camera-orthographic", "--up", "+Y"], "desc": "Top view (XY plane, looking down)"},
    {"label": "bottom",     "args": ["--camera-direction", "+Z", "--camera-orthographic", "--up", "+Y"], "desc": "Bottom view (XY plane, looking up)"},
    {"label": "iso-top",    "args": ["--camera-azimuth-angle", "30",  "--camera-elevation-angle", "30"],  "desc": "Isometric top-front"},
    {"label": "iso-bottom", "args": ["--camera-azimuth-angle", "210", "--camera-elevation-angle", "-30"], "desc": "Isometric bottom-back"},
]
SCAN_MATERIAL = ["--no-config", "--color", "#d8d8d8", "--roughness", "1", "--metallic", "0", "-f", "-t", "-q"]

def scan_model(session_dir):
    """Render engineering views of the current model using headless f3d.
    Returns 4 images: front, top, right (orthographic) + isometric.
    Coordinate system: +X=right, +Y=forward, +Z=up."""
    import subprocess
    import base64
    import tempfile
    import shutil

    if not session_dir:
        return [{"type": "text", "text": "No session directory set."}]

    stl_path = os.path.join(session_dir, "_buffer.stl")
    if not os.path.exists(stl_path):
        return [{"type": "text", "text": "No model built yet. Write code first."}]

    f3d_bin = shutil.which("f3d")
    if not f3d_bin:
        return [{"type": "text", "text": "f3d not found. Install f3d for model scanning."}]

    tmp_dir = tempfile.mkdtemp(prefix="mimodel_scan_")
    content = [{"type": "text", "text": "Engineering views (coordinate system: +X=right, +Y=forward, +Z=up):"}]
    errors = []

    try:
        # Launch all renders in parallel
        procs = []
        for view in SCAN_VIEWS:
            out_path = os.path.join(tmp_dir, f"{view['label']}.png")
            cmd = [f3d_bin, "--output", out_path, "--resolution", "800,600",
                   "--no-background", "--up", "+Z"]
            cmd.extend(SCAN_MATERIAL)
            cmd.extend(view["args"])
            cmd.append(stl_path)
            proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            procs.append((proc, out_path, view))

        for proc, out_path, view in procs:
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
                errors.append(f"{view['label']}: timed out")
                continue

            if proc.returncode != 0 or not os.path.exists(out_path):
                errors.append(f"{view['label']}: render failed")
                continue

            with open(out_path, "rb") as f:
                data = base64.standard_b64encode(f.read()).decode("ascii")
            content.append({"type": "text", "text": view["desc"]})
            content.append({"type": "image", "data": data, "mimeType": "image/png"})

        if len(content) <= 1:
            return [{"type": "text", "text": f"All renders failed: {'; '.join(errors)}"}]

        if errors:
            content.append({"type": "text", "text": f"Some views failed: {'; '.join(errors)}"})

        return content

    except Exception as e:
        return [{"type": "text", "text": f"Scan error: {e}"}]
    finally:
        shutil.rmtree(tmp_dir, ignore_errors=True)

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
        # Auto-generate goal.md from spec fields
        if session_dir:
            goal = generate_goal_document(spec_fields)
            goal_path = os.path.join(session_dir, "goal.md")
            os.makedirs(session_dir, exist_ok=True)
            with open(goal_path, "w") as f:
                f.write(goal)
        return [{"type": "text", "text": f"Spec marked complete with {len(spec_fields)} fields. goal.md generated. Awaiting user confirmation to advance.\n{summary}"}]

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
        return scan_model(session_dir)

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
                build_info = f"File written: {rel_path}\nBuild successful! Dimensions: {result['dimensions']}mm."
                if result.get("bbox"):
                    build_info += f"\nBounding box: {result['bbox']}"
                if result.get("center"):
                    build_info += f"\nCenter of mass: ({result['center']})"
                if result.get("topology"):
                    build_info += f"\nTopology: {result['topology']}"
                if result.get("holes"):
                    build_info += f"\nCylindrical features: {result['holes']}"
                build_info += "\nCoordinate system: +X=right, +Y=forward, +Z=up"
                build_info += "\nViewer will auto-reload. Use screenshot_viewer to verify geometry."
                return [{"type": "text", "text": build_info}]
            else:
                error = result['error']
                # Categorize error for Claude
                hint = ""
                error_lower = error.lower()
                if "nameerror" in error_lower or "undefined" in error_lower:
                    hint = "\nHint: A variable or import is missing. Check that all names are defined."
                elif "syntaxerror" in error_lower:
                    hint = "\nHint: Python syntax error. Check indentation, brackets, and colons."
                elif "standard_boolean" in error_lower or "boolean" in error_lower:
                    hint = "\nHint: Boolean operation failed — shapes may not overlap, or one may be empty. Check dimensions and positions."
                elif "no wire" in error_lower or "wire" in error_lower:
                    hint = "\nHint: CadQuery sketch/wire error. Check that profiles are closed and valid."
                elif "timed out" in error_lower:
                    hint = "\nHint: Build took too long. Simplify geometry or reduce fillet/chamfer operations."
                return [{"type": "text", "text": f"File written: {rel_path}\nBuild failed:\n{error[-1500:]}{hint}"}]
        return [{"type": "text", "text": f"File written: {rel_path}"}]

    if name == "list_references":
        ref_dir = _references_dir()
        if not os.path.exists(ref_dir):
            return [{"type": "text", "text": "No reference library found at ~/MiModel/references/"}]
        refs = []
        for f in sorted(os.listdir(ref_dir)):
            if f.endswith(".toml"):
                path = os.path.join(ref_dir, f)
                try:
                    with open(path) as fh:
                        content = fh.read()
                    name_val, category, dims, section = "", "", {}, ""
                    for line in content.split("\n"):
                        line = line.strip()
                        if line.startswith("["):
                            section = line.strip("[]").strip()
                        elif "=" in line and section == "identity":
                            k, v = line.split("=", 1)
                            k, v = k.strip(), v.strip().strip('"')
                            if k == "name": name_val = v
                            elif k == "category": category = v
                        elif "=" in line and section == "dimensions":
                            k, v = line.split("=", 1)
                            k, v = k.strip(), v.strip().strip('"')
                            if k != "units":
                                dims[k] = v
                    dim_summary = ", ".join(f"{k}={v}" for k, v in list(dims.items())[:5])
                    refs.append(f"- **{name_val}** [{category}] ({f[:-5]})\n  {dim_summary}")
                except Exception:
                    refs.append(f"- {f[:-5]} (error reading)")
        if not refs:
            return [{"type": "text", "text": "Reference library is empty."}]
        return [{"type": "text", "text": "## Reference Library\n\n" + "\n".join(refs)}]

    if name == "read_reference":
        query = arguments.get("query", "").lower().replace(" ", "_")
        ref_dir = _references_dir()
        if not os.path.exists(ref_dir):
            return [{"type": "text", "text": "No reference library found at ~/MiModel/references/"}]
        exact = os.path.join(ref_dir, query + ".toml")
        if os.path.exists(exact):
            with open(exact) as f:
                return [{"type": "text", "text": f.read()}]
        matches = [f for f in os.listdir(ref_dir) if f.endswith(".toml") and query in f.lower()]
        if len(matches) == 1:
            with open(os.path.join(ref_dir, matches[0])) as f:
                return [{"type": "text", "text": f.read()}]
        elif len(matches) > 1:
            return [{"type": "text", "text": f"Multiple matches for '{query}': " + ", ".join(m[:-5] for m in matches) + "\nPlease be more specific."}]
        return [{"type": "text", "text": f"No reference found matching '{query}'. Use list_references to see available references."}]

    if name == "fetch_url":
        url = arguments.get("url", "")
        max_length = arguments.get("max_length", 10000)
        if not url:
            return [{"type": "text", "text": "No URL provided."}]
        if not url.startswith(("http://", "https://")):
            return [{"type": "text", "text": "URL must start with http:// or https://"}]
        try:
            req = urllib.request.Request(url, headers={
                "User-Agent": "MiModel/0.3 (CAD reference checker)",
                "Accept": "text/html, text/plain, application/json, */*",
            })
            with urllib.request.urlopen(req, timeout=30) as resp:
                content_type = resp.headers.get("Content-Type", "")
                if "pdf" in content_type.lower() or url.lower().endswith(".pdf"):
                    # Download PDF and extract text with pymupdf
                    try:
                        import pymupdf
                    except ImportError:
                        return [{"type": "text", "text": f"URL points to a PDF but pymupdf is not installed. Run: pip install pymupdf"}]
                    import tempfile
                    pdf_data = resp.read(10 * 1024 * 1024)  # 10MB limit
                    with tempfile.NamedTemporaryFile(suffix=".pdf", delete=False) as tmp:
                        tmp.write(pdf_data)
                        tmp_path = tmp.name
                    try:
                        doc = pymupdf.open(tmp_path)
                        pages = []
                        for page_num in range(min(len(doc), 20)):  # cap at 20 pages
                            page_text = doc[page_num].get_text()
                            if page_text.strip():
                                pages.append(f"--- Page {page_num + 1} ---\n{page_text.strip()}")
                        total_pages = len(doc)
                        doc.close()
                        text = "\n\n".join(pages)
                        if not text:
                            text = "(PDF contains no extractable text — may be a scanned image.)"
                        if len(text) > max_length:
                            text = text[:max_length] + f"\n\n... (truncated at {max_length} chars, {total_pages} pages total)"
                    finally:
                        os.unlink(tmp_path)
                    return [{"type": "text", "text": text}]
                raw = resp.read(max_length * 2)
                charset = "utf-8"
                if "charset=" in content_type:
                    charset = content_type.split("charset=")[-1].split(";")[0].strip()
                text = raw.decode(charset, errors="replace")
                if "html" in content_type.lower() or "<html" in text[:500].lower():
                    text = _re.sub(r'<script[^>]*>.*?</script>', '', text, flags=_re.DOTALL | _re.IGNORECASE)
                    text = _re.sub(r'<style[^>]*>.*?</style>', '', text, flags=_re.DOTALL | _re.IGNORECASE)
                    text = _re.sub(r'<[^>]+>', ' ', text)
                    text = _re.sub(r'\s+', ' ', text).strip()
                if len(text) > max_length:
                    text = text[:max_length] + f"\n\n... (truncated at {max_length} chars)"
                return [{"type": "text", "text": text}]
        except urllib.error.HTTPError as e:
            return [{"type": "text", "text": f"HTTP error {e.code}: {e.reason} for {url}"}]
        except urllib.error.URLError as e:
            return [{"type": "text", "text": f"URL error: {e.reason} for {url}"}]
        except Exception as e:
            return [{"type": "text", "text": f"Failed to fetch URL: {e}"}]

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
        # Return images as visual content so Claude can see them
        if ext in (".png", ".jpg", ".jpeg"):
            import base64
            try:
                with open(full_path, "rb") as f:
                    data = base64.standard_b64encode(f.read()).decode("ascii")
                mime = "image/png" if ext == ".png" else "image/jpeg"
                return [{"type": "image", "data": data, "mimeType": mime}]
            except Exception as e:
                return [{"type": "text", "text": f"Error reading image: {e}"}]
        if ext in (".stl", ".step", ".stp", ".pdf"):
            return [{"type": "text", "text": f"Cannot read binary file ({ext})."}]
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
