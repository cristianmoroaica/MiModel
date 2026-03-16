"""Parameter override execution for zero-Claude component refinement."""
import json
import sys
import traceback
from pathlib import Path
from typing import Any

from .builder import _extract_features, _analyze_stl, _emit_error


class _LockedNamespace(dict):
    """A namespace dict that ignores re-assignments of locked keys.

    When a component script contains ``SIDE = 10.0`` and we want to override
    SIDE to 20.0, we pre-seed the namespace with {"SIDE": 20.0} and lock that
    key.  When exec runs the script's ``SIDE = 10.0`` assignment it calls
    __setitem__; we silently discard it, keeping the injected value.

    Derived params (e.g. ``HALF_WIDTH = WIDTH / 2``) are NOT locked, so they
    are computed using the overridden value of WIDTH and stored normally.
    """

    def __init__(self, locked: dict[str, Any]):
        super().__init__(locked)
        self._locked: frozenset[str] = frozenset(locked.keys())

    def __setitem__(self, key: str, value: Any) -> None:
        if key in self._locked:
            return  # silently ignore — override wins
        super().__setitem__(key, value)


def paramset(code_path: str, params_path: str, output_path: str, step_path: str | None = None) -> int:
    """Execute component with overridden parameters. Returns 0/1/2."""
    code_file = Path(code_path)
    params_file = Path(params_path)
    out_file = Path(output_path)

    if not code_file.exists():
        _emit_error("syntax", f"Code file not found: {code_path}")
        return 2

    if not params_file.exists():
        _emit_error("syntax", f"Params file not found: {params_path}")
        return 2

    code = code_file.read_text()

    # Syntax-check before executing
    try:
        compiled = compile(code, str(code_file), "exec")
    except SyntaxError as e:
        _emit_error("syntax", f"Syntax error at line {e.lineno}: {e.msg}")
        return 2

    # Load parameter overrides
    try:
        overrides: dict[str, Any] = json.loads(params_file.read_text())
    except json.JSONDecodeError as e:
        _emit_error("syntax", f"Invalid params JSON: {e}")
        return 2

    # Inject overrides into a locked namespace BEFORE executing the script.
    # The _LockedNamespace prevents the script from overwriting injected values.
    # Derived params (e.g. HALF_WIDTH = WIDTH / 2) execute after the locked
    # assignments are skipped, so they recompute using the new base values.
    namespace: dict[str, Any] = _LockedNamespace(overrides)

    try:
        exec(compiled, namespace)
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
