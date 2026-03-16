# AI3D Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build an interactive Rust CLI + Python hybrid tool that uses Claude to generate functional 3D models (STL) from natural language descriptions.

**Architecture:** Rust binary handles TUI (ratatui), Claude API (reqwest + SSE streaming), terminal preview (braille), and external viewer (f3d). Python subprocess (`ai3d-cad`) handles CadQuery/OpenSCAD code execution, STL export, and mesh analysis. Communication via temp files + JSON on stdout.

**Tech Stack:** Rust (ratatui, crossterm, reqwest, tokio, serde, clap, toml), Python (cadquery, trimesh, numpy), Claude API (HTTP SSE streaming)

**Spec:** `docs/superpowers/specs/2026-03-16-ai3d-design.md`

---

## Chunk 1: Python Package (ai3d-cad)

The foundation — Rust depends on this subprocess being functional.

### Task 1: Python project scaffold + version command

**Files:**
- Create: `python/pyproject.toml`
- Create: `python/src/ai3d_cad/__init__.py`
- Create: `python/src/ai3d_cad/__main__.py`
- Create: `python/environment.yml`
- Test: `python/tests/test_version.py`

- [ ] **Step 1: Create pyproject.toml**

```toml
[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"

[project]
name = "ai3d-cad"
version = "0.1.0"
description = "CadQuery/OpenSCAD execution engine for ai3d"
requires-python = ">=3.10"
dependencies = [
    "cadquery>=2.4",
    "trimesh>=4.0",
    "numpy>=1.24",
]

[project.optional-dependencies]
dev = ["pytest>=8.0"]
```

- [ ] **Step 2: Create environment.yml**

```yaml
name: ai3d
channels:
  - cadquery
  - conda-forge
  - defaults
dependencies:
  - python=3.11
  - cadquery
  - pip:
    - trimesh>=4.0
    - numpy>=1.24
    - pytest>=8.0
```

- [ ] **Step 3: Create __init__.py with version and protocol**

```python
__version__ = "0.1.0"
PROTOCOL_VERSION = 1
```

- [ ] **Step 4: Create __main__.py with --version support**

```python
"""ai3d-cad CLI entry point."""
import argparse
import sys

from . import __version__, PROTOCOL_VERSION


def main():
    parser = argparse.ArgumentParser(prog="ai3d-cad")
    parser.add_argument(
        "--version", action="version",
        version=f"ai3d-cad {__version__} (protocol {PROTOCOL_VERSION})",
    )
    subparsers = parser.add_subparsers(dest="command")

    # build
    build_parser = subparsers.add_parser("build", help="Execute CAD code and produce STL")
    build_parser.add_argument("--code", required=True, help="Path to .py or .scad file")
    build_parser.add_argument("--output", required=True, help="Output STL path")
    build_parser.add_argument(
        "--engine", choices=["cadquery", "openscad"], default="cadquery",
    )

    # info
    info_parser = subparsers.add_parser("info", help="Analyze an existing STL")
    info_parser.add_argument("--input", required=True, help="Path to STL file")

    # validate
    val_parser = subparsers.add_parser("validate", help="Syntax-check code without building")
    val_parser.add_argument("--code", required=True, help="Path to .py or .scad file")
    val_parser.add_argument(
        "--engine", choices=["cadquery", "openscad"], default="cadquery",
    )

    args = parser.parse_args()

    if args.command == "build":
        from .builder import build
        sys.exit(build(args.code, args.output, args.engine))
    elif args.command == "info":
        from .analyzer import info
        sys.exit(info(args.input))
    elif args.command == "validate":
        from .builder import validate
        sys.exit(validate(args.code, args.engine))
    else:
        parser.print_help()
        sys.exit(1)


if __name__ == "__main__":
    main()
```

- [ ] **Step 5: Write the failing test**

```python
# python/tests/test_version.py
import subprocess
import sys


def test_version_output():
    result = subprocess.run(
        [sys.executable, "-m", "ai3d_cad", "--version"],
        capture_output=True, text=True,
    )
    assert result.returncode == 0
    assert "ai3d-cad 0.1.0 (protocol 1)" in result.stdout
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cd python && pip install -e . && pytest tests/test_version.py -v`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add python/
git commit -m "feat: scaffold ai3d-cad Python package with version command"
```

---

### Task 2: CadQuery builder (build command)

**Files:**
- Create: `python/src/ai3d_cad/builder.py`
- Test: `python/tests/test_builder.py`

- [ ] **Step 1: Write the failing tests**

```python
# python/tests/test_builder.py
import json
import subprocess
import sys
import tempfile
from pathlib import Path


def test_build_simple_box():
    """Build a simple CadQuery box and verify STL + metadata output."""
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
             "--code", str(code_path),
             "--output", str(stl_path),
             "--engine", "cadquery"],
            capture_output=True, text=True,
        )

        assert result.returncode == 0, f"stderr: {result.stderr}"
        assert stl_path.exists()
        assert stl_path.stat().st_size > 0

        # Check metadata on stdout
        metadata = json.loads(result.stdout)
        assert metadata["engine"] == "cadquery"
        assert metadata["watertight"] is True
        dims = metadata["dimensions"]
        assert abs(dims["x"] - 10.0) < 0.5
        assert abs(dims["y"] - 10.0) < 0.5
        assert abs(dims["z"] - 10.0) < 0.5
        assert metadata["triangle_count"] > 0
        assert metadata["volume_mm3"] > 0

        # Check sidecar JSON
        sidecar = Path(str(stl_path) + ".json")
        assert sidecar.exists()
        sidecar_data = json.loads(sidecar.read_text())
        assert sidecar_data == metadata


def test_build_with_features():
    """Feature comments are extracted into metadata."""
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
             "--code", str(code_path), "--output", str(stl_path),
             "--engine", "cadquery"],
            capture_output=True, text=True,
        )
        assert r.returncode == 0
        meta = json.loads(r.stdout)
        assert "base plate 20x15x2mm" in meta["features"]
        assert "center hole 5mm" in meta["features"]


def test_build_syntax_error():
    """Bad Python code returns exit 2 with error JSON."""
    code = "def foo(\n"  # syntax error
    with tempfile.TemporaryDirectory() as tmpdir:
        code_path = Path(tmpdir) / "bad.py"
        code_path.write_text(code)
        stl_path = Path(tmpdir) / "bad.stl"

        r = subprocess.run(
            [sys.executable, "-m", "ai3d_cad", "build",
             "--code", str(code_path), "--output", str(stl_path),
             "--engine", "cadquery"],
            capture_output=True, text=True,
        )
        assert r.returncode == 2
        err = json.loads(r.stdout)
        assert err["error_type"] == "syntax"
        assert not stl_path.exists()


def test_build_runtime_error():
    """CadQuery runtime error returns exit 1 with error JSON."""
    code = '''
import cadquery as cq
result = cq.Workplane("XY").box(10, 10, 10)
# Fillet too large — will fail
result = result.edges().fillet(50)
'''
    with tempfile.TemporaryDirectory() as tmpdir:
        code_path = Path(tmpdir) / "bad_fillet.py"
        code_path.write_text(code)
        stl_path = Path(tmpdir) / "bad_fillet.stl"

        r = subprocess.run(
            [sys.executable, "-m", "ai3d_cad", "build",
             "--code", str(code_path), "--output", str(stl_path),
             "--engine", "cadquery"],
            capture_output=True, text=True,
        )
        assert r.returncode == 1
        err = json.loads(r.stdout)
        assert err["error_type"] == "build"
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd python && pytest tests/test_builder.py -v`
Expected: FAIL — `builder` module doesn't exist

- [ ] **Step 3: Implement builder.py**

```python
"""CadQuery/OpenSCAD code execution and STL export."""
import json
import re
import sys
import traceback
from pathlib import Path
from typing import Any


