import json
import subprocess
import sys
import tempfile
from pathlib import Path


def test_build_simple_box():
    code = '''
import cadquery as cq
# feature: box 10x10x10mm
result = cq.Workplane("XY").box(10, 10, 10)
'''
    with tempfile.TemporaryDirectory() as tmpdir:
        code_path = Path(tmpdir) / "box.py"
        code_path.write_text(code)
        stl_path = Path(tmpdir) / "box.stl"

        result = subprocess.run(
            [sys.executable, "-m", "ai3d_cad", "build",
             "--code", str(code_path), "--output", str(stl_path), "--engine", "cadquery"],
            capture_output=True, text=True,
        )

        assert result.returncode == 0, f"stderr: {result.stderr}"
        assert stl_path.exists()
        assert stl_path.stat().st_size > 0

        metadata = json.loads(result.stdout)
        assert metadata["engine"] == "cadquery"
        assert metadata["watertight"] is True
        dims = metadata["dimensions"]
        assert abs(dims["x"] - 10.0) < 0.5
        assert abs(dims["y"] - 10.0) < 0.5
        assert abs(dims["z"] - 10.0) < 0.5
        assert metadata["triangle_count"] > 0
        assert metadata["volume_mm3"] > 0

        sidecar = Path(str(stl_path) + ".json")
        assert sidecar.exists()
        sidecar_data = json.loads(sidecar.read_text())
        assert sidecar_data == metadata


def test_build_with_features():
    code = '''
import cadquery as cq
# feature: base plate 20x15x2mm
result = cq.Workplane("XY").box(20, 15, 2)
# feature: center hole 5mm
result = result.faces(">Z").workplane().hole(5)
'''
    with tempfile.TemporaryDirectory() as tmpdir:
        code_path = Path(tmpdir) / "plate.py"
        code_path.write_text(code)
        stl_path = Path(tmpdir) / "plate.stl"

        r = subprocess.run(
            [sys.executable, "-m", "ai3d_cad", "build",
             "--code", str(code_path), "--output", str(stl_path), "--engine", "cadquery"],
            capture_output=True, text=True,
        )
        assert r.returncode == 0
        meta = json.loads(r.stdout)
        assert "base plate 20x15x2mm" in meta["features"]
        assert "center hole 5mm" in meta["features"]


def test_build_syntax_error():
    code = "def foo(\n"
    with tempfile.TemporaryDirectory() as tmpdir:
        code_path = Path(tmpdir) / "bad.py"
        code_path.write_text(code)
        stl_path = Path(tmpdir) / "bad.stl"

        r = subprocess.run(
            [sys.executable, "-m", "ai3d_cad", "build",
             "--code", str(code_path), "--output", str(stl_path), "--engine", "cadquery"],
            capture_output=True, text=True,
        )
        assert r.returncode == 2
        err = json.loads(r.stdout)
        assert err["error_type"] == "syntax"
        assert not stl_path.exists()


def test_build_runtime_error():
    code = '''
import cadquery as cq
result = cq.Workplane("XY").box(10, 10, 10)
result = result.edges().fillet(50)
'''
    with tempfile.TemporaryDirectory() as tmpdir:
        code_path = Path(tmpdir) / "bad_fillet.py"
        code_path.write_text(code)
        stl_path = Path(tmpdir) / "bad_fillet.stl"

        r = subprocess.run(
            [sys.executable, "-m", "ai3d_cad", "build",
             "--code", str(code_path), "--output", str(stl_path), "--engine", "cadquery"],
            capture_output=True, text=True,
        )
        assert r.returncode == 1
        err = json.loads(r.stdout)
        assert err["error_type"] == "build"
