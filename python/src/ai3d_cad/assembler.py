"""Assembly execution: load component scripts, apply transforms + booleans."""
import json
import sys
import traceback
from pathlib import Path
from typing import Any


def _load_component(path: str):
    """Execute a component .py and return its `result` variable."""
    import cadquery as cq

    code = Path(path).read_text()
    namespace: dict[str, Any] = {"cq": cq, "__name__": "__main__"}
    compiled = compile(code, path, "exec")
    exec(compiled, namespace)
    if "result" not in namespace:
        raise ValueError(f"Component {path} does not define 'result'")
    return namespace["result"]


def _apply_transform(shape, transform: dict):
    """Apply translate/rotate to a CadQuery shape."""
    if "translate" in transform:
        tx, ty, tz = transform["translate"]
        shape = shape.translate((tx, ty, tz))
    if "rotate" in transform:
        rot = transform["rotate"]
        axis = tuple(rot["axis"])
        degrees = rot["degrees"]
        if degrees != 0:
            shape = shape.rotate((0, 0, 0), axis, degrees)
    return shape


def _emit_error(error_type: str, message: str):
    print(json.dumps({"error": message, "error_type": error_type}))


def assemble(manifest_path: str, output_path: str, step_path: str = None) -> int:
    """Execute assembly from manifest. Returns 0 on success, 1 on error."""
    import cadquery as cq

    # Parse manifest
    try:
        manifest_text = Path(manifest_path).read_text()
        manifest = json.loads(manifest_text)
    except FileNotFoundError:
        _emit_error("build", f"Manifest not found: {manifest_path}")
        return 1
    except json.JSONDecodeError as e:
        _emit_error("build", f"Invalid JSON in manifest: {e}")
        return 1

    components = manifest.get("components")
    if not components or not isinstance(components, list):
        _emit_error("build", "Manifest must have a 'components' list")
        return 1

    # Load each component and apply transforms
    loaded: list[dict[str, Any]] = []
    for i, entry in enumerate(components):
        comp_path = entry.get("path")
        if not comp_path:
            _emit_error("build", f"Component {i} missing 'path'")
            return 1

        try:
            shape = _load_component(comp_path)
        except Exception as e:
            traceback.print_exc(file=sys.stderr)
            _emit_error("build", f"Failed to load component {comp_path}: {e}")
            return 1

        transform = entry.get("transform", {})
        if transform:
            try:
                shape = _apply_transform(shape, transform)
            except Exception as e:
                _emit_error("build", f"Transform failed for {comp_path}: {e}")
                return 1

        loaded.append({
            "shape": shape,
            "role": entry.get("role", "base"),
            "target": entry.get("target"),
            "name": entry.get("name", Path(comp_path).stem),
        })

    # Build assembly via boolean operations
    named_shapes: dict[str, Any] = {}
    result_shape = None

    for item in loaded:
        role = item["role"]
        name = item["name"]
        shape = item["shape"]

        if role == "base":
            if result_shape is None:
                result_shape = shape
            else:
                # Additional bases are fused onto the current result
                try:
                    result_shape = result_shape.union(shape)
                except Exception as e:
                    _emit_error("build", f"Union failed for component '{name}': {e}")
                    return 1
            named_shapes[name] = shape

        elif role == "subtract":
            if result_shape is None:
                _emit_error("build", f"Cannot subtract '{name}': no base shape yet")
                return 1
            try:
                result_shape = result_shape.cut(shape)
            except Exception as e:
                _emit_error("build", f"Cut failed for component '{name}': {e}")
                return 1

        elif role == "fuse":
            if result_shape is None:
                _emit_error("build", f"Cannot fuse '{name}': no base shape yet")
                return 1
            try:
                result_shape = result_shape.union(shape)
            except Exception as e:
                _emit_error("build", f"Union failed for component '{name}': {e}")
                return 1

        elif role == "intersect":
            if result_shape is None:
                _emit_error("build", f"Cannot intersect '{name}': no base shape yet")
                return 1
            try:
                result_shape = result_shape.intersect(shape)
            except Exception as e:
                _emit_error("build", f"Intersect failed for component '{name}': {e}")
                return 1

        else:
            _emit_error("build", f"Unknown role '{role}' for component '{name}'")
            return 1

    if result_shape is None:
        _emit_error("build", "No components produced a result")
        return 1

    # Export STL
    out_file = Path(output_path)
    try:
        out_file.parent.mkdir(parents=True, exist_ok=True)
        from cadquery import exporters
        exporters.export(result_shape, str(out_file))
    except Exception as e:
        traceback.print_exc(file=sys.stderr)
        _emit_error("build", f"STL export failed: {e}")
        return 1

    # Optional STEP export
    if step_path is not None:
        try:
            from cadquery import exporters as cq_exporters
            cq_exporters.export(result_shape, step_path, "STEP")
        except Exception as e:
            print(f"Warning: STEP export failed: {e}", file=sys.stderr)

    # Analyze with trimesh, print metadata JSON
    try:
        import trimesh
        mesh = trimesh.load(str(out_file), force="mesh")
        bb = mesh.bounding_box.extents
        metadata: dict[str, Any] = {
            "dimensions": {
                "x": round(float(bb[0]), 2),
                "y": round(float(bb[1]), 2),
                "z": round(float(bb[2]), 2),
            },
            "volume_mm3": round(float(abs(mesh.volume)), 2),
            "triangle_count": len(mesh.faces),
            "watertight": bool(mesh.is_watertight),
        }
    except Exception as e:
        print(f"Warning: mesh analysis failed: {e}", file=sys.stderr)
        metadata = {
            "dimensions": {"x": 0, "y": 0, "z": 0},
            "volume_mm3": 0,
            "triangle_count": 0,
            "watertight": False,
        }

    metadata["engine"] = "cadquery"
    metadata["component_count"] = len(loaded)

    sidecar = Path(str(out_file) + ".json")
    sidecar.write_text(json.dumps(metadata, indent=2))

    print(json.dumps(metadata))
    return 0