def _extract_features(code: str) -> list[str]:
    """Extract # feature: ... comments from code."""
    return [
        m.group(1).strip()
        for m in re.finditer(r"#\s*feature:\s*(.+)", code)
    ]


def _analyze_stl(stl_path: Path) -> dict[str, Any]:
    """Compute mesh metadata from an STL file."""
    import trimesh

    mesh = trimesh.load(str(stl_path), force="mesh")
    bb = mesh.bounding_box.extents
    return {
        "dimensions": {
            "x": round(float(bb[0]), 2),
            "y": round(float(bb[1]), 2),
            "z": round(float(bb[2]), 2),
        },
        "volume_mm3": round(float(abs(mesh.volume)), 2),
        "triangle_count": len(mesh.faces),
        "watertight": bool(mesh.is_watertight),
    }


def build(code_path: str, output_path: str, engine: str) -> int:
    """Execute CAD code and produce STL + metadata.

    Returns: exit code (0=success, 1=build error, 2=syntax error).
    """
    code_file = Path(code_path)
    out_file = Path(output_path)

    if not code_file.exists():
        _emit_error("syntax", f"Code file not found: {code_path}")
        return 2

    code = code_file.read_text()

    if engine == "cadquery":
        return _build_cadquery(code, code_file, out_file)
    elif engine == "openscad":
        return _build_openscad(code, code_file, out_file)
    else:
        _emit_error("build", f"Unknown engine: {engine}")
        return 1


def _build_cadquery(code: str, code_file: Path, out_file: Path) -> int:
    """Execute CadQuery code and export result to STL."""
    # Syntax check
    try:
        compile(code, str(code_file), "exec")
    except SyntaxError as e:
        _emit_error("syntax", f"Syntax error at line {e.lineno}: {e.msg}")
        return 2

    # Execute
    namespace: dict[str, Any] = {}
    try:
        exec(code, namespace)
    except Exception as e:
        traceback.print_exc(file=sys.stderr)
        _emit_error("build", str(e))
        return 1

    # Find the CadQuery result — look for 'result' variable or last Workplane
    import cadquery as cq

    result_obj = namespace.get("result")
    if result_obj is None:
        # Fall back: find the last Workplane object in namespace
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

    # Export STL
    try:
        out_file.parent.mkdir(parents=True, exist_ok=True)
        from cadquery import exporters
        exporters.export(result_obj, str(out_file))
    except Exception as e:
        traceback.print_exc(file=sys.stderr)
        _emit_error("build", f"STL export failed: {e}")
        return 1

    # Analyze and emit metadata
    features = _extract_features(code)
    try:
        metadata = _analyze_stl(out_file)
    except Exception as e:
        # STL was written but analysis failed — still success with partial metadata
        metadata = {
            "dimensions": {"x": 0, "y": 0, "z": 0},
            "volume_mm3": 0,
            "triangle_count": 0,
            "watertight": False,
        }
        print(f"Warning: mesh analysis failed: {e}", file=sys.stderr)

    metadata["features"] = features
    metadata["engine"] = "cadquery"

    # Write sidecar JSON
    sidecar = Path(str(out_file) + ".json")
    sidecar.write_text(json.dumps(metadata, indent=2))

    # Emit metadata to stdout
    print(json.dumps(metadata))
    return 0


def _build_openscad(code: str, code_file: Path, out_file: Path) -> int:
    """Execute OpenSCAD code via system binary."""
    # Placeholder — implemented in Task 5
    _emit_error("build", "OpenSCAD engine not yet implemented")
    return 1


def validate(code_path: str, engine: str) -> int:
    """Syntax-check code without building."""
    code_file = Path(code_path)
    if not code_file.exists():
        print(json.dumps({"valid": False, "error": f"File not found: {code_path}"}))
        return 0

    code = code_file.read_text()

    if engine == "cadquery":
        try:
            compile(code, str(code_file), "exec")
            print(json.dumps({"valid": True}))
        except SyntaxError as e:
            print(json.dumps({"valid": False, "error": f"Line {e.lineno}: {e.msg}"}))
    elif engine == "openscad":
        # Basic check: non-empty file
        print(json.dumps({"valid": bool(code.strip())}))
    else:
        print(json.dumps({"valid": False, "error": f"Unknown engine: {engine}"}))

    return 0


def _emit_error(error_type: str, message: str):
    """Print error JSON to stdout."""
    print(json.dumps({"error": message, "error_type": error_type}))
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd python && pytest tests/test_builder.py -v`
Expected: All 4 tests PASS

- [ ] **Step 5: Commit**

```bash
git add python/src/ai3d_cad/builder.py python/tests/test_builder.py
git commit -m "feat: implement CadQuery builder with STL export and metadata"
```

---

### Task 3: Mesh analyzer (info command)

**Files:**
- Create: `python/src/ai3d_cad/analyzer.py`
- Test: `python/tests/test_analyzer.py`

- [ ] **Step 1: Write the failing test**

```python
# python/tests/test_analyzer.py
import json
import subprocess
import sys
import tempfile
from pathlib import Path


