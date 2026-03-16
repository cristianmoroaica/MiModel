# MiModel Phase Machine Redesign

**Date:** 2026-03-16
**Status:** Draft
**Scope:** Complete redesign of MiModel's generation paradigm from monolithic single-shot to phased, component-by-component workflow.

## Problem Statement

The current MiModel flow is: user prompt → Claude generates entire model → build → review → refine (full regeneration). This has compounding problems:

- **Token-heavy**: each iteration sends the full conversation history and gets a complete code response. By iteration 3-4, input alone is 15,000+ tokens.
- **Fragile**: one bad generation can break everything. Refinements often lose earlier work because Claude rewrites the entire model.
- **Slow**: full code generation + build per cycle, even for a single dimension tweak.
- **No structured intent**: Claude guesses what the user wants from freeform prompts. No formal spec, no decomposition, no named parameters to reference.

## Solution: Phase Machine

Replace the monolithic flow with five distinct phases, each with its own TUI mode, system prompt, and output contract. The user and Claude first co-create an exhaustive specification, then Claude builds the model one component at a time with continuous visual feedback.

### Design Priorities

1. **Reduce token usage** — each phase sends only what it needs, not the whole history
2. **Faster iteration** — parameter edits bypass Claude entirely; component builds are small/fast
3. **Reliability** — small scoped generations hallucinate less; each component is independently testable
4. **Better UX** — guided spec flow, progressive assembly, parameter editing
5. **Reusability** — component library grows over time, enables cross-session reuse

## Phase Architecture

```
┌─────────┐    ┌───────────────┐    ┌───────────────┐    ┌──────────┐    ┌────────────┐
│  SPEC   │───>│  DECOMPOSE    │───>│  COMPONENT    │───>│ ASSEMBLY │───>│ REFINEMENT │
│  Phase  │    │  Phase        │    │  Phase (loop) │    │  Phase   │    │  Phase     │
└─────────┘    └───────────────┘    └───────────────┘    └──────────┘    └────────────┘
```

Phases are not strictly linear — the user can jump back. Editing the spec re-enters Decompose. Flagging a component issue in Assembly routes to Component phase for that part. Current phase is persisted in `session.json` so sessions resume in the right place.

## Phase 1: Spec

**Purpose:** Co-create an exhaustive, structured specification for the model before any code is generated.

**Input:** User's idea (text + optional reference images/PDFs).

**Flow:** Claude asks guided questions one at a time, following a fixed order:
1. Purpose and context
2. Overall dimensions and envelope
3. Key features and their measurements
4. Mechanical constraints (tolerances, wall thickness, fitment)
5. Surface finish / aesthetic requirements

**Output:** `spec.toml` — a structured document the user approves before proceeding.

**TUI mode:** Q&A conversation. No model panel, no preview. Right panel shows the evolving spec in real-time.

**Claude prompt:** Spec-only system prompt. No CadQuery knowledge needed — just dimensional/functional reasoning. Outputs structured key-value pairs, not prose. Emits `SPEC_COMPLETE` when done.

**Token budget:** ~6 small calls, ~500-800 tokens input each. Spec is synthesized locally from structured answers.

### Spec Format (TOML)

```toml
[model]
name = "SW280 Watch Case"
purpose = "Resin-printed watch case for Sellita SW280 movement"
units = "mm"
print_method = "resin"

[model.envelope]
max_x = 42.0
max_y = 42.0
max_z = 14.0

[[components]]
id = "case_body"
name = "Case Body"
description = "Main cylindrical case with curved profile"
depends_on = []
assembly_op = "none"
assembly_target = ""

[components.parameters]
outer_diameter = { value = 40.0, unit = "mm", description = "Case outer diameter" }
height = { value = 11.5, unit = "mm", description = "Case height excluding lugs" }
wall_thickness = { value = 1.8, unit = "mm", description = "Minimum wall thickness" }

[components.constraints]
items = [
  "Must accommodate movement cavity (25.6mm diameter)",
  "Wall thickness >= 1.5mm for resin printing"
]

[assembly]
order = ["case_body", "movement_cavity", "crown_bore", "lug_pair"]
notes = "Movement cavity is a boolean subtraction from case_body."
```

