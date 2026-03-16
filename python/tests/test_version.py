import subprocess
import sys


def test_version_output():
    result = subprocess.run(
        [sys.executable, "-m", "ai3d_cad", "--version"],
        capture_output=True, text=True,
    )
    assert result.returncode == 0
    assert "ai3d-cad 0.1.0 (protocol 1)" in result.stdout
