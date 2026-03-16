# Phase Machine Redesign — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace MiModel's monolithic single-shot generation with a 5-phase pipeline (Spec → Decompose → Component → Assembly → Refinement) that builds models component-by-component.

**Architecture:** Introduce a `Phase` state machine that drives the TUI, Claude prompts, and Python execution layer. Each phase has its own system prompt, context contract, and TUI layout. The existing session/storage layer is restructured around per-component directories. The Python `ai3d-cad` package gains `assemble` and `paramset` subcommands plus STEP export.

**Tech Stack:** Rust (ratatui 0.29, crossterm 0.28, serde, toml), Python (cadquery, trimesh), Claude CLI subprocess

**Spec:** `docs/superpowers/specs/2026-03-16-phase-machine-redesign.md`

**Parallelization note:** Chunks 1 and 2 are independent (Rust vs. Python) and can be executed in parallel by separate agents.

---

## File Structure Overview

### New Rust files to create
| File | Responsibility |
|------|---------------|
| `src/phase.rs` | Phase enum, state machine transitions, validation |
| `src/spec.rs` | Spec TOML parsing, serialization, parameter types |
| `src/component.rs` | Component state (pending/building/approved), per-component history, undo |
| `src/assembly.rs` | Assembly manifest generation, progressive rebuild triggers |
| `src/prompt_builder.rs` | Build phase-specific prompts with scoped context |
| `src/tui/spec_panel.rs` | Right panel: live TOML spec preview (Spec phase) |
| `src/tui/component_tree.rs` | Right panel: component dependency tree visualization (Decompose phase) |
| `src/tui/component_list.rs` | Left panel: component list with status badges |
| `src/tui/param_editor.rs` | Right panel: editable parameter list for Refinement |
| `prompts/spec.md` | Spec phase system prompt |
| `prompts/decompose.md` | Decompose phase system prompt |
| `prompts/component.md` | Component phase system prompt |
| `prompts/assembly.md` | Assembly phase system prompt |
| `prompts/refinement.md` | Refinement phase system prompt |

### Existing Rust files to modify
| File | Changes |
|------|---------|
| `src/main.rs` | Replace monolithic event loop with phase-driven dispatch. Add phase keybindings (Alt+1-5, Ctrl+Left/Right). Wire new panels. |
| `src/model_session.rs` | Restructure around phases + components. New `session.json` format with `phase`, `current_component`, `claude_sessions`. Per-component save/load. |
| `src/claude.rs` | Support per-phase/per-component session IDs. Add `send_with_system_prompt()` for phase-specific prompts. |
| `src/python.rs` | Add `assemble()`, `paramset()` functions. Add `--step` support to `build()`. |
| `src/parser.rs` | Add phase-aware parsing: TOML extraction for Decompose, strict cadquery-only for Component. |
| `src/viewer.rs` | Update `working.stl` to be a real file copy (not symlink). Add `working.step` support. |
| `src/storage/project.rs` | Add legacy session detection. Support new directory structure. |
| `src/storage/session.rs` | New `session.json` schema with phase state + component state + per-scope session IDs. |
| `src/tui/mod.rs` | Add Phase to Focus enum. Add new panel module declarations. |
| `src/tui/layout.rs` | Phase-aware layout (different left/right panels per phase). |
| `src/tui/model_panel.rs` | Adapt to show per-component or assembly metadata depending on phase. |
| `src/tui/project_tree.rs` | Show legacy sessions as read-only. |
| `src/tui/conversation.rs` | Scope conversation per phase/component. Support clearing on phase transition. |

### New Python files to create
| File | Responsibility |
|------|---------------|
| `python/src/ai3d_cad/assembler.py` | Assembly: load component scripts, apply transforms + booleans |
| `python/src/ai3d_cad/paramset.py` | Parameter override via namespace injection |
| `python/tests/test_assembler.py` | Assembly tests |
| `python/tests/test_paramset.py` | Paramset tests |

### Existing Python files to modify
| File | Changes |
|------|---------|
| `python/src/ai3d_cad/__main__.py` | Add `assemble` and `paramset` subcommands |
| `python/src/ai3d_cad/builder.py` | Add `--step` flag for STEP export |

### New prompt files to create
| File | Purpose |
|------|---------|
| `prompts/spec.md` | Guided Q&A for spec building |
| `prompts/decompose.md` | Component decomposition from spec |
| `prompts/component.md` | Single component CadQuery generation |
| `prompts/assembly.md` | Assembly code generation |
| `prompts/refinement.md` | Scoped component modification |

### Existing prompt to modify
| File | Changes |
|------|---------|
| `prompts/system.md` | Rename to `prompts/legacy.md` (kept for legacy sessions only) |

---

## Chunk 1: Foundation — Phase State Machine + Spec Types

### Task 1: Create the Phase enum and state machine

**Files:**
- Create: `src/phase.rs`
- Modify: `src/main.rs:1` (add `mod phase;`)

- [ ] **Step 1: Write test for Phase enum and transitions**

Create `src/phase.rs` with tests at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_ordering() {
        assert_eq!(Phase::Spec.index(), 0);
        assert_eq!(Phase::Decompose.index(), 1);
        assert_eq!(Phase::Component.index(), 2);
        assert_eq!(Phase::Assembly.index(), 3);
        assert_eq!(Phase::Refinement.index(), 4);
    }

    #[test]
    fn test_can_advance() {
        assert!(Phase::Spec.can_advance_to(Phase::Decompose));
        assert!(Phase::Decompose.can_advance_to(Phase::Component));
        assert!(!Phase::Spec.can_advance_to(Phase::Component));
    }

    #[test]
    fn test_can_go_back() {
        assert!(Phase::Decompose.can_go_back_to(Phase::Spec));
        assert!(Phase::Component.can_go_back_to(Phase::Decompose));
        assert!(Phase::Assembly.can_go_back_to(Phase::Component));
    }

    #[test]
    fn test_label() {
        assert_eq!(Phase::Spec.label(), "Spec");
        assert_eq!(Phase::Component.label(), "Component");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib phase::tests -q`
Expected: FAIL — module doesn't exist yet

- [ ] **Step 3: Implement Phase enum**

Write `src/phase.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Spec,
    Decompose,
    Component,
    Assembly,
    Refinement,
}

