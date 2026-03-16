import json
import subprocess
import sys
import tempfile
from pathlib import Path


def test_validate_valid_code():
    code = "import cadquery as cq\nresult = cq.Workplane('XY').box(10, 10, 10)\n"
    with tempfile.TemporaryDirectory() as tmpdir:
        p = Path(tmpdir) / "good.py"
        p.write_text(code)
        r = subprocess.run(
            [sys.executable, "-m", "ai3d_cad", "validate",
             "--code", str(p), "--engine", "cadquery"],
            capture_output=True, text=True,
        )
        assert r.returncode == 0
        data = json.loads(r.stdout)
        assert data["valid"] is True


def test_validate_syntax_error():
    code = "def foo(\n"
    with tempfile.TemporaryDirectory() as tmpdir:
        p = Path(tmpdir) / "bad.py"
        p.write_text(code)
        r = subprocess.run(
            [sys.executable, "-m", "ai3d_cad", "validate",
             "--code", str(p), "--engine", "cadquery"],
            capture_output=True, text=True,
        )
        assert r.returncode == 0
        data = json.loads(r.stdout)
        assert data["valid"] is False
        assert "error" in data