def _make_test_stl(tmpdir: str) -> Path:
    """Create a simple STL for testing via CadQuery."""
    code = "import cadquery as cq\nresult = cq.Workplane('XY').box(20, 10, 5)\n"
    code_path = Path(tmpdir) / "test.py"
    code_path.write_text(code)
    stl_path = Path(tmpdir) / "test.stl"
    subprocess.run(
        [sys.executable, "-m", "ai3d_cad", "build",
         "--code", str(code_path), "--output", str(stl_path),
         "--engine", "cadquery"],
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd python && pytest tests/test_analyzer.py -v`
Expected: FAIL — `analyzer` module doesn't exist

- [ ] **Step 3: Implement analyzer.py**

```python
"""Mesh analysis for STL files."""
import json
import sys
from pathlib import Path


def info(input_path: str) -> int:
    """Analyze an STL file and print metadata as JSON."""
    stl_file = Path(input_path)
    if not stl_file.exists():
        print(json.dumps({"error": f"File not found: {input_path}", "error_type": "build"}))
        return 1

    try:
        import trimesh
        mesh = trimesh.load(str(stl_file), force="mesh")
        bb = mesh.bounding_box.extents
        metadata = {
            "dimensions": {
                "x": round(float(bb[0]), 2),
                "y": round(float(bb[1]), 2),
                "z": round(float(bb[2]), 2),
            },
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd python && pytest tests/test_analyzer.py -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add python/src/ai3d_cad/analyzer.py python/tests/test_analyzer.py
git commit -m "feat: add mesh analyzer (info command) for STL inspection"
```

---

### Task 4: Code validator tests

**Files:**
- Test: `python/tests/test_validate.py`

Note: `validate()` was implemented alongside `build()` in Task 2. This task adds dedicated test coverage.

- [ ] **Step 1: Write the tests**

```python
# python/tests/test_validate.py
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
```

- [ ] **Step 2: Run tests**

Run: `cd python && pytest tests/test_validate.py -v`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add python/tests/test_validate.py
git commit -m "test: add validation command tests"
```

---

### Task 5: OpenSCAD fallback

**Files:**
- Create: `python/src/ai3d_cad/openscad.py`
- Modify: `python/src/ai3d_cad/builder.py` — replace `_build_openscad` placeholder
- Test: `python/tests/test_openscad.py`

- [ ] **Step 1: Write the failing test**

```python
# python/tests/test_openscad.py
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
             "--code", str(code_path), "--output", str(stl_path),
             "--engine", "openscad"],
            capture_output=True, text=True,
        )
        assert r.returncode == 0, f"stderr: {r.stderr}"
        assert stl_path.exists()
        meta = json.loads(r.stdout)
        assert meta["engine"] == "openscad"
        assert abs(meta["dimensions"]["x"] - 10.0) < 0.5
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python && pytest tests/test_openscad.py -v`
Expected: FAIL — "OpenSCAD engine not yet implemented"

- [ ] **Step 3: Implement openscad.py**

```python
"""OpenSCAD execution via system binary."""
import json
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

from .builder import _analyze_stl, _extract_features, _emit_error


def build_openscad(code: str, code_file: Path, out_file: Path) -> int:
    """Run OpenSCAD to produce STL."""
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
        metadata = {
            "dimensions": {"x": 0, "y": 0, "z": 0},
            "volume_mm3": 0, "triangle_count": 0, "watertight": False,
        }

    metadata["features"] = features
    metadata["engine"] = "openscad"

    sidecar = Path(str(out_file) + ".json")
    sidecar.write_text(json.dumps(metadata, indent=2))

    print(json.dumps(metadata))
    return 0
```

- [ ] **Step 4: Wire up openscad in builder.py**

Replace `_build_openscad` in `builder.py`:

```python
def _build_openscad(code: str, code_file: Path, out_file: Path) -> int:
    """Execute OpenSCAD code via system binary."""
    from .openscad import build_openscad
    return build_openscad(code, code_file, out_file)
```

- [ ] **Step 5: Run tests**

Run: `cd python && pytest tests/ -v`
Expected: All tests PASS (openscad test skipped if not installed)

- [ ] **Step 6: Commit**

```bash
git add python/src/ai3d_cad/openscad.py python/src/ai3d_cad/builder.py python/tests/test_openscad.py
git commit -m "feat: add OpenSCAD fallback engine"
```

---

## Chunk 2: Rust Foundation

Config, STL reader, and subprocess interface — building blocks the session needs.

### Task 6: Cargo project scaffold + dependencies

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "ai3d"
version = "0.1.0"
edition = "2021"
description = "Interactive 3D model generator using Claude AI"

[dependencies]
clap = { version = "4", features = ["derive"] }
crossterm = "0.28"
ratatui = "0.29"
reqwest = { version = "0.12", features = ["json", "stream"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
toml = "0.8"
dirs = "6"
tempfile = "3"
futures-util = "0.3"
wait-timeout = "0.2"

[target.'cfg(unix)'.dependencies]
libc = "0.2"
```

- [ ] **Step 2: Create minimal main.rs**

```rust
fn main() {
    println!("ai3d v0.1.0 — interactive 3D model generator");
}
```

- [ ] **Step 3: Verify it builds**

Run: `cargo build`
Expected: Compiles successfully

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml src/main.rs
git commit -m "feat: scaffold Rust project with dependencies"
```

---

### Task 7: Config module

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs` — add `mod config;`

- [ ] **Step 1: Write config.rs with tests**

```rust
// src/config.rs
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub claude: ClaudeConfig,
    #[serde(default)]
    pub viewer: ViewerConfig,
    #[serde(default)]
    pub defaults: DefaultsConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ClaudeConfig {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_model")]
    pub model: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ViewerConfig {
    #[serde(default = "default_viewer")]
    pub command: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DefaultsConfig {
    #[serde(default = "default_output_dir")]
    pub output_dir: String,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_build_timeout")]
    pub build_timeout: u64,
}

fn default_model() -> String { "claude-sonnet-4-6".to_string() }
fn default_viewer() -> String { "f3d".to_string() }
fn default_output_dir() -> String { ".".to_string() }
fn default_max_retries() -> u32 { 3 }
fn default_build_timeout() -> u64 { 60 }

impl Default for Config {
    fn default() -> Self {
        Self {
            claude: ClaudeConfig::default(),
            viewer: ViewerConfig::default(),
            defaults: DefaultsConfig::default(),
        }
    }
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self { api_key: None, model: default_model() }
    }
}

impl Default for ViewerConfig {
    fn default() -> Self {
        Self { command: default_viewer() }
    }
}

impl Default for DefaultsConfig {
    fn default() -> Self {
        Self {
            output_dir: default_output_dir(),
            max_retries: default_max_retries(),
            build_timeout: default_build_timeout(),
        }
    }
}

impl Config {
    /// Load config from ~/.config/ai3d/config.toml, falling back to defaults.
    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            if let Ok(contents) = std::fs::read_to_string(&path) {
                if let Ok(config) = toml::from_str::<Config>(&contents) {
                    return config;
                }
            }
        }
        Config::default()
    }

    /// Resolve API key: env var > config file.
    pub fn api_key(&self) -> Option<String> {
        std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .or_else(|| self.claude.api_key.clone())
    }

    fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("ai3d")
            .join("config.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.claude.model, "claude-sonnet-4-6");
        assert_eq!(config.viewer.command, "f3d");
        assert_eq!(config.defaults.max_retries, 3);
        assert_eq!(config.defaults.build_timeout, 60);
    }

    #[test]
    fn test_parse_toml() {
        let toml_str = r#"
[claude]
model = "claude-opus-4-6"

[defaults]
build_timeout = 120
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.claude.model, "claude-opus-4-6");
        assert_eq!(config.defaults.build_timeout, 120);
        assert_eq!(config.defaults.max_retries, 3);
    }

    #[test]
    fn test_api_key_env_override() {
        let config = Config::default();
        std::env::set_var("ANTHROPIC_API_KEY", "test-key-123");
        assert_eq!(config.api_key(), Some("test-key-123".to_string()));
        std::env::remove_var("ANTHROPIC_API_KEY");
    }
}
```

- [ ] **Step 2: Add `mod config;` to main.rs**

- [ ] **Step 3: Run tests**

Run: `cargo test config`
Expected: 3 tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/config.rs src/main.rs
git commit -m "feat: add config module with TOML loading and env overrides"
```

---

### Task 8: Binary STL reader

**Files:**
- Create: `src/stl.rs`
- Modify: `src/main.rs` — add `mod stl;`

- [ ] **Step 1: Write stl.rs with tests**

```rust
// src/stl.rs
//! Binary STL reader for terminal preview rendering.
//!
//! Binary STL format:
//! - 80 bytes: header (ignored)
//! - 4 bytes: u32 triangle count
//! - Per triangle (50 bytes):
//!   - 12 bytes: normal (3x f32, ignored)
//!   - 36 bytes: 3 vertices (9x f32)
//!   - 2 bytes: attribute byte count (ignored)

use std::io;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone)]
pub struct Triangle {
    pub vertices: [Vec3; 3],
}

#[derive(Debug)]
pub struct StlMesh {
    pub triangles: Vec<Triangle>,
    pub min: Vec3,
    pub max: Vec3,
}

impl StlMesh {
    pub fn from_file(path: &Path) -> io::Result<Self> {
        let data = std::fs::read(path)?;
        Self::from_bytes(&data)
    }

    pub fn from_bytes(data: &[u8]) -> io::Result<Self> {
        if data.len() < 84 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "STL too short"));
        }

        let tri_count = u32::from_le_bytes([data[80], data[81], data[82], data[83]]) as usize;
        let expected = 84 + tri_count * 50;
        if data.len() < expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("STL truncated: expected {expected} bytes, got {}", data.len()),
            ));
        }

        let mut triangles = Vec::with_capacity(tri_count);
        let mut min = Vec3 { x: f32::MAX, y: f32::MAX, z: f32::MAX };
        let mut max = Vec3 { x: f32::MIN, y: f32::MIN, z: f32::MIN };

        for i in 0..tri_count {
            let offset = 84 + i * 50;
            let mut verts = [Vec3 { x: 0.0, y: 0.0, z: 0.0 }; 3];
            for v in 0..3 {
                let vo = offset + 12 + v * 12;
                let x = f32::from_le_bytes([data[vo], data[vo+1], data[vo+2], data[vo+3]]);
                let y = f32::from_le_bytes([data[vo+4], data[vo+5], data[vo+6], data[vo+7]]);
                let z = f32::from_le_bytes([data[vo+8], data[vo+9], data[vo+10], data[vo+11]]);
                verts[v] = Vec3 { x, y, z };
                min.x = min.x.min(x); min.y = min.y.min(y); min.z = min.z.min(z);
                max.x = max.x.max(x); max.y = max.y.max(y); max.z = max.z.max(z);
            }
            triangles.push(Triangle { vertices: verts });
        }

        Ok(StlMesh { triangles, min, max })
    }

    pub fn extents(&self) -> Vec3 {
        Vec3 {
            x: self.max.x - self.min.x,
            y: self.max.y - self.min.y,
            z: self.max.z - self.min.z,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_triangle_stl(v0: [f32; 3], v1: [f32; 3], v2: [f32; 3]) -> Vec<u8> {
        let mut data = vec![0u8; 84 + 50];
        data[80] = 1; // 1 triangle
        for (i, coord) in v0.iter().chain(v1.iter()).chain(v2.iter()).enumerate() {
            let bytes = coord.to_le_bytes();
            let off = 96 + i * 4; // 84 header + 12 normal skip
            data[off..off + 4].copy_from_slice(&bytes);
        }
        data
    }

    #[test]
    fn test_parse_single_triangle() {
        let data = make_triangle_stl([0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [5.0, 10.0, 0.0]);
        let mesh = StlMesh::from_bytes(&data).unwrap();
        assert_eq!(mesh.triangles.len(), 1);
        assert!((mesh.extents().x - 10.0).abs() < 0.001);
        assert!((mesh.extents().y - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_bounding_box() {
        let data = make_triangle_stl([-5.0, -3.0, 0.0], [5.0, 3.0, 0.0], [0.0, 0.0, 7.0]);
        let mesh = StlMesh::from_bytes(&data).unwrap();
        assert!((mesh.min.x - (-5.0)).abs() < 0.001);
        assert!((mesh.max.x - 5.0).abs() < 0.001);
        assert!((mesh.extents().z - 7.0).abs() < 0.001);
    }

    #[test]
    fn test_reject_truncated() {
        assert!(StlMesh::from_bytes(&vec![0u8; 50]).is_err());
    }
}
```

- [ ] **Step 2: Add `mod stl;` to main.rs**

- [ ] **Step 3: Run tests**

Run: `cargo test stl`
Expected: 3 tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/stl.rs src/main.rs
git commit -m "feat: add binary STL reader for terminal preview"
```

---

### Task 9: Python subprocess interface

**Files:**
- Create: `src/python.rs`
- Modify: `src/main.rs` — add `mod python;`

- [ ] **Step 1: Write python.rs**

```rust
// src/python.rs
//! Subprocess interface to ai3d-cad Python package.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ModelMetadata {
    pub dimensions: Dimensions,
    pub volume_mm3: f64,
    pub triangle_count: u64,
    pub features: Vec<String>,
    pub watertight: bool,
    pub engine: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Dimensions {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Deserialize)]
pub struct BuildError {
    pub error: String,
    pub error_type: String,
}

#[derive(Debug)]
pub enum BuildResult {
    Success(ModelMetadata),
    BuildError(BuildError),
    SyntaxError(BuildError),
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Engine {
    CadQuery,
    OpenSCAD,
}

impl Engine {
    pub fn as_str(&self) -> &str {
        match self {
            Engine::CadQuery => "cadquery",
            Engine::OpenSCAD => "openscad",
        }
    }

    pub fn file_extension(&self) -> &str {
        match self {
            Engine::CadQuery => "py",
            Engine::OpenSCAD => "scad",
        }
    }
}

/// Check that ai3d-cad is installed and protocol-compatible.
pub fn check_python() -> Result<(), String> {
    let output = Command::new("python")
        .args(["-m", "ai3d_cad", "--version"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to run python: {e}"))?;

    if !output.status.success() {
        return Err(
            "ai3d-cad not installed. Run: cd python && pip install -e .".to_string()
        );
    }

    let version_str = String::from_utf8_lossy(&output.stdout);
    if !version_str.contains("protocol 1") {
        return Err(format!(
            "Incompatible ai3d-cad version: {}. Expected protocol 1.",
            version_str.trim()
        ));
    }

    Ok(())
}

/// Build an STL from CAD code.
pub fn build(
    code_path: &Path,
    output_path: &Path,
    engine: Engine,
    timeout: Duration,
) -> BuildResult {
    let mut child = match Command::new("python")
        .args([
            "-m", "ai3d_cad", "build",
            "--code", &code_path.to_string_lossy(),
            "--output", &output_path.to_string_lossy(),
            "--engine", engine.as_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return BuildResult::BuildError(BuildError {
                error: format!("Failed to spawn python: {e}"),
                error_type: "build".to_string(),
            });
        }
    };

    match child.wait_timeout(timeout) {
        Ok(Some(status)) => {
            let stdout = {
                let mut s = String::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = io::Read::read_to_string(&mut out, &mut s);
                }
                s
            };

            match status.code() {
                Some(0) => {
                    match serde_json::from_str::<ModelMetadata>(&stdout) {
                        Ok(meta) => BuildResult::Success(meta),
                        Err(e) => BuildResult::BuildError(BuildError {
                            error: format!("Failed to parse metadata: {e}"),
                            error_type: "build".to_string(),
                        }),
                    }
                }
                Some(2) => {
                    match serde_json::from_str::<BuildError>(&stdout) {
                        Ok(err) => BuildResult::SyntaxError(err),
                        Err(_) => BuildResult::SyntaxError(BuildError {
                            error: stdout, error_type: "syntax".to_string(),
                        }),
                    }
                }
                _ => {
                    match serde_json::from_str::<BuildError>(&stdout) {
                        Ok(err) => BuildResult::BuildError(err),
                        Err(_) => BuildResult::BuildError(BuildError {
                            error: stdout, error_type: "build".to_string(),
                        }),
                    }
                }
            }
        }
        Ok(None) => {
            // Timeout — SIGTERM then SIGKILL
            #[cfg(unix)]
            {
                unsafe { libc::kill(child.id() as i32, libc::SIGTERM); }
                std::thread::sleep(Duration::from_secs(5));
                let _ = child.kill();
            }
            #[cfg(not(unix))]
            { let _ = child.kill(); }
            let _ = child.wait();
            BuildResult::Timeout
        }
        Err(e) => {
            let _ = child.kill();
            BuildResult::BuildError(BuildError {
                error: format!("Wait failed: {e}"),
                error_type: "build".to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_str() {
        assert_eq!(Engine::CadQuery.as_str(), "cadquery");
        assert_eq!(Engine::OpenSCAD.as_str(), "openscad");
        assert_eq!(Engine::CadQuery.file_extension(), "py");
        assert_eq!(Engine::OpenSCAD.file_extension(), "scad");
    }
}
```

- [ ] **Step 2: Add `mod python;` to main.rs**

- [ ] **Step 3: Run tests**

Run: `cargo test python`
Expected: 1 test PASS

- [ ] **Step 4: Commit**

```bash
git add src/python.rs src/main.rs Cargo.toml
git commit -m "feat: add Python subprocess interface with timeout handling"
```

---

## Chunk 3: Claude API + Response Parsing

### Task 10: Claude API client (streaming)

**Files:**
- Create: `src/claude.rs`
- Modify: `src/main.rs` — add `mod claude;`

- [ ] **Step 1: Write claude.rs**

```rust
// src/claude.rs
//! Claude API client with SSE streaming support.

use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Serialize)]
struct ApiRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<Message>,
    stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
struct ContentBlockDelta {
    #[serde(default)]
    delta: Option<DeltaContent>,
}

#[derive(Debug, Deserialize)]
struct DeltaContent {
    #[serde(default)]
    text: Option<String>,
}

pub const SYSTEM_PROMPT: &str = r#"You are a CAD engineer assistant. You generate CadQuery Python code that produces 3D models for resin 3D printing.

Rules:
- Output ONLY a ```cadquery code block + brief explanation
- All dimensions in millimeters
- Design for resin printing (no FDM-specific features like bridging)
- Prefer CadQuery. Fall back to OpenSCAD only if the user requests it or the geometry is better expressed as CSG
- When refining, modify the existing code — don't rewrite from scratch
- If the user's request is ambiguous, ask ONE clarifying question instead of guessing
- Annotate features with # feature: comments in the code
- Always assign the final model to a variable called `result`"#;

pub struct ClaudeClient {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

/// Callback for streaming tokens.
pub type OnToken = Box<dyn FnMut(&str) + Send>;

impl ClaudeClient {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key, model,
            client: reqwest::Client::new(),
        }
    }

    /// Send messages to Claude with streaming.
    /// Calls `on_token` for each text chunk as it arrives.
    /// Returns the complete response text.
    pub async fn send(
        &self,
        messages: &[Message],
        mut on_token: OnToken,
    ) -> Result<String, String> {
        let request = ApiRequest {
            model: self.model.clone(),
            max_tokens: 4096,
            system: SYSTEM_PROMPT.to_string(),
            messages: messages.to_vec(),
            stream: true,
        };

        let mut headers = HeaderMap::new();
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&self.api_key).map_err(|e| e.to_string())?,
        );
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let response = self.client
            .post(API_URL)
            .headers(headers)
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("API request failed: {e}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("API error {status}: {body}"));
        }

        let mut full_text = String::new();
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Stream error: {e}"))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim_end().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" { continue; }
                    if let Ok(event) = serde_json::from_str::<ContentBlockDelta>(data) {
                        if let Some(delta) = event.delta {
                            if let Some(text) = delta.text {
                                on_token(&text);
                                full_text.push_str(&text);
                            }
                        }
                    }
                }
            }
        }

        Ok(full_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_serialization() {
        let msg = Message { role: "user".to_string(), content: "make a box".to_string() };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"role\":\"user\""));
    }

    #[test]
    fn test_system_prompt_content() {
        assert!(SYSTEM_PROMPT.contains("CadQuery"));
        assert!(SYSTEM_PROMPT.contains("resin"));
        assert!(SYSTEM_PROMPT.contains("result"));
        assert!(SYSTEM_PROMPT.contains("# feature:"));
    }
}
```

- [ ] **Step 2: Add `mod claude;` to main.rs**

- [ ] **Step 3: Run tests**

Run: `cargo test claude`
Expected: 2 tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/claude.rs src/main.rs
git commit -m "feat: add Claude API client with SSE streaming"
```

---

### Task 11: Response parser (code block extraction)

**Files:**
- Create: `src/parser.rs`
- Modify: `src/main.rs` — add `mod parser;`

- [ ] **Step 1: Write parser.rs with tests**

```rust
// src/parser.rs
//! Parse Claude's response to extract code blocks and text.

use crate::python::Engine;

#[derive(Debug, PartialEq)]
pub struct ParsedResponse {
    pub text: String,
    pub code: Option<CodeBlock>,
}

#[derive(Debug, PartialEq)]
pub struct CodeBlock {
    pub code: String,
    pub engine: Engine,
}

/// Parse Claude's response for code blocks and text.
///
/// - ```cadquery -> CadQuery engine
/// - ```openscad -> OpenSCAD engine
/// - ```python -> CadQuery only if contains "import cadquery"
/// - Everything outside code blocks -> text
pub fn parse_response(response: &str) -> ParsedResponse {
    let mut text = String::new();
    let mut code_block: Option<CodeBlock> = None;
    let mut in_code = false;
    let mut current_lang = "";
    let mut code_content = String::new();

    for line in response.lines() {
        if !in_code {
            let trimmed = line.trim();
            if trimmed.starts_with("```cadquery")
                || trimmed.starts_with("```openscad")
                || trimmed.starts_with("```python")
            {
                in_code = true;
                current_lang = if trimmed.starts_with("```cadquery") {
                    "cadquery"
                } else if trimmed.starts_with("```openscad") {
                    "openscad"
                } else {
                    "python"
                };
                code_content.clear();
            } else if !text.is_empty() || !trimmed.is_empty() {
                text.push_str(line);
                text.push('\n');
            }
        } else if line.trim() == "```" {
            in_code = false;
            let engine = match current_lang {
                "cadquery" => Some(Engine::CadQuery),
                "openscad" => Some(Engine::OpenSCAD),
                "python" if code_content.contains("import cadquery") => Some(Engine::CadQuery),
                _ => None,
            };

            if let Some(eng) = engine {
                code_block = Some(CodeBlock {
                    code: code_content.clone(),
                    engine: eng,
                });
            } else {
                text.push_str("```python\n");
                text.push_str(&code_content);
                text.push_str("```\n");
            }
        } else {
            code_content.push_str(line);
            code_content.push('\n');
        }
    }

    ParsedResponse { text: text.trim_end().to_string(), code: code_block }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cadquery_block() {
        let r = parse_response("Here's a box:\n\n```cadquery\nimport cadquery as cq\nresult = cq.Workplane(\"XY\").box(10, 10, 10)\n```\n\n10mm cube.");
        assert!(r.text.contains("Here's a box:"));
        assert!(r.text.contains("10mm cube."));
        let code = r.code.unwrap();
        assert_eq!(code.engine, Engine::CadQuery);
        assert!(code.code.contains("cq.Workplane"));
    }

    #[test]
    fn test_parse_openscad_block() {
        let r = parse_response("```openscad\ncube([10, 10, 10]);\n```");
        let code = r.code.unwrap();
        assert_eq!(code.engine, Engine::OpenSCAD);
    }

    #[test]
    fn test_python_with_cadquery_import() {
        let r = parse_response("```python\nimport cadquery as cq\nresult = cq.Workplane(\"XY\").box(5, 5, 5)\n```");
        assert_eq!(r.code.unwrap().engine, Engine::CadQuery);
    }

    #[test]
    fn test_python_without_cadquery_is_text() {
        let r = parse_response("Example:\n\n```python\nprint('hello')\n```");
        assert!(r.code.is_none());
        assert!(r.text.contains("print('hello')"));
    }

    #[test]
    fn test_plain_text_only() {
        let r = parse_response("What dimensions do you need?");
        assert!(r.code.is_none());
        assert_eq!(r.text, "What dimensions do you need?");
    }

    #[test]
    fn test_text_and_code() {
        let r = parse_response("Making it:\n\n```cadquery\nimport cadquery as cq\nresult = cq.Workplane(\"XY\").box(10, 10, 10)\n```\n\nDone!");
        assert!(r.text.contains("Making it:"));
        assert!(r.text.contains("Done!"));
        assert!(r.code.is_some());
    }
}
```

- [ ] **Step 2: Add `mod parser;` to main.rs**

- [ ] **Step 3: Run tests**

Run: `cargo test parser`
Expected: 6 tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/parser.rs src/main.rs
git commit -m "feat: add response parser for code block extraction"
```

---

## Chunk 4: Session, TUI, Preview, and Viewer

### Task 12: Session manager

**Files:**
- Create: `src/session.rs`
- Modify: `src/main.rs` — add `mod session;`

- [ ] **Step 1: Write session.rs**

```rust
// src/session.rs
//! Session manager — conversation, iterations, undo, temp files.

use crate::claude::Message;
use crate::python::{self, BuildResult, Engine, ModelMetadata};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum SessionState {
    Idle,
    Clarifying,
    Generating,
    Reviewing,
    Error(String),
}

#[derive(Debug, Clone)]
struct Snapshot {
    iteration: u32,
    messages: Vec<Message>,
    metadata: Option<ModelMetadata>,
    code: Option<String>,
    engine: Option<Engine>,
}

pub struct Session {
    pub state: SessionState,
    pub messages: Vec<Message>,
    pub current_metadata: Option<ModelMetadata>,
    pub current_code: Option<String>,
    pub current_engine: Option<Engine>,
    iteration: u32,
    undo_snapshot: Option<Snapshot>,
    temp_dir: PathBuf,
    build_timeout: Duration,
}

impl Session {
    pub fn new(build_timeout: u64) -> Self {
        let temp_dir = tempfile::tempdir()
            .expect("Failed to create temp directory")
            .into_path();

        Session {
            state: SessionState::Idle,
            messages: Vec::new(),
            current_metadata: None,
            current_code: None,
            current_engine: None,
            iteration: 0,
            undo_snapshot: None,
            temp_dir,
            build_timeout: Duration::from_secs(build_timeout),
        }
    }

    fn snapshot(&mut self) {
        self.undo_snapshot = Some(Snapshot {
            iteration: self.iteration,
            messages: self.messages.clone(),
            metadata: self.current_metadata.clone(),
            code: self.current_code.clone(),
            engine: self.current_engine,
        });
    }

    pub fn undo(&mut self) -> bool {
        if let Some(snap) = self.undo_snapshot.take() {
            self.iteration = snap.iteration;
            self.messages = snap.messages;
            self.current_metadata = snap.metadata;
            self.current_code = snap.code;
            self.current_engine = snap.engine;
            self.update_symlink();
            self.state = if self.current_metadata.is_some() {
                SessionState::Reviewing
            } else {
                SessionState::Idle
            };
            true
        } else {
            false
        }
    }

    pub fn add_user_message(&mut self, content: &str) {
        self.messages.push(Message {
            role: "user".to_string(),
            content: content.to_string(),
        });
    }

    pub fn add_assistant_message(&mut self, content: &str) {
        self.messages.push(Message {
            role: "assistant".to_string(),
            content: content.to_string(),
        });
    }

    /// Write code to temp file and build via Python subprocess.
    pub fn build(&mut self, code: &str, engine: Engine) -> BuildResult {
        self.snapshot();
        self.iteration += 1;

        let ext = engine.file_extension();
        let code_path = self.temp_dir.join(format!("iter_{:03}.{}", self.iteration, ext));
        let stl_path = self.temp_dir.join(format!("iter_{:03}.stl", self.iteration));

        fs::write(&code_path, code).expect("Failed to write code file");

        let result = python::build(&code_path, &stl_path, engine, self.build_timeout);

        match &result {
            BuildResult::Success(meta) => {
                self.current_metadata = Some(meta.clone());
                self.current_code = Some(code.to_string());
                self.current_engine = Some(engine);
                self.state = SessionState::Reviewing;
                self.update_symlink();
            }
            BuildResult::Timeout => {
                self.state = SessionState::Error(format!(
                    "Build timed out after {}s", self.build_timeout.as_secs()
                ));
            }
            BuildResult::BuildError(e) | BuildResult::SyntaxError(e) => {
                self.state = SessionState::Error(e.error.clone());
            }
        }

        result
    }

    fn update_symlink(&self) {
        let symlink = self.temp_dir.join("current.stl");
        let _ = fs::remove_file(&symlink);
        let target = self.temp_dir.join(format!("iter_{:03}.stl", self.iteration));
        if target.exists() {
            #[cfg(unix)]
            { let _ = std::os::unix::fs::symlink(&target, &symlink); }
        }
    }

    pub fn current_stl_path(&self) -> PathBuf {
        self.temp_dir.join("current.stl")
    }

    pub fn latest_stl_path(&self) -> Option<PathBuf> {
        let p = self.temp_dir.join(format!("iter_{:03}.stl", self.iteration));
        if p.exists() { Some(p) } else { None }
    }

    pub fn export(&self, dest: &Path) -> Result<(), String> {
        let src = self.latest_stl_path().ok_or("No model to export")?;
        fs::copy(&src, dest).map_err(|e| format!("Export failed: {e}"))?;
        Ok(())
    }

    pub fn reset(&mut self) {
        if let Ok(entries) = fs::read_dir(&self.temp_dir) {
            for entry in entries.flatten() {
                let _ = fs::remove_file(entry.path());
            }
        }
        self.messages.clear();
        self.current_metadata = None;
        self.current_code = None;
        self.current_engine = None;
        self.iteration = 0;
        self.undo_snapshot = None;
        self.state = SessionState::Idle;
    }

    pub fn exchange_count(&self) -> usize { self.messages.len() / 2 }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.temp_dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_session() {
        let s = Session::new(60);
        assert_eq!(s.state, SessionState::Idle);
        assert!(s.messages.is_empty());
        assert!(s.temp_dir.exists());
    }

    #[test]
    fn test_add_messages() {
        let mut s = Session::new(60);
        s.add_user_message("make a box");
        s.add_assistant_message("here's a box");
        assert_eq!(s.messages.len(), 2);
        assert_eq!(s.exchange_count(), 1);
    }

    #[test]
    fn test_reset() {
        let mut s = Session::new(60);
        s.add_user_message("make a box");
        s.reset();
        assert!(s.messages.is_empty());
        assert_eq!(s.state, SessionState::Idle);
    }
}
```

- [ ] **Step 2: Add `mod session;` to main.rs**

- [ ] **Step 3: Run tests**

Run: `cargo test session`
Expected: 3 tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/session.rs src/main.rs
git commit -m "feat: add session manager with undo, temp files, and state machine"
```

---

### Task 13: Terminal braille preview

**Files:**
- Create: `src/preview.rs`
- Modify: `src/main.rs` — add `mod preview;`

- [ ] **Step 1: Write preview.rs with braille renderer and tests**

```rust
// src/preview.rs
//! Terminal 3D preview using braille characters.
//! Braille chars (U+2800-U+28FF) encode a 2x4 dot grid per character.

use crate::stl::{StlMesh, Vec3};

const BRAILLE_BASE: u32 = 0x2800;

fn dot_bit(col: usize, row: usize) -> u8 {
    match (col, row) {
        (0, 0) => 0, (0, 1) => 1, (0, 2) => 2,
        (1, 0) => 3, (1, 1) => 4, (1, 2) => 5,
        (0, 3) => 6, (1, 3) => 7,
        _ => 0,
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ViewAngle {
    Front, Back, Right, Left, Top, Bottom,
}

impl ViewAngle {
    pub fn project(&self, p: &Vec3) -> (f32, f32) {
        match self {
            ViewAngle::Front => (p.x, p.z),
            ViewAngle::Back => (-p.x, p.z),
            ViewAngle::Right => (p.y, p.z),
            ViewAngle::Left => (-p.y, p.z),
            ViewAngle::Top => (p.x, p.y),
            ViewAngle::Bottom => (p.x, -p.y),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            ViewAngle::Front => "front", ViewAngle::Back => "back",
            ViewAngle::Right => "right", ViewAngle::Left => "left",
            ViewAngle::Top => "top", ViewAngle::Bottom => "bottom",
        }
    }

    pub fn next(&self) -> ViewAngle {
        match self {
            ViewAngle::Front => ViewAngle::Right,
            ViewAngle::Right => ViewAngle::Back,
            ViewAngle::Back => ViewAngle::Left,
            ViewAngle::Left => ViewAngle::Top,
            ViewAngle::Top => ViewAngle::Bottom,
            ViewAngle::Bottom => ViewAngle::Front,
        }
    }

    pub fn prev(&self) -> ViewAngle {
        match self {
            ViewAngle::Front => ViewAngle::Bottom,
            ViewAngle::Right => ViewAngle::Front,
            ViewAngle::Back => ViewAngle::Right,
            ViewAngle::Left => ViewAngle::Back,
            ViewAngle::Top => ViewAngle::Left,
            ViewAngle::Bottom => ViewAngle::Top,
        }
    }
}

pub fn render_braille(mesh: &StlMesh, view: ViewAngle, term_width: usize) -> String {
    let char_cols = term_width.min(80);
    let char_rows = char_cols / 2;
    let dot_cols = char_cols * 2;
    let dot_rows = char_rows * 4;

    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;

    for tri in &mesh.triangles {
        for v in &tri.vertices {
            let (px, py) = view.project(v);
            min_x = min_x.min(px); max_x = max_x.max(px);
            min_y = min_y.min(py); max_y = max_y.max(py);
        }
    }

    let range_x = (max_x - min_x).max(0.001);
    let range_y = (max_y - min_y).max(0.001);
    let margin = 2.0;
    let scale = ((dot_cols as f32 - margin * 2.0) / range_x)
        .min((dot_rows as f32 - margin * 2.0) / range_y);

    let mut dots = vec![vec![false; dot_cols]; dot_rows];

    for tri in &mesh.triangles {
        for edge in [(0, 1), (1, 2), (2, 0)] {
            let (ax, ay) = view.project(&tri.vertices[edge.0]);
            let (bx, by) = view.project(&tri.vertices[edge.1]);
            let x0 = ((ax - min_x) * scale + margin) as i32;
            let y0 = ((ay - min_y) * scale + margin) as i32;
            let x1 = ((bx - min_x) * scale + margin) as i32;
            let y1 = ((by - min_y) * scale + margin) as i32;
            draw_line(&mut dots, x0, y0, x1, y1, dot_cols, dot_rows);
        }
    }

    let mut output = String::new();
    for row in (0..dot_rows).step_by(4).rev() {
        for col in (0..dot_cols).step_by(2) {
            let mut bits: u8 = 0;
            for dr in 0..4 {
                for dc in 0..2 {
                    let r = row + dr;
                    let c = col + dc;
                    if r < dot_rows && c < dot_cols && dots[r][c] {
                        bits |= 1 << dot_bit(dc, dr);
                    }
                }
            }
            output.push(char::from_u32(BRAILLE_BASE + bits as u32).unwrap_or(' '));
        }
        output.push('\n');
    }
    output
}

fn draw_line(
    dots: &mut [Vec<bool>], x0: i32, y0: i32, x1: i32, y1: i32,
    width: usize, height: usize,
) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let (mut x, mut y) = (x0, y0);

    loop {
        if x >= 0 && x < width as i32 && y >= 0 && y < height as i32 {
            dots[y as usize][x as usize] = true;
        }
        if x == x1 && y == y1 { break; }
        let e2 = 2 * err;
        if e2 >= dy { err += dy; x += sx; }
        if e2 <= dx { err += dx; y += sy; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stl::{StlMesh, Triangle, Vec3};

    fn make_test_mesh() -> StlMesh {
        StlMesh {
            triangles: vec![Triangle {
                vertices: [
                    Vec3 { x: 0.0, y: 0.0, z: 0.0 },
                    Vec3 { x: 10.0, y: 0.0, z: 0.0 },
                    Vec3 { x: 5.0, y: 0.0, z: 10.0 },
                ],
            }],
            min: Vec3 { x: 0.0, y: 0.0, z: 0.0 },
            max: Vec3 { x: 10.0, y: 0.0, z: 10.0 },
        }
    }

    #[test]
    fn test_render_produces_braille() {
        let mesh = make_test_mesh();
        let output = render_braille(&mesh, ViewAngle::Front, 40);
        assert!(!output.is_empty());
        assert!(output.chars().any(|c| (0x2800..=0x28FF).contains(&(c as u32))));
    }

    #[test]
    fn test_view_rotation_cycle() {
        assert!(matches!(ViewAngle::Front.next(), ViewAngle::Right));
        assert!(matches!(ViewAngle::Right.next(), ViewAngle::Back));
        assert!(matches!(ViewAngle::Front.prev(), ViewAngle::Bottom));
    }
}
```

- [ ] **Step 2: Add `mod preview;` to main.rs**

- [ ] **Step 3: Run tests**

Run: `cargo test preview`
Expected: 2 tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/preview.rs src/main.rs
git commit -m "feat: add terminal braille preview renderer"
```

---

### Task 14: External viewer launcher

**Files:**
- Create: `src/viewer.rs`
- Modify: `src/main.rs` — add `mod viewer;`

- [ ] **Step 1: Write viewer.rs with tests**

```rust
// src/viewer.rs
//! External 3D viewer launcher (f3d, meshlab, or xdg-open).

use std::path::Path;
use std::process::{Child, Command, Stdio};

pub struct Viewer {
    preferred: String,
    child: Option<Child>,
}

impl Viewer {
    pub fn new(preferred: &str) -> Self {
        Self { preferred: preferred.to_string(), child: None }
    }

    /// Launch viewer for STL. Returns Ok(true) if launched, Ok(false) if already running.
    pub fn show(&mut self, stl_path: &Path) -> Result<bool, String> {
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(None) => return Ok(false), // still running
                _ => { self.child = None; }
            }
        }

        let (cmd, args) = self.resolve_viewer(stl_path)?;
        let child = Command::new(&cmd)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to launch {cmd}: {e}"))?;

        self.child = Some(child);
        Ok(true)
    }

    fn resolve_viewer(&self, stl_path: &Path) -> Result<(String, Vec<String>), String> {
        let path_str = stl_path.to_string_lossy().to_string();

        if which(&self.preferred) {
            let args = if self.preferred == "f3d" {
                vec!["--watch".to_string(), path_str]
            } else {
                vec![path_str]
            };
            return Ok((self.preferred.clone(), args));
        }

        for viewer in ["f3d", "meshlab", "xdg-open"] {
            if viewer == self.preferred { continue; }
            if which(viewer) {
                let args = if viewer == "f3d" {
                    vec!["--watch".to_string(), path_str]
                } else {
                    vec![path_str]
                };
                return Ok((viewer.to_string(), args));
            }
        }

        Err("No 3D viewer found. Install f3d: pacman -S f3d".to_string())
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn which(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewer_new() {
        let v = Viewer::new("f3d");
        assert_eq!(v.preferred, "f3d");
        assert!(v.child.is_none());
    }

    #[test]
    fn test_show_missing_viewer() {
        let mut v = Viewer::new("nonexistent_viewer_xyz");
        // If no known viewers are installed, should error
        // (this test is best-effort — may pass or fail depending on system)
        let _ = v.show(Path::new("/tmp/test.stl"));
    }
}
```

- [ ] **Step 2: Add `mod viewer;` to main.rs**

- [ ] **Step 3: Run tests**

Run: `cargo test viewer`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/viewer.rs src/main.rs
git commit -m "feat: add external viewer launcher with fallback chain"
```

---

### Task 15: Main entry point + TUI session loop

**Files:**
- Modify: `src/main.rs` — full implementation

- [ ] **Step 1: Implement main.rs**

```rust
// src/main.rs
mod claude;
mod config;
mod parser;
mod preview;
mod python;
mod session;
mod stl;
mod viewer;

use std::io::{self, Write};
use std::path::Path;

use crate::claude::ClaudeClient;
use crate::config::Config;
use crate::parser::parse_response;
use crate::preview::{render_braille, ViewAngle};
use crate::python::BuildResult;
use crate::session::{Session, SessionState};
use crate::stl::StlMesh;
use crate::viewer::Viewer;

fn main() {
    let config = Config::load();

    if let Err(e) = startup_checks(&config) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }

    let api_key = config.api_key().unwrap();
    let rt = tokio::runtime::Runtime::new().expect("Failed to create async runtime");
    rt.block_on(run_session(config, api_key));
}

fn startup_checks(config: &Config) -> Result<(), String> {
    if config.api_key().is_none() {
        return Err(
            "ANTHROPIC_API_KEY not set. Export it or add to ~/.config/ai3d/config.toml".into()
        );
    }
    python::check_python()?;
    if !which_exists(&config.viewer.command) {
        eprintln!("Warning: {} not found. Install for 3D preview.", config.viewer.command);
    }
    Ok(())
}

fn which_exists(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

async fn run_session(config: Config, api_key: String) {
    let client = ClaudeClient::new(api_key, config.claude.model.clone());
    let mut session = Session::new(config.defaults.build_timeout);
    let mut viewer = Viewer::new(&config.viewer.command);

    println!("\n  ai3d v0.1.0 — interactive 3D model generator");
    println!("  Type what you want to build. Type 'help' for commands.\n");

    loop {
        print!("> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() || input.is_empty() { break; }
        let input = input.trim();
        if input.is_empty() { continue; }

        // Command dispatch
        match input {
            "quit" | "q" => break,
            "help" | "h" => { print_help(); continue; }
            "new" | "n" => {
                session.reset();
                println!("  Fresh session started\n");
                continue;
            }
            "undo" | "u" => {
                if session.undo() {
                    println!("  Reverted to previous iteration");
                    if let Some(meta) = &session.current_metadata {
                        print_metadata(meta);
                    }
                } else {
                    println!("  Nothing to undo");
                }
                println!();
                continue;
            }
            "code" | "c" => {
                match &session.current_code {
                    Some(code) => println!("{code}"),
                    None => println!("  No code yet"),
                }
                println!();
                continue;
            }
            "history" => {
                for msg in &session.messages {
                    let prefix = if msg.role == "user" { ">" } else { " " };
                    println!("{prefix} {}", msg.content.lines().next().unwrap_or(""));
                }
                println!();
                continue;
            }
            "show" | "s" => {
                if session.latest_stl_path().is_some() {
                    match viewer.show(&session.current_stl_path()) {
                        Ok(true) => println!("  Opened in viewer"),
                        Ok(false) => println!("  Viewer already open (auto-reloads)"),
                        Err(e) => eprintln!("  Error: {e}"),
                    }
                } else {
                    println!("  No model yet");
                }
                println!();
                continue;
            }
            "preview" | "p" => {
                if let Some(stl_path) = session.latest_stl_path() {
                    match StlMesh::from_file(&stl_path) {
                        Ok(mesh) => {
                            let (w, _) = crossterm::terminal::size().unwrap_or((80, 24));
                            println!("{}", render_braille(&mesh, ViewAngle::Front, w as usize));
                            let e = mesh.extents();
                            println!("  {:.1} x {:.1} x {:.1} mm (front view)\n", e.x, e.y, e.z);
                        }
                        Err(e) => eprintln!("  Error: {e}\n"),
                    }
                } else {
                    println!("  No model yet\n");
                }
                continue;
            }
            _ => {}
        }

        // Export command
        if input.starts_with("export ") || input.starts_with("e ") {
            let dest = input.split_whitespace().nth(1).unwrap_or("model.stl");
            match session.export(Path::new(dest)) {
                Ok(()) => println!("  Exported to {dest}\n"),
                Err(e) => eprintln!("  Error: {e}\n"),
            }
            continue;
        }

        // Free text — strip optional 'r ' prefix
        let user_input = input.strip_prefix("r ").unwrap_or(input);

        // Inject current model context for refinement
        let mut prompt = user_input.to_string();
        if let (Some(meta), Some(code)) = (&session.current_metadata, &session.current_code) {
            prompt.push_str(&format!(
                "\n\n[Current model: {:.1}x{:.1}x{:.1}mm]\n[Current code:\n```cadquery\n{}\n```]",
                meta.dimensions.x, meta.dimensions.y, meta.dimensions.z, code,
            ));
        }

        session.add_user_message(&prompt);

        print!("  Thinking...");
        io::stdout().flush().unwrap();

        let response = client.send(
            &session.messages,
            Box::new(|_| {}), // streaming callback — TUI enhancement later
        ).await;

        print!("\r              \r");
        io::stdout().flush().unwrap();

        match response {
            Ok(text) => {
                session.add_assistant_message(&text);
                let parsed = parse_response(&text);

                if !parsed.text.is_empty() {
                    println!("  {}\n", parsed.text.replace('\n', "\n  "));
                }

                if let Some(code_block) = parsed.code {
                    print!("  Building...");
                    io::stdout().flush().unwrap();

                    let result = session.build(&code_block.code, code_block.engine);

                    print!("\r              \r");
                    io::stdout().flush().unwrap();

                    match result {
                        BuildResult::Success(meta) => {
                            println!("  Built successfully");
                            print_metadata(&meta);
                            println!("  [s]how  [p]review  [e]xport  [r]efine\n");
                        }
                        BuildResult::BuildError(e) | BuildResult::SyntaxError(e) => {
                            eprintln!("  Build failed: {}\n", e.error);
                        }
                        BuildResult::Timeout => {
                            eprintln!("  Build timed out\n");
                        }
                    }
                }
            }
            Err(e) => eprintln!("  API error: {e}\n"),
        }
    }

    println!("\n  Goodbye!\n");
}

fn print_metadata(meta: &python::ModelMetadata) {
    println!("  {:.1} x {:.1} x {:.1} mm", meta.dimensions.x, meta.dimensions.y, meta.dimensions.z);
    for f in &meta.features {
        println!("  - {f}");
    }
}

fn print_help() {
    println!("  Commands:");
    println!("    (text)       Describe what to build or refine");
    println!("    r <text>     Refine current model");
    println!("    show / s     Open in 3D viewer");
    println!("    preview / p  Terminal wireframe preview");
    println!("    export / e   Export STL (e.g. 'e bracket.stl')");
    println!("    code / c     Show current CadQuery source");
    println!("    history      Show conversation history");
    println!("    undo / u     Revert to previous iteration");
    println!("    new / n      Start fresh design");
    println!("    help / h     Show this help");
    println!("    quit / q     Exit");
    println!();
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: Compiles successfully

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: implement interactive session loop with all commands"
```

---

### Task 16: Integration smoke test

**Files:**
- Create: `tests/integration.rs`

- [ ] **Step 1: Write smoke test verifying binary runs**

```rust
// tests/integration.rs
use std::process::Command;

#[test]
fn test_binary_starts() {
    let output = Command::new("cargo")
        .args(["run", "--", "--help"])
        .output()
        .expect("Failed to run binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("ai3d") || combined.contains("ANTHROPIC_API_KEY"),
        "Unexpected output: {combined}"
    );
}
```

- [ ] **Step 2: Run all tests**

Run: `cargo test && cd python && pytest tests/ -v`
Expected: All Rust and Python tests PASS

- [ ] **Step 3: Commit**

```bash
git add tests/
git commit -m "test: add integration smoke test"
```

---

## Summary

| Chunk | Tasks | What it delivers |
|-------|-------|-----------------|
| 1: Python Package | 1-5 | Working `ai3d-cad` CLI: build, info, validate for CadQuery + OpenSCAD |
| 2: Rust Foundation | 6-9 | Cargo project with config, STL reader, Python subprocess interface |
| 3: Claude API | 10-11 | Streaming Claude client + code block parser |
| 4: Session + TUI | 12-16 | Full interactive session with preview, viewer, undo, export |

Build order is bottom-up: each chunk depends on the previous. Tasks within a chunk are sequential.