impl Phase {
    pub fn index(self) -> usize {
        match self {
            Phase::Spec => 0,
            Phase::Decompose => 1,
            Phase::Component => 2,
            Phase::Assembly => 3,
            Phase::Refinement => 4,
        }
    }

    pub fn from_index(i: usize) -> Option<Phase> {
        match i {
            0 => Some(Phase::Spec),
            1 => Some(Phase::Decompose),
            2 => Some(Phase::Component),
            3 => Some(Phase::Assembly),
            4 => Some(Phase::Refinement),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Phase::Spec => "Spec",
            Phase::Decompose => "Decompose",
            Phase::Component => "Component",
            Phase::Assembly => "Assembly",
            Phase::Refinement => "Refinement",
        }
    }

    pub fn can_advance_to(self, target: Phase) -> bool {
        target.index() == self.index() + 1
    }

    pub fn can_go_back_to(self, target: Phase) -> bool {
        target.index() < self.index()
    }
}
```

- [ ] **Step 4: Add module declaration to main.rs**

Add `mod phase;` to `src/main.rs` module declarations (after line 10).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib phase::tests -q`
Expected: PASS (4 tests)

- [ ] **Step 6: Commit**

```bash
git add src/phase.rs src/main.rs
git commit -m "feat: add Phase enum with state machine transitions"
```

---

### Task 2: Create the Spec TOML types

**Files:**
- Create: `src/spec.rs`
- Modify: `src/main.rs:1` (add `mod spec;`)

