import json
import subprocess
import sys
import tempfile
from pathlib import Path


def _make_test_stl(tmpdir: str) -> Path:
    code = "import cadquery as cq\nresult = cq.Workplane('XY').box(20, 10, 5)\n"
    code_path = Path(tmpdir) / "test.py"
    code_path.write_text(code)
    stl_path = Path(tmpdir) / "test.stl"
    subprocess.run(
        [sys.executable, "-m", "ai3d_cad", "build",
         "--code", str(code_path), "--output", str(stl_path), "--engine", "cadquery"],
        capture_output=True,
    )
    return stl_path


def test_info_reports_dimensions():
    with tempfile.TemporaryDirectory() as tmpdir:
        stl_path = _make_test_stl(tmpdir)
        r = subprocess.run(
            [sys.executable, "-m", "ai3d_cad", "info", "--input", str(stl_path)],
            capture_output=True, text=True,
        )
        assert r.returncode == 0
        meta = json.loads(r.stdout)
        assert abs(meta["dimensions"]["x"] - 20.0) < 0.5
        assert abs(meta["dimensions"]["y"] - 10.0) < 0.5
        assert abs(meta["dimensions"]["z"] - 5.0) < 0.5
        assert meta["triangle_count"] > 0
        assert meta["watertight"] is True


def test_info_missing_file():
    r = subprocess.run(
        [sys.executable, "-m", "ai3d_cad", "info", "--input", "/nonexistent.stl"],
        capture_output=True, text=True,
    )
    assert r.returncode == 1