Key properties:
- **Parameters are typed with units and descriptions** — Claude and user both reference them by name.
- **`depends_on`** defines build order.
- **Constraints** are natural language guardrails for Claude during generation.
- **Assembly order** is explicit.
- **Human-editable** — user can open the TOML and tweak values directly.

## Phase 2: Decompose

**Purpose:** Break the spec into independently buildable CadQuery components with explicit dependencies and assembly operations.

**Input:** Approved `spec.toml`.

**Flow:** Claude receives only the spec. Proposes a component tree with dependencies and assembly order. User approves, edits, or asks Claude to revise.

**Output:** `spec.toml` updated with `[[components]]` and `[assembly]` sections.

**TUI mode:** Right panel shows proposed component tree with dependency arrows.

**Claude prompt:** Decomposition-only system prompt. Outputs TOML, no prose. Rules: each component must be independently buildable, under 50 lines of CadQuery, no circular dependencies.

**Token budget:** Single Claude call, ~300-600 tokens input.

## Phase 3: Component (Loop)

**Purpose:** Generate, build, and approve components one at a time.

**Input:** One component definition from `spec.toml` + code of approved dependency components (for reference).

**Flow per component:**
1. Claude generates CadQuery code (~20-50 lines)
2. Python subprocess builds STL + STEP
3. f3d shows the component
4. Assembly auto-updates (progressive)
5. User approves or gives feedback
6. On approve: component saved to library, move to next per assembly order
7. On feedback: Claude regenerates with scoped context (current code + feedback only)

**Output:** `<component_id>.py`, `<component_id>.stl`, `<component_id>.step`, `<component_id>.json`

**TUI mode:** Left panel shows component list with status (pending/building/approved). Right panel shows component parameters + metadata + braille preview. Conversation scoped to current component only.

**Claude prompt:** Component-only system prompt. Receives: component name, parameters, constraints, dependency code. Outputs a single fenced `cadquery` block, nothing else. Rules: all tunable parameters as UPPERCASE constants at the top, assign final shape to `result`, use `# feature:` comments, keep under 50 lines.

**Token budget:** ~500-1500 tokens input per call. ~1-2 calls per component.

### Code Convention (enforced by system prompt)

```python
# Parameters (auto-generated from spec)
OUTER_DIAMETER = 40.0  # mm
HEIGHT = 11.5  # mm
WALL_THICKNESS = 1.8  # mm

# Component code
import cadquery as cq
result = (
    cq.Workplane("XY")
    .circle(OUTER_DIAMETER / 2)
    .extrude(HEIGHT)
    .shell(-WALL_THICKNESS)
)
# feature: cylindrical case body, 40mm OD
```

## Phase 4: Assembly (Progressive)

**Purpose:** Combine approved components into a running assembly, updating after each new component is approved.

**Trigger:** Automatic after each component approval in Phase 3.

**Flow:**
1. After first component approved → f3d shows it alone
2. After each subsequent component → assembly script updated, rebuilt, f3d shows growing model
3. If user flags a fit issue → routes back to Component phase for the specific part

**Output:** `assembly.py`, `assembly.stl`, `assembly.step`, `assembly.json`

**Assembly manifest:**

```json
{
  "components": [
    { "id": "case_body", "path": "components/case_body/case_body.py", "role": "base" },
    { "id": "movement_cavity", "path": "components/movement_cavity/movement_cavity.py", "op": "subtract", "from": "case_body" },
    { "id": "lug_pair", "path": "components/lug_pair/lug_pair.py", "op": "fuse", "to": "case_body" }
  ]
}
```

**Claude prompt:** Assembly-only system prompt. Only called if user requests structural changes to how parts connect. Most assemblies are generated deterministically from the manifest without a Claude call.

**Token budget:** Small — ~400-800 tokens when Claude is needed. Often zero.

