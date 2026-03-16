"""CadQuery/OpenSCAD code execution and STL export."""
import json
import re
import sys
import traceback
from pathlib import Path
from typing import Any


def _extract_features(code: str) -> list[str]:
    """Extract # feature: ... comments from code."""
    return [
        m.group(1).strip()
        for m in re.finditer(r"#\s*feature:\s*(.+)", code)
    ]


def _analyze_stl(stl_path: Path) -> dict[str, Any]:
    """Compute mesh metadata from an STL file."""
    import trimesh

    mesh = trimesh.load(str(stl_path), force="mesh")
    bb = mesh.bounding_box.extents
    return {
        "dimensions": {
            "x": round(float(bb[0]), 2),
            "y": round(float(bb[1]), 2),
            "z": round(float(bb[2]), 2),
        },
        "volume_mm3": round(float(abs(mesh.volume)), 2),
        "triangle_count": len(mesh.faces),
        "watertight": bool(mesh.is_watertight),
    }


def build(code_path: str, output_path: str, engine: str, step_path: str | None = None) -> int:
    code_file = Path(code_path)
    out_file = Path(output_path)

    if not code_file.exists():
        _emit_error("syntax", f"Code file not found: {code_path}")
        return 2

    code = code_file.read_text()

    if engine == "cadquery":
        return _build_cadquery(code, code_file, out_file, step_path=step_path)
    elif engine == "openscad":
        return _build_openscad(code, code_file, out_file)
    else:
        _emit_error("build", f"Unknown engine: {engine}")
        return 1


def _build_cadquery(code: str, code_file: Path, out_file: Path, step_path: str | None = None) -> int:
    try:
        compile(code, str(code_file), "exec")
    except SyntaxError as e:
        _emit_error("syntax", f"Syntax error at line {e.lineno}: {e.msg}")
        return 2

    namespace: dict[str, Any] = {}
    try:
        exec(code, namespace)
    except Exception as e:
        traceback.print_exc(file=sys.stderr)
        _emit_error("build", str(e))
        return 1

    import cadquery as cq

    result_obj = namespace.get("result")
    if result_obj is None:
        for val in reversed(list(namespace.values())):
            if isinstance(val, cq.Workplane):
                result_obj = val
                break

    if result_obj is None:
        _emit_error("build", "No 'result' variable found. Assign your model to 'result'.")
        return 1

    if not isinstance(result_obj, cq.Workplane):
        _emit_error("build", f"'result' is {type(result_obj).__name__}, expected cq.Workplane")
        return 1

    try:
        out_file.parent.mkdir(parents=True, exist_ok=True)
        from cadquery import exporters
        exporters.export(result_obj, str(out_file))
    except Exception as e:
        traceback.print_exc(file=sys.stderr)
        _emit_error("build", f"STL export failed: {e}")
        return 1

    if step_path is not None:
        try:
            from cadquery import exporters as cq_exporters
            cq_exporters.export(result_obj, step_path, "STEP")
        except Exception as e:
            print(f"Warning: STEP export failed: {e}", file=sys.stderr)

    features = _extract_features(code)
    try:
        metadata = _analyze_stl(out_file)
    except Exception as e:
        metadata = {"dimensions": {"x": 0, "y": 0, "z": 0}, "volume_mm3": 0, "triangle_count": 0, "watertight": False}
        print(f"Warning: mesh analysis failed: {e}", file=sys.stderr)

    metadata["features"] = features
    metadata["engine"] = "cadquery"

    sidecar = Path(str(out_file) + ".json")
    sidecar.write_text(json.dumps(metadata, indent=2))

    print(json.dumps(metadata))
    return 0


def _build_openscad(code: str, code_file: Path, out_file: Path) -> int:
    """Execute OpenSCAD code via system binary."""
    from .openscad import build_openscad
    return build_openscad(code, code_file, out_file)


def validate(code_path: str, engine: str) -> int:
    code_file = Path(code_path)
    if not code_file.exists():
        print(json.dumps({"valid": False, "error": f"File not found: {code_path}"}))
        return 0

    code = code_file.read_text()

    if engine == "cadquery":
        try:
            compile(code, str(code_file), "exec")
            print(json.dumps({"valid": True}))
        except SyntaxError as e:
            print(json.dumps({"valid": False, "error": f"Line {e.lineno}: {e.msg}"}))
    elif engine == "openscad":
        print(json.dumps({"valid": bool(code.strip())}))
    else:
        print(json.dumps({"valid": False, "error": f"Unknown engine: {engine}"}))

    return 0


def _emit_error(error_type: str, message: str):
    print(json.dumps({"error": message, "error_type": error_type}))
