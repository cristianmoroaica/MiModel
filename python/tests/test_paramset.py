import json
import subprocess
import sys
from pathlib import Path


def test_paramset_overrides_value(tmp_path):
    """Component with SIDE = 10.0, override to 20.0. Assert exit 0, STL exists."""
    code = """\
import cadquery as cq
SIDE = 10.0
# feature: simple cube
result = cq.Workplane("XY").box(SIDE, SIDE, SIDE)
"""
    code_path = tmp_path / "cube.py"
    code_path.write_text(code)

    params = {"SIDE": 20.0}
    params_path = tmp_path / "params.json"
    params_path.write_text(json.dumps(params))

    stl_path = tmp_path / "cube.stl"

    r = subprocess.run(
        [sys.executable, "-m", "ai3d_cad", "paramset",
         "--code", str(code_path),
         "--params", str(params_path),
         "--output", str(stl_path)],
        capture_output=True, text=True,
    )
    assert r.returncode == 0, f"stderr: {r.stderr}\nstdout: {r.stdout}"
    assert stl_path.exists()
    assert stl_path.stat().st_size > 0

    metadata = json.loads(r.stdout)
    dims = metadata["dimensions"]
    assert abs(dims["x"] - 20.0) < 0.5
    assert abs(dims["y"] - 20.0) < 0.5
    assert abs(dims["z"] - 20.0) < 0.5


def test_paramset_derived_params_recompute(tmp_path):
    """Derived params recompute: WIDTH=30 => HALF_WIDTH=15 (box z-dim)."""
    code = """\
import cadquery as cq
WIDTH = 10.0
HALF_WIDTH = WIDTH / 2
# feature: box with derived half-width
result = cq.Workplane("XY").box(WIDTH, WIDTH, HALF_WIDTH)
"""
    code_path = tmp_path / "derived.py"
    code_path.write_text(code)

    params = {"WIDTH": 30.0}
    params_path = tmp_path / "params.json"
    params_path.write_text(json.dumps(params))

    stl_path = tmp_path / "derived.stl"

    r = subprocess.run(
        [sys.executable, "-m", "ai3d_cad", "paramset",
         "--code", str(code_path),
         "--params", str(params_path),
         "--output", str(stl_path)],
        capture_output=True, text=True,
    )
    assert r.returncode == 0, f"stderr: {r.stderr}\nstdout: {r.stdout}"

    metadata = json.loads(r.stdout)
    dims = metadata["dimensions"]
    # WIDTH overridden to 30; HALF_WIDTH = WIDTH/2 = 15 (z dimension)
    assert abs(dims["x"] - 30.0) < 0.5
    assert abs(dims["y"] - 30.0) < 0.5
    assert abs(dims["z"] - 15.0) < 0.5


def test_paramset_with_step(tmp_path):
    """Override + STEP export. Assert both STL and STEP exist."""
    code = """\
import cadquery as cq
SIDE = 10.0
result = cq.Workplane("XY").box(SIDE, SIDE, SIDE)
"""
    code_path = tmp_path / "cube.py"
    code_path.write_text(code)

    params = {"SIDE": 15.0}
    params_path = tmp_path / "params.json"
    params_path.write_text(json.dumps(params))

    stl_path = tmp_path / "cube.stl"
    step_path = tmp_path / "cube.step"

    r = subprocess.run(
        [sys.executable, "-m", "ai3d_cad", "paramset",
         "--code", str(code_path),
         "--params", str(params_path),
         "--output", str(stl_path),
         "--step", str(step_path)],
        capture_output=True, text=True,
    )
    assert r.returncode == 0, f"stderr: {r.stderr}\nstdout: {r.stdout}"
    assert stl_path.exists()
    assert step_path.exists()
    assert step_path.stat().st_size > 0


def test_paramset_syntax_error(tmp_path):
    """Bad Python code. Assert exit code 2."""
    code = "def foo(\n"
    code_path = tmp_path / "bad.py"
    code_path.write_text(code)

    params = {}
    params_path = tmp_path / "params.json"
    params_path.write_text(json.dumps(params))

    stl_path = tmp_path / "bad.stl"

    r = subprocess.run(
        [sys.executable, "-m", "ai3d_cad", "paramset",
         "--code", str(code_path),
         "--params", str(params_path),
         "--output", str(stl_path)],
        capture_output=True, text=True,
    )
    assert r.returncode == 2
    err = json.loads(r.stdout)
    assert err["error_type"] == "syntax"
    assert not stl_path.exists()
