import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import pytest

has_openscad = shutil.which("openscad") is not None


@pytest.mark.skipif(not has_openscad, reason="openscad not installed")
def test_build_openscad_cube():
    code = "cube([10, 10, 10]);\n"
    with tempfile.TemporaryDirectory() as tmpdir:
        code_path = Path(tmpdir) / "cube.scad"
        code_path.write_text(code)
        stl_path = Path(tmpdir) / "cube.stl"

        r = subprocess.run(
            [sys.executable, "-m", "ai3d_cad", "build",
             "--code", str(code_path), "--output", str(stl_path), "--engine", "openscad"],
            capture_output=True, text=True,
        )
        assert r.returncode == 0, f"stderr: {r.stderr}"
        assert stl_path.exists()
        meta = json.loads(r.stdout)
        assert meta["engine"] == "openscad"
        assert abs(meta["dimensions"]["x"] - 10.0) < 0.5
