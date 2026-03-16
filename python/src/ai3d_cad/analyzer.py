"""Mesh analysis for STL files."""
import json
from pathlib import Path


def info(input_path: str) -> int:
    stl_file = Path(input_path)
    if not stl_file.exists():
        print(json.dumps({"error": f"File not found: {input_path}", "error_type": "build"}))
        return 1

    try:
        import trimesh
        mesh = trimesh.load(str(stl_file), force="mesh")
        bb = mesh.bounding_box.extents
        metadata = {
            "dimensions": {"x": round(float(bb[0]), 2), "y": round(float(bb[1]), 2), "z": round(float(bb[2]), 2)},
            "volume_mm3": round(float(abs(mesh.volume)), 2),
            "triangle_count": len(mesh.faces),
            "watertight": bool(mesh.is_watertight),
            "features": [],
            "engine": "unknown",
        }
        print(json.dumps(metadata))
        return 0
    except Exception as e:
        print(json.dumps({"error": str(e), "error_type": "build"}))
        return 1