## Phase 5: Refinement

**Purpose:** Modify the completed model through three sub-modes, cheapest first.

### Sub-mode A: Parameter Edit (zero tokens)

User edits a value in `spec.toml` via TUI parameter editor or text editor. System:
1. Updates `spec.toml`
2. Re-runs affected component's `.py` with new parameter value via `paramset` command
3. Rebuilds assembly
4. f3d updates

No Claude call. Parameters are validated against spec constraints before building (e.g., `wall_thickness >= 1.5` rejects `0.5` immediately).

### Sub-mode B: Text Feedback (small tokens)

User describes a structural change scoped to one component. Claude receives: component spec + current code + feedback. Regenerates just that component. Assembly auto-rebuilds.

### Sub-mode C: Visual Annotation (future)

User references a feature by name from the spec (e.g., "make the crown bore deeper"). Claude gets the feature context and modifies accordingly.

## TUI Layout Per Phase

The existing 3-column layout adapts per phase:

| Phase | Left Panel | Center Panel | Right Panel |
|-------|-----------|-------------|-------------|
| **Spec** | Project tree | Q&A conversation + input | Live spec preview (TOML) |
| **Decompose** | Project tree | Conversation + input | Component tree with deps |
| **Component** | Component list (checkmarks) | Conversation scoped to component | Component params + metadata + preview |
| **Assembly** | Component list | Assembly log / conversation | Full model metadata + preview |
| **Refinement** | Component list | Conversation or param editor | Model metadata + diff from previous |

### Navigation

- **Phase indicator** in status bar: shows current phase and progress (e.g., "Component 3/6: Crown Bore")
- `Ctrl+1-5` to jump between phases (with confirmation if going backwards)
- `Ctrl+Left/Right` to navigate between components within Component phase
- `Tab` cycles between panels (unchanged)
- **Parameter editor mode** in Refinement: Tab into right panel, edit value, Enter to rebuild

## File Structure

```
~/MiModel/<project>/<session>/
├── spec.toml                       # Master specification
├── session.json                    # Phase state, conversation per phase
├── working.stl                     # Current state (real file, not symlink)
├── working.step                    # Current state BREP
├── components/
│   ├── case_body/
│   │   ├── case_body.py            # Approved CadQuery source
│   │   ├── case_body.stl           # Mesh export
│   │   ├── case_body.step          # Parametric BREP export
│   │   ├── case_body.json          # Metadata (dims, volume, features)
│   │   └── history/
│   │       ├── iter_001.py         # First attempt
│   │       ├── iter_001.stl
│   │       ├── iter_002.py         # After feedback
│   │       └── iter_002.stl
│   ├── movement_cavity/
│   │   └── ...
│   └── lug_pair/
│       └── ...
├── assembly/
│   ├── assembly.py                 # Assembly script
│   ├── assembly.stl                # Full model mesh
│   ├── assembly.step               # Full model BREP
│   └── assembly.json               # Full model metadata
```

Nothing is cleaned up on quit. `working.stl` and `working.step` are real files preserved across sessions. History directories keep all iterations.

## Component Library

Approved components are indexed in a project-level library:

```
~/MiModel/<project>/library.toml
```

```toml
[[components]]
id = "case_body"
name = "Case Body"
source_session = "session_001"
parameters = { outer_diameter = 40.0, height = 11.5, wall_thickness = 1.8 }
tags = ["enclosure", "cylindrical", "resin"]
path = "session_001/components/case_body/case_body.py"
```

### Reuse flow

During Decompose phase of a new session:
1. Claude proposes components
2. System checks library for similar components (by tag + parameter overlap)
3. If match found, user offered: "Reuse `case_body` from session_001 with modified parameters?"
4. If yes: copy .py, substitute parameters, rebuild. No Claude call.

## Python Execution Layer

### Subcommands

```
python -m ai3d_cad build     --code <py> --output <stl> --step <step>
python -m ai3d_cad assemble  --manifest <json> --output <stl> --step <step>
python -m ai3d_cad paramset  --code <py> --params <json> --output <stl> --step <step>
python -m ai3d_cad info      --stl <stl>
python -m ai3d_cad validate  --code <py>
```

