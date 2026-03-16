"""OpenSCAD execution via system binary."""
import json
import shutil
import subprocess
from pathlib import Path

from .builder import _analyze_stl, _extract_features, _emit_error


def build_openscad(code: str, code_file: Path, out_file: Path) -> int:
    openscad_bin = shutil.which("openscad")
    if not openscad_bin:
        _emit_error("build", "OpenSCAD not found. Install: pacman -S openscad")
        return 1

    out_file.parent.mkdir(parents=True, exist_ok=True)

    try:
        r = subprocess.run(
            [openscad_bin, "-o", str(out_file), str(code_file)],
            capture_output=True, text=True, timeout=120,
        )
    except subprocess.TimeoutExpired:
        _emit_error("build", "OpenSCAD timed out after 120s")
        return 1

    if r.returncode != 0:
        error_msg = r.stderr.strip() or "OpenSCAD failed with no error message"
        _emit_error("build", error_msg)
        return 1

    if not out_file.exists() or out_file.stat().st_size == 0:
        _emit_error("build", "OpenSCAD produced empty output")
        return 1

    features = _extract_features(code)
    try:
        metadata = _analyze_stl(out_file)
    except Exception:
        metadata = {"dimensions": {"x": 0, "y": 0, "z": 0}, "volume_mm3": 0, "triangle_count": 0, "watertight": False}

    metadata["features"] = features
    metadata["engine"] = "openscad"

    sidecar = Path(str(out_file) + ".json")
    sidecar.write_text(json.dumps(metadata, indent=2))

    print(json.dumps(metadata))
    return 0
