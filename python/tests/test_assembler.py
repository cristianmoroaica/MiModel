"""Tests for the assembler module."""
import json
from pathlib import Path

from ai3d_cad.assembler import assemble


def _write_box_component(path: Path, x: float, y: float, z: float) -> None:
    path.write_text(
        f"import cadquery as cq\n"
        f"result = cq.Workplane('XY').box({x}, {y}, {z})\n"
    )


def test_assemble_single_base_component(tmp_path):
    """Single component with role=base produces STL and STEP."""
    comp = tmp_path / "box.py"
    _write_box_component(comp, 10, 10, 10)

    manifest = {
        "components": [
            {"path": str(comp), "role": "base"},
        ]
    }
    manifest_file = tmp_path / "manifest.json"
    manifest_file.write_text(json.dumps(manifest))

    stl_path = tmp_path / "out.stl"
    step_path = tmp_path / "out.step"

    rc = assemble(str(manifest_file), str(stl_path), step_path=str(step_path))

    assert rc == 0, "exit code should be 0"
    assert stl_path.exists(), "STL should be created"
    assert stl_path.stat().st_size > 0
    assert step_path.exists(), "STEP should be created"
    assert step_path.stat().st_size > 0

    # Check sidecar metadata
    sidecar = Path(str(stl_path) + ".json")
    assert sidecar.exists()
    meta = json.loads(sidecar.read_text())
    assert meta["component_count"] == 1
    assert meta["engine"] == "cadquery"
    assert meta["watertight"] is True


def test_assemble_subtract_operation(tmp_path):
    """Outer box minus inner box produces valid STL."""
    outer = tmp_path / "outer.py"
    _write_box_component(outer, 20, 20, 20)

    hole = tmp_path / "hole.py"
    _write_box_component(hole, 10, 10, 10)

    manifest = {
        "components": [
            {"path": str(outer), "role": "base"},
            {"path": str(hole), "role": "subtract"},
        ]
    }
    manifest_file = tmp_path / "manifest.json"
    manifest_file.write_text(json.dumps(manifest))

    stl_path = tmp_path / "out.stl"

    rc = assemble(str(manifest_file), str(stl_path))

    assert rc == 0, "exit code should be 0"
    assert stl_path.exists(), "STL should be created"
    assert stl_path.stat().st_size > 0

    sidecar = Path(str(stl_path) + ".json")
    meta = json.loads(sidecar.read_text())
    # Subtracted shape should have smaller volume than 20^3=8000
    assert meta["volume_mm3"] < 8000
    assert meta["component_count"] == 2


def test_assemble_with_translate(tmp_path):
    """Base plate plus translated pin (fuse) produces valid STL."""
    plate = tmp_path / "plate.py"
    _write_box_component(plate, 30, 30, 2)

    pin = tmp_path / "pin.py"
    _write_box_component(pin, 4, 4, 10)

    manifest = {
        "components": [
            {"path": str(plate), "role": "base"},
            {
                "path": str(pin),
                "role": "fuse",
                "transform": {"translate": [0, 0, 6]},
            },
        ]
    }
    manifest_file = tmp_path / "manifest.json"
    manifest_file.write_text(json.dumps(manifest))

    stl_path = tmp_path / "out.stl"

    rc = assemble(str(manifest_file), str(stl_path))

    assert rc == 0, "exit code should be 0"
    assert stl_path.exists(), "STL should be created"
    assert stl_path.stat().st_size > 0

    sidecar = Path(str(stl_path) + ".json")
    meta = json.loads(sidecar.read_text())
    assert meta["component_count"] == 2
    # Combined shape should be taller than just the plate (z > 2)
    assert meta["dimensions"]["z"] > 2


def test_assemble_invalid_manifest(tmp_path):
    """Bad JSON in manifest returns exit code 1."""
    manifest_file = tmp_path / "manifest.json"
    manifest_file.write_text("{not valid json }")

    stl_path = tmp_path / "out.stl"

    rc = assemble(str(manifest_file), str(stl_path))

    assert rc == 1, "exit code should be 1 for invalid manifest"
    assert not stl_path.exists(), "STL should not be created on error"