- [ ] **Step 1: Write tests for spec parsing**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_model_only_spec() {
        let toml_str = r#"
[model]
name = "Test Model"
purpose = "Test"
units = "mm"
print_method = "resin"

[model.envelope]
max_x = 42.0
max_y = 42.0
max_z = 14.0

[model.features]
items = ["Feature A", "Feature B"]

[model.constraints]
items = ["Wall >= 1.5mm"]
"#;
        let spec: ModelSpec = toml::from_str(toml_str).unwrap();
        assert_eq!(spec.model.name, "Test Model");
        assert_eq!(spec.model.envelope.max_x, 42.0);
        assert_eq!(spec.model.features.items.len(), 2);
        assert!(spec.components.is_empty());
        assert!(spec.assembly.is_none());
    }

    #[test]
    fn test_parse_full_spec_with_components() {
        let toml_str = r#"
[model]
name = "Test"
purpose = "Test"
units = "mm"
print_method = "resin"

[model.envelope]
max_x = 10.0
max_y = 10.0
max_z = 10.0

[model.features]
items = []

[model.constraints]
items = []

[[components]]
id = "body"
name = "Body"
description = "Main body"
depends_on = []
assembly_op = "none"
assembly_target = ""

[components.parameters]
width = { value = 10.0, unit = "mm", description = "Width" }

[components.constraints]
items = ["Must be > 5mm"]

[assembly]
order = ["body"]
notes = ""
"#;
        let spec: ModelSpec = toml::from_str(toml_str).unwrap();
        assert_eq!(spec.components.len(), 1);
        assert_eq!(spec.components[0].id, "body");
        assert_eq!(spec.components[0].parameters["width"].value, 10.0);
        assert!(spec.assembly.is_some());
    }

    #[test]
    fn test_roundtrip_serialize() {
        let spec = ModelSpec {
            model: Model {
                name: "Test".into(),
                purpose: "Test".into(),
                units: "mm".into(),
                print_method: "resin".into(),
                envelope: Envelope { max_x: 10.0, max_y: 10.0, max_z: 10.0 },
                features: ItemList { items: vec!["F1".into()] },
                constraints: ItemList { items: vec!["C1".into()] },
            },
            components: vec![],
            assembly: None,
        };
        let serialized = toml::to_string_pretty(&spec).unwrap();
        let parsed: ModelSpec = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.model.name, "Test");
    }

    #[test]
    fn test_validate_no_cycles() {
        let spec = ModelSpec {
            model: Model {
                name: "T".into(), purpose: "T".into(), units: "mm".into(),
                print_method: "resin".into(),
                envelope: Envelope { max_x: 1.0, max_y: 1.0, max_z: 1.0 },
                features: ItemList { items: vec![] },
                constraints: ItemList { items: vec![] },
            },
            components: vec![
                Component {
                    id: "a".into(), name: "A".into(), description: "".into(),
                    depends_on: vec![], assembly_op: "none".into(),
                    assembly_target: "".into(),
                    parameters: std::collections::HashMap::new(),
                    constraints: ItemList { items: vec![] },
                },
                Component {
                    id: "b".into(), name: "B".into(), description: "".into(),
                    depends_on: vec!["a".into()], assembly_op: "subtract".into(),
                    assembly_target: "a".into(),
                    parameters: std::collections::HashMap::new(),
                    constraints: ItemList { items: vec![] },
                },
            ],
            assembly: Some(Assembly { order: vec!["a".into(), "b".into()], notes: "".into() }),
        };
        assert!(spec.validate().is_ok());
    }

    #[test]
    fn test_validate_detects_cycle() {
        let spec = ModelSpec {
            model: Model {
                name: "T".into(), purpose: "T".into(), units: "mm".into(),
                print_method: "resin".into(),
                envelope: Envelope { max_x: 1.0, max_y: 1.0, max_z: 1.0 },
                features: ItemList { items: vec![] },
                constraints: ItemList { items: vec![] },
            },
            components: vec![
                Component {
                    id: "a".into(), name: "A".into(), description: "".into(),
                    depends_on: vec!["b".into()], assembly_op: "none".into(),
                    assembly_target: "".into(),
                    parameters: std::collections::HashMap::new(),
                    constraints: ItemList { items: vec![] },
                },
                Component {
                    id: "b".into(), name: "B".into(), description: "".into(),
                    depends_on: vec!["a".into()], assembly_op: "none".into(),
                    assembly_target: "".into(),
                    parameters: std::collections::HashMap::new(),
                    constraints: ItemList { items: vec![] },
                },
            ],
            assembly: None,
        };
        assert!(spec.validate().is_err());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib spec::tests -q`
Expected: FAIL — module doesn't exist

- [ ] **Step 3: Implement Spec types**

Write `src/spec.rs` with `ModelSpec`, `Model`, `Envelope`, `ItemList`, `Component`, `Parameter`, `Assembly` structs. Include `load()`, `save()`, `validate()` (duplicate IDs, missing deps, cycle detection via DFS), and `build_order()` (topological sort from assembly order).

See spec document for exact TOML structure.

- [ ] **Step 4: Add module declaration to main.rs**

Add `mod spec;` to `src/main.rs` module declarations.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib spec::tests -q`
Expected: PASS (5 tests)

- [ ] **Step 6: Commit**

```bash
git add src/spec.rs src/main.rs
git commit -m "feat: add ModelSpec TOML types with validation and cycle detection"
```

---

### Task 3: Create the Component state tracker

**Files:**
- Create: `src/component.rs`
- Modify: `src/main.rs:1` (add `mod component;`)

- [ ] **Step 1: Write tests for ComponentState**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_initial_status() {
        let cs = ComponentState::new("case_body", "Case Body");
        assert_eq!(cs.status, ComponentStatus::Pending);
        assert_eq!(cs.iteration, 0);
    }

    #[test]
    fn test_record_iteration() {
        let tmp = TempDir::new().unwrap();
        let mut cs = ComponentState::new("case_body", "Case Body");
        cs.set_dir(tmp.path().to_path_buf());
        let code = "import cadquery as cq\nresult = cq.Workplane('XY').box(10,10,10)";
        cs.record_iteration(code).unwrap();
        assert_eq!(cs.iteration, 1);
        assert!(tmp.path().join("history/iter_001.py").exists());
    }

    #[test]
    fn test_approve() {
        let tmp = TempDir::new().unwrap();
        let mut cs = ComponentState::new("test", "Test");
        cs.set_dir(tmp.path().to_path_buf());
        cs.current_code = Some("code".into());
        cs.approve().unwrap();
        assert_eq!(cs.status, ComponentStatus::Approved);
        assert!(tmp.path().join("test.py").exists());
    }

    #[test]
    fn test_undo() {
        let tmp = TempDir::new().unwrap();
        let mut cs = ComponentState::new("test", "Test");
        cs.set_dir(tmp.path().to_path_buf());
        cs.record_iteration("code_v1").unwrap();
        cs.record_iteration("code_v2").unwrap();
        assert_eq!(cs.iteration, 2);
        cs.undo();
        assert_eq!(cs.iteration, 1);
        assert_eq!(cs.current_code.as_deref(), Some("code_v1"));
    }

    #[test]
    fn test_two_strikes() {
        let mut cs = ComponentState::new("test", "Test");
        assert!(!cs.two_strikes());
        cs.record_error();
        assert!(!cs.two_strikes());
        cs.record_error();
        assert!(cs.two_strikes());
        assert_eq!(cs.status, ComponentStatus::Error);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib component::tests -q`
Expected: FAIL

- [ ] **Step 3: Implement ComponentState**

Write `src/component.rs` with `ComponentStatus` enum (Pending, Building, Reviewing, Approved, Error) and `ComponentState` struct with `new()`, `set_dir()`, `record_iteration()` (writes to `history/iter_NNN.py`), `approve()` (copies to `<id>.py`), `undo()` (pops from history), `record_error()`, `two_strikes()`.

- [ ] **Step 4: Add module declaration to main.rs**

Add `mod component;` to `src/main.rs` module declarations.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib component::tests -q`
Expected: PASS (4 tests)

- [ ] **Step 6: Commit**

```bash
git add src/component.rs src/main.rs
git commit -m "feat: add ComponentState with history, undo, and two-strikes tracking"
```

---

### Task 4: Create the prompt builder and system prompts

**Important:** This task generalizes the prompt discovery mechanism. The existing `find_system_prompt()` in `claude.rs` only looks for `system.md`. The new `load_phase_system_prompt()` in `prompt_builder.rs` must use the same directory-walking logic but accept any filename. Task 24 (renaming system.md to legacy.md) and Task 18 (Spec phase using `send_with_prompt()`) both depend on this generalized mechanism being in place first.

**Files:**
- Create: `src/prompt_builder.rs`
- Create: `prompts/spec.md`
- Create: `prompts/decompose.md`
- Create: `prompts/component.md`
- Create: `prompts/assembly.md`
- Create: `prompts/refinement.md`
- Modify: `src/main.rs:1` (add `mod prompt_builder;`)

- [ ] **Step 1: Write the five system prompt files**

Write each prompt file per the spec document's "System Prompt Design" section. Key points:
- `spec.md`: Ask ONE question at a time, output key-value pairs, emit SPEC_COMPLETE when done
- `decompose.md`: Output TOML only, no prose. Each component independently buildable.
- `component.md`: UPPERCASE params at top, assign to `result`, single cadquery block only
- `assembly.md`: Load components, apply transforms, single cadquery block only
- `refinement.md`: Modify existing code, don't rewrite, preserve parameters

- [ ] **Step 2: Write tests for prompt_builder**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_spec_prompt() {
        let prompt = build_spec_prompt("What is this model for?", "A watch case");
        assert!(prompt.contains("watch case"));
    }

    #[test]
    fn test_build_component_prompt() {
        let params = vec![("DIAMETER".to_string(), "25.6".to_string(), "mm".to_string())];
        let constraints = vec!["Must fit SW280 movement".to_string()];
        let prompt = build_component_prompt("movement_cavity", &params, &constraints, None);
        assert!(prompt.contains("movement_cavity"));
        assert!(prompt.contains("DIAMETER"));
        assert!(prompt.contains("25.6"));
    }

    #[test]
    fn test_build_refinement_prompt() {
        let prompt = build_refinement_prompt(
            "current code here",
            "Make it 2mm wider",
            &[("WIDTH".to_string(), "10.0".to_string(), "mm".to_string())],
        );
        assert!(prompt.contains("current code here"));
        assert!(prompt.contains("2mm wider"));
    }

    #[test]
    fn test_load_system_prompt() {
        let prompt = load_phase_system_prompt("spec");
        assert!(prompt.is_ok());
        assert!(prompt.unwrap().contains("ONE question at a time"));
    }
}
```

- [ ] **Step 3: Implement prompt_builder**

Write `src/prompt_builder.rs` with:
- `load_phase_system_prompt(phase_name)` — find and read `prompts/<phase>.md`
- `build_spec_prompt(question, answer)` — format Q&A for spec phase
- `build_decompose_prompt(spec_toml)` — format spec for decompose phase
- `build_component_prompt(id, params, constraints, dep_code)` — format component context
- `build_refinement_prompt(code, feedback, params)` — format refinement context
- `build_assembly_prompt(components, notes)` — format assembly context

- [ ] **Step 4: Add module declaration to main.rs**

Add `mod prompt_builder;` to `src/main.rs` module declarations.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib prompt_builder::tests -q`
Expected: PASS (4 tests)

- [ ] **Step 6: Commit**

```bash
git add src/prompt_builder.rs prompts/spec.md prompts/decompose.md prompts/component.md prompts/assembly.md prompts/refinement.md src/main.rs
git commit -m "feat: add phase-specific system prompts and prompt builder"
```

---

## Chunk 2: Python Layer — STEP Export, Assemble, Paramset

### Task 5: Add STEP export to builder.py

**Files:**
- Modify: `python/src/ai3d_cad/builder.py:55-112` (`_build_cadquery` function)
- Modify: `python/src/ai3d_cad/__main__.py:8-45` (add `--step` arg)
- Test: `python/tests/test_builder.py`

- [ ] **Step 1: Write test for STEP export**

Add to `python/tests/test_builder.py`:

```python
def test_build_cadquery_with_step_export(tmp_path):
    code = '''
import cadquery as cq
result = cq.Workplane("XY").box(10, 10, 10)
# feature: test cube
'''
    code_path = tmp_path / "test.py"
    code_path.write_text(code)
    stl_path = tmp_path / "test.stl"
    step_path = tmp_path / "test.step"

    from ai3d_cad.builder import build
    exit_code = build(str(code_path), str(stl_path), "cadquery", step_path=str(step_path))

    assert exit_code == 0
    assert stl_path.exists()
    assert step_path.exists()
    assert step_path.stat().st_size > 0
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /home/mcr/Projects/AI3D && .venv-cadquery/bin/python -m pytest python/tests/test_builder.py::test_build_cadquery_with_step_export -v`
Expected: FAIL — `build()` doesn't accept `step_path`

- [ ] **Step 3: Implement STEP export in builder.py**

Modify `python/src/ai3d_cad/builder.py`:

1. Add `step_path=None` parameter to `build()` function signature
2. Pass `step_path` through to `_build_cadquery()`
3. In `_build_cadquery()`, after STL export, add STEP export via `cq.exporters.export(result_obj, step_path, "STEP")` (non-fatal on failure)
4. Add `--step` argument to `__main__.py` argparse for the `build` subcommand

- [ ] **Step 4: Run test to verify it passes**

Run: `cd /home/mcr/Projects/AI3D && .venv-cadquery/bin/python -m pytest python/tests/test_builder.py::test_build_cadquery_with_step_export -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add python/src/ai3d_cad/builder.py python/src/ai3d_cad/__main__.py python/tests/test_builder.py
git commit -m "feat: add STEP export support to builder (CadQuery only)"
```

---

### Task 6: Create the assembler module

**Files:**
- Create: `python/src/ai3d_cad/assembler.py`
- Create: `python/tests/test_assembler.py`
- Modify: `python/src/ai3d_cad/__main__.py` (add `assemble` subcommand)

- [ ] **Step 1: Write assembler tests**

Write `python/tests/test_assembler.py` with 4 tests:
- `test_assemble_single_base_component` — base-only assembly outputs shape
- `test_assemble_subtract_operation` — subtract small cube from large
- `test_assemble_with_translate` — components translated before boolean ops
- `test_assemble_invalid_manifest` — bad manifest returns error exit code

Each test creates temp component .py files, a manifest JSON, and asserts on the output.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /home/mcr/Projects/AI3D && .venv-cadquery/bin/python -m pytest python/tests/test_assembler.py -v`
Expected: FAIL — module doesn't exist

- [ ] **Step 3: Implement assembler.py**

Write `python/src/ai3d_cad/assembler.py` with:
- `_load_component(path)` — run component .py, return its `result` variable
- `_apply_transform(shape, transform)` — apply translate/rotate to CadQuery shape
- `assemble(manifest_path, output_path, step_path=None)` — load manifest, run each component, apply ops in order, export STL+STEP, return metadata JSON. Returns 0/1.

- [ ] **Step 4: Add `assemble` subcommand to __main__.py**

Add argparse subcommand and routing.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd /home/mcr/Projects/AI3D && .venv-cadquery/bin/python -m pytest python/tests/test_assembler.py -v`
Expected: PASS (4 tests)

- [ ] **Step 6: Commit**

```bash
git add python/src/ai3d_cad/assembler.py python/tests/test_assembler.py python/src/ai3d_cad/__main__.py
git commit -m "feat: add assembly engine with transforms and booleans"
```

---

### Task 7: Create the paramset module

**Files:**
- Create: `python/src/ai3d_cad/paramset.py`
- Create: `python/tests/test_paramset.py`
- Modify: `python/src/ai3d_cad/__main__.py` (add `paramset` subcommand)

- [ ] **Step 1: Write paramset tests**

Write `python/tests/test_paramset.py` with 4 tests:
- `test_paramset_overrides_value` — override SIDE=10 to 20, verify STL produced
- `test_paramset_derived_params_recompute` — override WIDTH=10 to 30, verify HALF_WIDTH recomputes to 15 by checking mesh dimensions
- `test_paramset_with_step` — STEP output alongside STL
- `test_paramset_syntax_error` — bad code returns exit code 2

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /home/mcr/Projects/AI3D && .venv-cadquery/bin/python -m pytest python/tests/test_paramset.py -v`
Expected: FAIL — module doesn't exist

- [ ] **Step 3: Implement paramset.py**

Write `python/src/ai3d_cad/paramset.py`:
- `paramset(code_path, params_path, output_path, step_path=None)` → int
- Read code and override JSON
- Syntax check with `compile()`
- Create namespace with `cq` import, inject overrides into namespace
- Run compiled code in namespace (overrides take precedence, derived params recompute)
- Export STL+STEP, analyze mesh, output metadata JSON
- Returns 0 (success), 1 (build error), 2 (syntax error)

- [ ] **Step 4: Add `paramset` subcommand to __main__.py**

Add argparse subcommand and routing.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd /home/mcr/Projects/AI3D && .venv-cadquery/bin/python -m pytest python/tests/test_paramset.py -v`
Expected: PASS (4 tests)

- [ ] **Step 6: Commit**

```bash
git add python/src/ai3d_cad/paramset.py python/tests/test_paramset.py python/src/ai3d_cad/__main__.py
git commit -m "feat: add paramset for zero-Claude parameter edits via namespace injection"
```

---

### Task 8: Add Rust-side Python subprocess wrappers for assemble and paramset

**Note:** The existing codebase has no Rust-Python integration tests (only Python-side pytest and Rust-side unit tests). Full integration testing of the Rust→Python bridge for `assemble` and `paramset` is deferred to Task 27 (end-to-end manual test). The tests here verify function signatures and shared helper extraction.

**Files:**
- Modify: `src/python.rs`

- [ ] **Step 1: Write tests for new subprocess wrappers**

Add signature tests to `src/python.rs` tests to verify the new function signatures compile.

- [ ] **Step 2: Refactor `build()` into shared helper**

Extract the spawn/timeout/JSON-parse logic from existing `build()` into a private `run_python_subprocess(python, args, timeout) -> BuildResult` helper.

- [ ] **Step 3: Implement assemble() and paramset()**

Using the shared helper:
- `assemble(python, manifest_path, output_path, step_path, timeout) -> BuildResult`
- `paramset(python, code_path, params_path, output_path, step_path, timeout) -> BuildResult`
- Add `step_path: Option<&Path>` to existing `build()` signature

- [ ] **Step 4: Keep backward-compatible build() wrapper**

Instead of modifying `model_session.rs` call sites (which Task 10 will restructure entirely), keep the old `build()` function signature and have it delegate to the new one with `step_path: None`. This avoids changes that Task 10 will immediately obsolete.

Note: STEP path construction (e.g., `components/<id>/<id>.step`) will be handled in Task 10 when `PhaseSession` takes over build orchestration.

- [ ] **Step 5: Run full test suite**

Run: `cargo test -q`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/python.rs
git commit -m "feat: add assemble/paramset Rust subprocess wrappers, STEP support in build()"
```

---

## Chunk 3: Session Layer Restructure

### Task 9: New session.json schema types

**Files:**
- Modify: `src/storage/session.rs`

- [ ] **Step 1: Write tests for new schema**

Test serialization/deserialization of `PhaseSessionData` with phase, component states, per-scope Claude session IDs. Test legacy detection with `is_legacy_session_json()`.

- [ ] **Step 2: Implement new types**

Add to `src/storage/session.rs`:
- `ClaudeSessionMap` — spec, decompose session IDs + per-component HashMap
- `PhaseSessionData` — name, created, phase, current_component, claude_sessions, conversations, component_states
- `ConversationEntry` — role + content
- `is_legacy_session_json(json_str)` — detect legacy format

- [ ] **Step 3: Run tests, commit**

```bash
git add src/storage/session.rs
git commit -m "feat: add phase-aware session schema with per-component Claude sessions"
```

---

### Task 10: Restructure model_session.rs for phases

**Scope note:** This is a significant refactoring. Renaming `Session` to `LegacySession` will break references in `main.rs`, `storage/session.rs`, and `storage/project.rs`. The approach is: (a) create `PhaseSession` alongside existing `Session`, (b) migrate `App` to use `PhaseSession` in Task 17, (c) rename old `Session` to `LegacySession` at the end of this task once nothing depends on it directly.

**Files:**
- Modify: `src/model_session.rs`

- [ ] **Step 1: Write tests for PhaseSession**

Test `init_components()` creates `components/<id>/`, `components/<id>/history/`, and `assembly/` directories. Test `update_working_stl()` copies file. Test `component_dir()` returns correct paths.

- [ ] **Step 2: Create PhaseSession alongside existing Session**

Add `PhaseSession` to `src/model_session.rs` without renaming `Session` yet:
- `base_dir`, `phase`, `spec`, `components`, `current_component_idx`, `conversations`, `claude_sessions`
- `new(base_dir)` — creates `components/` and `assembly/` directories
- `init_components(ids)` — creates `components/<id>/` and `components/<id>/history/` for each component
- `component_dir(id)` → `PathBuf`
- `assembly_dir()` → `PathBuf` — returns `base_dir/assembly/`
- `update_working_stl(src)` — atomic copy to `base_dir/working.stl`
- `update_working_step(src)` — atomic copy to `base_dir/working.step`
- `save()`, `load(path)`

- [ ] **Step 3: Rename Session to LegacySession**

After `PhaseSession` is tested and working, rename the old `Session` struct to `LegacySession`. Keep it for read-only legacy session loading. Update any remaining references.

- [ ] **Step 4: Run tests, commit**

```bash
git add src/model_session.rs
git commit -m "feat: restructure session layer around phases and per-component directories"
```

---

### Task 11: Legacy session detection in project tree

**Files:**
- Modify: `src/storage/project.rs`
- Modify: `src/tui/project_tree.rs`

- [ ] **Step 1: Add SessionInfo with is_legacy flag**

In project listing, read each session.json and detect format.

- [ ] **Step 2: Show legacy badge in project tree**

Legacy sessions show `[legacy]` badge, editing/resuming disabled.

- [ ] **Step 3: Run tests, commit**

```bash
git add src/storage/project.rs src/tui/project_tree.rs
git commit -m "feat: detect and display legacy sessions as read-only in project tree"
```

---

## Chunk 4: TUI Panels — Spec Preview, Component List, Param Editor

### Task 12: Create the spec preview panel

**Files:**
- Create: `src/tui/spec_panel.rs`
- Modify: `src/tui/mod.rs`

- [ ] **Step 1: Write failing test for SpecPanel**

Test `new()`, `set_content()`, `content()` accessor.

- [ ] **Step 2: Run test to verify failure**

- [ ] **Step 3: Implement SpecPanel**

A scrollable panel that displays TOML text with a " Spec " title. Methods: `new()`, `set_content()`, `content()`, `scroll_up/down()`, `render()`.

- [ ] **Step 4: Run tests, commit**

```bash
git add src/tui/spec_panel.rs src/tui/mod.rs
git commit -m "feat: add spec preview panel for right-side TOML display"
```

---

### Task 12b: Create the component tree panel (Decompose phase)

**Files:**
- Create: `src/tui/component_tree.rs`
- Modify: `src/tui/mod.rs`

The spec says the Decompose phase right panel shows "Component tree with deps" — a visual dependency tree, not raw TOML.

- [ ] **Step 1: Write failing test**

Test rendering of a component tree with dependencies showing arrows/indentation.

- [ ] **Step 2: Implement ComponentTreePanel**

Renders components as a tree with dependency arrows. Example:
```
  case_body (base)
  ├── movement_cavity (subtract)
  ├── rotor_pocket (subtract)
  ├── stem_bore (subtract)
  └── lug_pair (fuse)
```

Methods: `from_spec(spec)`, `render()`.

- [ ] **Step 3: Run tests, commit**

```bash
git add src/tui/component_tree.rs src/tui/mod.rs
git commit -m "feat: add component dependency tree panel for Decompose phase"
```

---

### Task 13: Create the component list panel

**Files:**
- Create: `src/tui/component_list.rs`
- Modify: `src/tui/mod.rs`

- [ ] **Step 1: Write failing tests**

Test `from_components()`, `status_badge()` for each status (○/⋯/✓/✗), `select_next()`/`select_prev()`, `selected()`.

- [ ] **Step 2: Run tests to verify failure**

- [ ] **Step 3: Implement ComponentListPanel**

List widget showing components with status badges: ○ pending, ⋯ building, ✓ approved, ✗ error. Keyboard navigation (j/k), active component highlight.

- [ ] **Step 4: Run tests, commit**

```bash
git add src/tui/component_list.rs src/tui/mod.rs
git commit -m "feat: add component list panel with status badges and navigation"
```

---

### Task 14: Create the parameter editor panel

**Files:**
- Create: `src/tui/param_editor.rs`
- Modify: `src/tui/mod.rs`

- [ ] **Step 1: Write failing tests**

Test `new()` from param list, `set_value()`, `value()`, `changed_params()` returning only modified values.

- [ ] **Step 2: Run tests to verify failure**

- [ ] **Step 3: Implement ParamEditor**

Table-like widget: name | value | unit. Value becomes editable text input in edit mode. Changed values highlighted. `changed_params()` returns overrides for paramset.

- [ ] **Step 4: Run tests, commit**

```bash
git add src/tui/param_editor.rs src/tui/mod.rs
git commit -m "feat: add parameter editor panel for zero-Claude refinement"
```

---

## Chunk 5: Phase-Aware TUI Layout + Event Loop

### Task 15: Make layout phase-aware

**Dependencies:** Requires Task 1 (Phase enum).

**Files:**
- Modify: `src/tui/layout.rs`

- [ ] **Step 1: Add `phase: Phase` to LayoutConfig**

- [ ] **Step 2: Implement phase-specific panel assignments**

- Spec: left=project_tree, right=spec_panel
- Decompose: left=project_tree, right=component_tree
- Component/Assembly/Refinement: left=component_list, right=model_panel or param_editor

- [ ] **Step 3: Run tests, commit**

```bash
git add src/tui/layout.rs
git commit -m "feat: make TUI layout phase-aware with contextual panels"
```

---

### Task 16: Add phase indicator to status bar

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Render phase indicator in legend area**

Show: `[Spec] ● ○ ○ ○ ○ | Alt+1-5: switch phase | Tab: switch pane`
In Component phase: `[Component 3/6: Crown Bore]`

- [ ] **Step 2: Commit**

```bash
git add src/main.rs
git commit -m "feat: add phase indicator to status bar with progress display"
```

---

### Task 17a: Add phase and component state to App struct

**Dependencies:** Requires Tasks 3, 10, 12, 13, 14, 15 to be complete. This is the integration point where all the foundation pieces come together.

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add new fields to App struct**

Add `phase: Phase`, `spec: Option<ModelSpec>`, `components: Vec<ComponentState>`, `current_component_idx: Option<usize>`, `spec_panel: SpecPanel`, `component_list: ComponentListPanel`, `param_editor: Option<ParamEditor>`.

- [ ] **Step 2: Update App::new() to initialize new fields**

Default to `Phase::Spec`, empty components, None for current_component.

- [ ] **Step 3: Update App::render() to dispatch to correct panels per phase**

Use the phase-aware layout from Task 15. In Spec/Decompose: render spec_panel on right. In Component/Assembly/Refinement: render component_list on left, model_panel on right.

- [ ] **Step 4: Run cargo build**

Run: `cargo build`
Expected: compiles

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: add phase/component state to App struct with phase-aware rendering"
```

---

### Task 17b: Extract phase-specific key handlers

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Create handler method stubs**

Create empty methods: `handle_spec_keys()`, `handle_decompose_keys()`, `handle_component_keys()`, `handle_assembly_keys()`, `handle_refinement_keys()`. Each returns `Option<Action>`.

- [ ] **Step 2: Refactor event loop to dispatch by phase**

In the main key event handler, dispatch to the phase-specific handler based on `self.phase`. Keep shared keybindings (Tab, Ctrl+C, etc.) in the main handler.

- [ ] **Step 3: Run cargo build + tests**

Run: `cargo build && cargo test -q`

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "refactor: extract phase-specific key handlers from monolithic event loop"
```

---

### Task 17c: Wire phase navigation and transitions

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Wire Alt+1-5 for phase navigation**

- [ ] **Step 2: Wire Ctrl+Left/Right for component navigation**

- [ ] **Step 3: Add phase transition logic with prerequisite validation**

Going backwards prompts confirmation ("Switch back to Spec? Press Alt+1 again to confirm."). Going forward validates prerequisites (e.g., cannot advance from Spec without SPEC_COMPLETE, cannot advance from Decompose without approved component tree).

- [ ] **Step 4: Write tests for prerequisite validation**

Test that `can_advance_from_spec()` returns false when spec is incomplete, true when complete. Test that `can_advance_from_decompose()` requires non-empty component list.

- [ ] **Step 5: Run cargo build + tests**

Run: `cargo build && cargo test -q`

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire Alt+1-5 phase navigation with prerequisite validation"
```

---

## Chunk 6: Phase Implementations — Spec + Decompose

### Task 18: Implement Spec phase flow

**Files:**
- Modify: `src/main.rs`
- Modify: `src/claude.rs`

- [ ] **Step 1: Add `send_with_prompt()` to ClaudeClient**

New method that sends a prompt with a specified system prompt file and optional session ID (not the default system.md). Uses `load_phase_system_prompt()` from `prompt_builder.rs` (Task 4) for the generalized prompt discovery.

- [ ] **Step 2: Add session expiry detection**

In `send_with_prompt()`, if `--resume` is used and Claude returns an error indicating the session is expired/invalid, catch it and retry without `--resume` (fresh session). Log a warning to the user via the conversation pane: "Previous session expired, starting fresh."

- [ ] **Step 3: Implement Spec phase in handle_spec_keys**

User submits text → build prompt → send to Claude with spec.md → parse key-value responses → update spec panel → check for SPEC_COMPLETE → build ModelSpec → write spec.toml → transition to Decompose.

- [ ] **Step 4: Manual test, commit**

```bash
git add src/main.rs src/claude.rs
git commit -m "feat: implement Spec phase with guided Q&A and spec.toml generation"
```

---

### Task 19: Implement Decompose phase flow

**Files:**
- Modify: `src/main.rs`
- Modify: `src/parser.rs`

- [ ] **Step 1: Add TOML extraction to parser.rs**

`parse_toml_response(response)` — extract raw TOML or content from ````toml``` blocks.

- [ ] **Step 2: Implement Decompose phase**

Load spec.toml → send to Claude with decompose.md → parse TOML response → validate (no cycles) → show component tree → user approves → merge into spec.toml → create component dirs → transition to Component.

- [ ] **Step 3: Manual test, commit**

```bash
git add src/main.rs src/parser.rs
git commit -m "feat: implement Decompose phase with component tree generation and validation"
```

---

## Chunk 7: Phase Implementations — Component + Assembly

### Task 20: Implement Component phase loop

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Implement component build cycle**

Load component from assembly order → build prompt → send to Claude with component.md → parse cadquery block → build with STEP → update viewer → wait for approve/feedback/undo.

- [ ] **Step 2: Implement approve flow**

Approve → save to component dir → trigger progressive assembly → advance to next component.

- [ ] **Step 3: Implement feedback flow**

Text feedback → refinement prompt → send with --resume → parse/build → stay on same component.

- [ ] **Step 4: Implement two-strikes error handling**

Two failures → stop, show errors, suggest alternatives.

- [ ] **Step 5: Manual test, commit**

```bash
git add src/main.rs
git commit -m "feat: implement Component phase with build/approve/feedback loop"
```

---

### Task 21: Implement progressive assembly

**Files:**
- Create: `src/assembly.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement AssemblyManifest**

`AssemblyManifest` with `ManifestEntry` (id, path, role, op, from/to, transform). `from_spec()` builds manifest from approved components. `save()` writes JSON.

- [ ] **Step 2: Wire into Component phase**

After each approval: build manifest → write manifest.json → call `python::assemble()` → update working files → update viewer.

- [ ] **Step 3: Run tests, manual test, commit**

```bash
git add src/assembly.rs src/main.rs
git commit -m "feat: implement progressive assembly with manifest generation"
```

---

### Task 21b: Implement standalone Assembly phase TUI mode

**Files:**
- Modify: `src/main.rs` (handle_assembly_keys)

The spec defines Assembly as a distinct phase with its own TUI mode (left=component list, center=assembly log/conversation, right=full model metadata). This is separate from the automatic progressive assembly triggered by component approvals.

- [ ] **Step 1: Implement handle_assembly_keys()**

Assembly phase becomes active after all components are approved. The TUI shows:
- Left: component list (all approved)
- Center: assembly log showing build results, or conversation for structural changes
- Right: full model metadata (dimensions, volume, watertight)

User can:
- Flag a fit issue (routes back to Component phase for that specific part)
- Request structural changes to how parts connect (sends to Claude with assembly.md prompt)
- Edit the assembly manifest directly (modify transforms)
- Approve assembly to proceed to Refinement

- [ ] **Step 2: Implement "route back to Component" flow**

When user selects a component and presses a "fix" key:
- Switch to Component phase
- Set current_component to the flagged part
- User can modify the component, approve, assembly re-triggers

- [ ] **Step 3: Manual test, commit**

```bash
git add src/main.rs
git commit -m "feat: implement standalone Assembly phase with fit-issue routing"
```

---

## Chunk 8: Refinement Phase + Viewer Updates

### Task 22: Implement Refinement phase

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Implement parameter edit sub-mode**

ParamEditor change → validate constraints → write params JSON → call `python::paramset()` → rebuild assembly → update viewer. Zero Claude calls.

- [ ] **Step 2: Implement text feedback sub-mode**

Text input → determine target component → build refinement prompt → send to Claude → parse/build → rebuild assembly.

- [ ] **Step 3: Manual test, commit**

```bash
git add src/main.rs
git commit -m "feat: implement Refinement phase with parameter editing and scoped feedback"
```

---

### Task 23: Update viewer for working file management

**Files:**
- Modify: `src/viewer.rs`

- [ ] **Step 1: Update to real file copy + session dir**

`update_working_stl()` writes to session dir (not temp). Add `update_working_step()`. Add `set_session_dir()`.

- [ ] **Step 2: Commit**

```bash
git add src/viewer.rs
git commit -m "feat: update viewer for session-dir working files"
```

---

### Task 24: Rename legacy system prompt

**Files:**
- Rename: `prompts/system.md` → `prompts/legacy.md`
- Modify: `src/claude.rs`

- [ ] **Step 1: Rename file and update find_system_prompt()**

Rename `prompts/system.md` to `prompts/legacy.md`. Update `find_system_prompt()` in `claude.rs` to look for `legacy.md` instead of `system.md`. This function is now only used for legacy session support — phase-specific prompts use `load_phase_system_prompt()` from `prompt_builder.rs`.

- [ ] **Step 2: Update test_system_prompt_found()**

The test at `claude.rs` line ~293 asserts `find_system_prompt()` succeeds. Update it to look for `legacy.md` instead of `system.md`.

- [ ] **Step 3: Run tests**

Run: `cargo test -q`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add prompts/ src/claude.rs
git commit -m "refactor: rename system.md to legacy.md, phase prompts are now primary"
```

---

## Chunk 9: Integration + Session Persistence

### Task 25: Wire save/load for phase sessions

**Files:**
- Modify: `src/model_session.rs`, `src/storage/session.rs`, `src/main.rs`

- [ ] **Step 1: Implement PhaseSession save()**

Write session.json with phase, component states, conversations, Claude session IDs. Write spec.toml alongside.

- [ ] **Step 2: Implement PhaseSession load()**

Read session.json, detect new vs legacy, restore exact state.

- [ ] **Step 3: Wire auto-save after every state change**

- [ ] **Step 4: Wire session resume on startup**

- [ ] **Step 5: Manual test save/load roundtrip, commit**

```bash
git add src/model_session.rs src/storage/session.rs src/main.rs
git commit -m "feat: wire phase session save/load with exact state restoration"
```

---

### Task 26: Session recovery on crash

**Files:**
- Modify: `src/model_session.rs`

- [ ] **Step 1: Detect interrupted builds on load**

Check for .py in history without matching .stl → prompt rebuild or undo.

- [ ] **Step 2: Commit**

```bash
git add src/model_session.rs
git commit -m "feat: detect and recover from interrupted builds on session resume"
```

---

## Chunk 10: End-to-End Testing + Polish

### Task 27: Full end-to-end manual test

- [ ] **Step 1: Verify existing Python subcommands still work**

Run: `cd /home/mcr/Projects/AI3D && .venv-cadquery/bin/python -m pytest python/tests/ -v`
Expected: ALL tests pass (including pre-existing test_builder, test_analyzer, test_validate, test_openscad).

This ensures the refactoring of `builder.py` and `__main__.py` didn't break existing functionality.

- [ ] **Step 2: Test complete TUI flow**

Spec → Decompose → Component (all) → Assembly → Refinement (param + text) → Export → Quit → Resume.

- [ ] **Step 3: Fix issues, commit**

```bash
git add -A
git commit -m "fix: address issues found in end-to-end testing"
```

---

### Task 28: Update protocol version

**Files:**
- Modify: `python/src/ai3d_cad/__init__.py`
- Modify: `src/python.rs`

- [ ] **Step 1: Bump to protocol 2, update check**

- [ ] **Step 2: Commit**

```bash
git add python/src/ai3d_cad/__init__.py src/python.rs
git commit -m "chore: bump protocol version to 2 for phase machine API"
```

---

### Task 29: Version bump to 0.3.0

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Bump version**

- [ ] **Step 2: Commit**

```bash
git add Cargo.toml
git commit -m "chore: bump version to 0.3.0 — Phase Machine redesign"
```