### `build` (modified)

Adds `--step` flag to export STEP alongside STL. Otherwise unchanged.

### `assemble` (new)

Takes a manifest JSON. Executes each component .py in isolation, captures `result`, applies operations (subtract, fuse, translate) in dependency order, exports combined assembly.

### `paramset` (new)

For zero-Claude parameter edits. Reads component .py, finds UPPERCASE parameter assignments at the top, overrides with values from `--params` JSON, executes, exports. Simple string substitution — reliable because the parameter convention is enforced by the system prompt.

## Claude Integration & Token Optimization

### Per-phase system prompts

```
prompts/
├── spec.md          # Guided questionnaire, key-value output
├── decompose.md     # Component decomposition, TOML output
├── component.md     # Single component CadQuery generation
├── assembly.md      # Assembly code generation
└── refinement.md    # Scoped component modification
```

Each prompt is self-contained — no reliance on conversation history from prior phases.

### Context sent per phase

| Phase | Context | Estimated tokens |
|-------|---------|-----------------|
| Spec | System prompt + user's latest answer + Q&A summary | ~500-800 |
| Decompose | System prompt + full spec.toml | ~300-600 |
| Component | System prompt + component def + dependency code | ~500-1500 |
| Assembly | System prompt + component list + spatial relationships | ~400-800 |
| Refinement | System prompt + component spec + current code + feedback | ~500-1200 |

### Session ID strategy

- **No `--resume` across phases.** Each phase starts fresh with only what it needs.
- **Keep `--resume` within a phase** for multi-turn refinement of a single component. Reset when moving to next component.
- Conversation history managed by us in `session.json`, not by Claude's session memory.

### No-Claude fast paths

These operations never call Claude:
1. Parameter edit in spec.toml → re-run component .py → rebuild STL
2. Undo → restore previous component file → rebuild
3. Re-assembly after component update → re-run assembly.py
4. Export individual component STL/STEP

## Error Handling & Recovery

| Phase | Error | Handling |
|-------|-------|---------|
| Spec | Unusable Claude answer | Re-ask with clarification. User can edit spec.toml directly. |
| Decompose | Invalid component graph | Validate locally (cycles, missing deps) before showing user. Ask Claude to fix with specific error. |
| Component | CadQuery syntax error | Auto-retry once with error appended to prompt. Second fail → show user, ask for guidance. |
| Component | Build timeout | Kill process (60s default), show message. User can increase timeout or simplify. |
| Assembly | Boolean op fails | Show which operation failed and between which components. Route to Component phase. |
| Assembly | Components don't fit | Show bounding box intersections. Route to Component phase for offending part. |
| Refinement | Parameter out of range | Validate against spec constraints before building. Reject with message. |

### Two strikes rule

If Claude fails to generate valid code for a component twice: stop, show both errors, suggest alternatives (simplify, split further, or manual edit).

### Undo

Each component keeps full `history/` directory. Undo within Component phase restores the previous iteration for that component only. Assembly auto-rebuilds. Multiple undo levels.

### Session recovery

- `session.json` written after every state change
- On startup: resume at exact phase and component
- If build was in progress at crash: detect missing .stl for latest .py in history, prompt "rebuild or undo?"

## Token Budget Comparison

Typical session: 5-component model with 3 refinement iterations and 1 parameter tweak.

| | Old paradigm | New paradigm |
|---|---|---|
| Spec phase | 0 | ~3,000 tokens |
| Initial generation | ~6,000 (full model) | ~8,000 (across ~8 calls) |
| 3 refinement iterations | ~27,000 (growing history × 3) | ~2,000 (scoped to one component) |
| Parameter tweak | ~12,000 (full history + regen) | 0 (no Claude call) |
| **Total** | **~45,000 tokens** | **~13,000 tokens** |

~3x reduction in token usage for a typical session, with higher reliability per generation.
