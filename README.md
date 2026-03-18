# MiModel

An interactive terminal UI for generating functional 3D models from natural language. Describe what you need, Claude orchestrates a multi-phase pipeline through MCP tools, and you get a printable STL with live 3D preview.

Built for resin 3D printing workflows where you want to go from idea to STL without opening a CAD program.

```
┌─ Projects ────────┬─ Conversation ──────────────────────┬─ Spec/Refs/Model ─┐
│                   │                                     │                   │
│ ▼ NEMA23 mount    │  you:                               │ # Design Goal     │
│   • session_1 ◀   │  a clamp mount for a NEMA 23        │                   │
│     ├ components/ │  stepper motor                      │ ## Components     │
│     │ └ body/    │                                     │ - NEMA 23 stepper │
│     │   » code.py│  claude:                            │ - M5 clamp bolts  │
│     ├ assembly/  │  The mount is ready for review.     │                   │
│     ├ goal.md    │  Key dimensions:                    │ ## Functional     │
│     ◇ _buffer.stp│  • 70 x 90 x 73mm                  │ - [ ] pocket 57.75│
│     ◆ _buffer.stl│  • 57.75mm motor pocket             │ - [ ] wall min 4mm│
│                   │                                     │                   │
│ ▶ Drone Parts     │  > Review model in viewer.          │ ## Visual         │
│                   │                                     │ - [ ] clamping slt│
│ + New Project     │                                     │ - [ ] chamfers    │
├───────────────────┴─────────────────────────────────────┴───────────────────┤
│ Input  Describe what you want to build...                                  │
├────────────────────────────────────────────────────────────────────────────┤
│ Spec ● ● ○ ○ ○  Enter Send  PgUp/Dn Scroll  Tab Panes  ^C Quit  5h 12%  │
└────────────────────────────────────────────────────────────────────────────┘
```

## How It Works

MiModel uses a **5-phase pipeline**, each with dedicated MCP tools that constrain Claude to the right task:

1. **Spec** — Describe your part. Claude asks clarifying questions and records dimensions, constraints, and features. A `goal.md` verification checklist is generated automatically.
2. **Decompose** — Claude proposes a component tree (base shape + boolean cuts/unions). You approve or adjust.
3. **Component** — Claude writes CadQuery code to `code.py`, the system auto-builds, Claude verifies against `goal.md` with a 360° model scan, and iterates up to 5 times before asking for approval.
4. **Assembly** — Claude combines approved components with boolean ops and transforms.
5. **Refinement** — Adjust parameters, add features, or modify geometry with guided feedback.

Each phase exposes only its own MCP tools — Claude cannot skip ahead or use tools from another phase.

## Goal-Driven Verification

Every build is checked against `goal.md`, a structured checklist auto-generated from the spec:

```markdown
# Design Goal

## Components to Accommodate
- NEMA 23 stepper: 57.3mm face, 47.14mm bolt pattern
- M5 clamp bolts: 2x through-holes

## Functional Requirements (verify FIRST)
- [ ] motor_pocket: 57.75 mm
- [ ] overall_size: 70x90x73 mm
- [ ] min_wall: 4 mm

## Visual & Feature Requirements (verify SECOND)
- [ ] clamping_slot: 3mm slot splitting top wall
- [ ] chamfers: 1mm on edges
```

After every build, Claude:
1. Checks build results (dimensions, topology, hole diameters) against functional requirements
2. Runs a **360° model scan** (6 headless f3d renders at 60° increments) to visually verify
3. Only requests approval when all requirements pass

## Safety Rails

- **Never fabricates specs** — Claude will not invent dimensions for real-world components. If a motor, PCB, or connector isn't in the reference library, Claude asks the user to provide specs or use `/ref` to research it.
- **Phase-gated tools** — Each phase exposes only relevant tools; Claude can't skip steps
- **No auto-advance** — Phase transitions require explicit user command (`advance` / `approve`)
- **Topology regression detection** — Refinement checks that features aren't lost between builds

## Features

- **Goal-driven verification** — `goal.md` checklist drives all build validation, functional before visual
- **360° model scanning** — 6 headless f3d renders for self-verification (no window capture needed)
- **Auto-building `write_file`** — Writing `.py` to a build directory auto-triggers CadQuery build
- **Enriched build results** — Dimensions + topology + cylindrical feature detection after every build
- **STEP import** — `/import file.step` analyzes geometry and generates starter code for parametric reconstruction
- **Live 3D preview** — f3d opens once with `--watch`, auto-reloads on every build
- **File introspection** — Claude reads its own generated code, lists session files, reviews prior iterations
- **Tab auto-complete** — `/ref` completes from reference library, `/import` completes filesystem paths
- **Three-column TUI** — Project tree, conversation, and tabbed info panel (spec/refs/model)
- **Component references** — `/ref nema23` researches real-world specs from datasheets
- **Image input** — Paste from clipboard (`Ctrl+V`), drag-drop, or reference file paths
- **Session persistence** — Spec, refs, model info, and conversation restored on session reload
- **Usage monitoring** — API usage stats (5h/7d limits) shown in the status bar

## Requirements

- **Rust** (1.70+)
- **Python 3.11** with CadQuery (see [Python Setup](#python-setup))
- **Claude CLI** (`claude`) — [Install Claude Code](https://docs.anthropic.com/en/docs/claude-code)
- **f3d** (required) — `pacman -S f3d` / `brew install f3d` — used for live preview and headless 360° scans
- **wl-clipboard** (optional, for image paste on Wayland) — `pacman -S wl-clipboard`

## Installation

### 1. Clone and build

```bash
git clone <repo-url> MiModel
cd MiModel
cargo build --release
```

The binary is at `target/release/mimodel`. Copy it somewhere in your `$PATH`:

```bash
cp target/release/mimodel ~/.local/bin/
```

### 2. Python setup

CadQuery requires Python 3.11 (the OCP bindings don't have wheels for 3.14 yet). Set up a dedicated venv:

```bash
# Using mise/asdf to get Python 3.11
mise install python@3.11
mise shell python@3.11

# Create the venv
python3.11 -m venv .venv-cadquery
source .venv-cadquery/bin/activate

# Install CadQuery + dependencies
pip install cadquery trimesh numpy

# Install the ai3d-cad package
cd python && pip install -e . && cd ..

# Verify
python -m ai3d_cad --version
# ai3d-cad 0.1.0 (protocol 1)
```

MiModel auto-detects the `.venv-cadquery` directory. You can also set the Python path explicitly:

```bash
export MIMODEL_PYTHON=/path/to/python3.11
```

### 3. Claude CLI

Make sure `claude` is installed and authenticated:

```bash
claude --version
```

MiModel spawns `claude` with `--dangerously-skip-permissions` and `--strict-mcp-config` for non-interactive use.

## Usage

```bash
mimodel
```

Create or select a project, then start describing your part:

```
> a NEMA 23 clamp mount for a CNC router
```

Claude walks through the phases — spec, decompose, component builds, assembly.

### Commands

| Command | Action |
|---------|--------|
| `advance` | Move to the next phase (after requirements met) |
| `approve` | Approve the current component or tree |
| `undo` | Revert to the previous iteration |
| `/ref nema23` + Tab | Research component specs (auto-completes from library) |
| `/import ~/file.step` + Tab | Import existing STEP file (auto-completes filesystem) |
| `/attach path` | Attach image/PDF for reference |

### Multi-line input

End a line with `\` to continue on the next line:

```
> a complex bracket with \
  4 mounting holes on the base \
  and a vertical wall with cable routing slots
```

### Image input

Paste from clipboard with `Ctrl+V`, or reference a file path:

```
> design a mount based on ~/photos/sketch.png
```

### STEP import

Import existing models to modify or rebuild parametrically:

```
> /import ~/Downloads/motor_mount.step
```

The system analyzes geometry (bounding box, faces, holes), generates a starter `code.py`, and loads the model into the viewer.

## Keybindings

### Input (default focus)

| Key | Action |
|-----|--------|
| `Enter` | Send prompt |
| `\` + `Enter` | Continue on next line |
| `Tab` | Auto-complete (`/ref`, `/import`) or switch panes |
| `Esc` | Return focus to input |
| `Ctrl+W` | Save current model as a named part |
| `Ctrl+V` | Paste image from clipboard |
| `Ctrl+O` | Open/focus f3d viewer |
| `Ctrl+N` | New session |
| `Ctrl+P` | New project |
| `Ctrl+L` | Toggle project sidebar |
| `Ctrl+R` | Toggle model panel |
| `Ctrl+C` | Cancel in-flight request (double-tap to quit) |

### Projects pane

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate up/down |
| `l` / `h` | Expand / collapse |
| `Enter` | Open session / activate file |
| `e` | Rename selected item |
| `d` | Delete (prompts for confirmation) |
| `Tab` | Switch to Conversation pane |

### Conversation pane

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll up/down |
| `u` / `d` | Page scroll |
| `Tab` | Switch to Input |

### Right panel

| Key | Action |
|-----|--------|
| `h` / `l` | Switch tabs (Spec / Refs / Model) |
| `j` / `k` | Scroll content |
| `Tab` | Switch to Input |

## Session Structure

All projects live under `~/MiModel/`:

```
~/MiModel/
├── NEMA23 mount/
│   ├── project.json
│   ├── clamp_mount.stl              # Saved part (Ctrl+W)
│   ├── clamp_mount.py               # CadQuery source for saved part
│   └── Lets_build_a_NEMA23_mount/
│       ├── session.json             # Phase state, conversations, component states
│       ├── goal.md                  # Verification checklist (auto-generated from spec)
│       ├── spec.toml                # Recorded specifications
│       ├── _buffer.stl              # Current model (f3d watches this)
│       ├── _buffer.step             # STEP export
│       ├── components/
│       │   └── cradle_body/
│       │       ├── code.py          # CadQuery code (written by Claude, auto-builds)
│       │       ├── result.stl       # Built STL
│       │       └── result.step      # STEP for assembly import
│       ├── assembly/
│       │   ├── code.py
│       │   ├── result.stl
│       │   └── result.step
│       ├── refinement/
│       │   ├── code.py
│       │   ├── result.stl
│       │   └── result.step
│       ├── imported/                # From /import command
│       │   ├── imported.step
│       │   └── code.py
│       └── images/
│           └── clipboard_123.png    # Pasted reference images
└── references/                      # Shared component library
    ├── m3_socket_head_cap_screw.toml
    └── nema_23_stepper_motor.toml
```

## MCP Tools by Phase

### All build phases (Component, Assembly, Refinement)

| Tool | Description |
|------|-------------|
| `write_file` | Write files — auto-builds when writing `.py` to a build directory |
| `read_file` | Read text files from the session directory |
| `list_files` | List session directory tree |
| `open_viewer` | Open f3d on the current model |
| `screenshot_viewer` | 360° headless scan — 6 isometric renders at 60° increments |
| `import_step` | Import a STEP file, analyze geometry, generate starter code |

### Spec

| Tool | Description |
|------|-------------|
| `ask_question` | Ask the user a clarifying question |
| `record_spec_field` | Record a dimension, constraint, feature, or component ref |
| `mark_spec_complete` | Signal spec is ready — auto-generates `goal.md` |

### Decompose

| Tool | Description |
|------|-------------|
| `ask_clarification` | Clarify decomposition details |
| `propose_component_tree` | Submit component tree for review |

### Component

| Tool | Description |
|------|-------------|
| `request_approval` | Ask user to approve after self-verification passes |

### Refinement

| Tool | Description |
|------|-------------|
| `update_parameter` | Tweak a parameter value |

## Configuration

Optional config file at `~/.config/mimodel/config.toml`:

```toml
[claude]
model = "sonnet"        # or "opus", "haiku", or full model ID

[viewer]
command = "f3d"          # or "meshlab", "xdg-open"

[defaults]
output_dir = "."
max_retries = 3
build_timeout = 60       # seconds before killing a hung build
```

## Architecture

```
src/
├── main.rs              # TUI app, event loop, render, App struct
├── event_handler.rs     # Key, mouse, paste, auto-complete event dispatch
├── phase_dispatch.rs    # Phase-aware prompt routing + context building
├── claude_bridge.rs     # Background Claude CLI orchestration, MCP config generation
├── claude.rs            # Claude CLI subprocess (--resume, --mcp-config, streaming)
├── tui/
│   ├── layout.rs        # Three-column responsive layout
│   ├── input_bar.rs     # Text input with tui-textarea + auto-complete
│   ├── conversation.rs  # Scrollable markdown-styled messages
│   ├── project_tree.rs  # Collapsible project/session/file tree
│   ├── right_panel.rs   # Tabbed panel (Spec / Refs / Model)
│   ├── component_list.rs# Component status badges
│   ├── component_tree.rs# Dependency tree visualization
│   └── status_bar.rs    # Usage stats overlay
├── storage/
│   ├── project.rs       # Project CRUD (~/MiModel/)
│   └── session.rs       # PhaseSessionData serialization
├── phase.rs             # Phase enum (Spec→Decompose→Component→Assembly→Refinement)
├── model_session.rs     # PhaseSession runtime state
├── session_manager.rs   # Active session tracking, build dispatch
├── viewer.rs            # f3d launcher with --watch, atomic file updates
├── usage.rs             # Claude API usage monitoring (OAuth, 5min cache)
├── render.rs            # Phase indicator, legend bar
├── parser.rs            # Code block extraction from responses
├── spec.rs              # ModelSpec TOML serialization
├── reference.rs         # Component reference library (~/MiModel/references/)
├── reference_detect.rs  # Auto-detect /ref markers in conversation
├── image.rs             # Clipboard paste, file path detection
├── python.rs            # ai3d-cad subprocess runner
├── preview.rs           # Braille 3D wireframe
├── stl.rs               # Binary STL reader
└── config.rs            # TOML config loading

mcp/
└── server.py            # MCP server — phase-gated tools, auto-build on write,
                         #   STEP import + analysis, headless 360° model scanning,
                         #   goal.md generation, categorized error hints

prompts/
├── spec.md              # Spec phase — question flow, field recording, no-fabrication rule
├── decompose.md         # Decompose phase — component tree proposal
├── component.md         # Component phase — goal-driven build+scan+verify loop
├── assembly.md          # Assembly phase — read components, position, verify
└── refinement.md        # Refinement phase — read-before-modify, regression detection
```

## How the Pipeline Works

```
User prompt
    │
    ▼
Rust TUI generates MCP config (phase + session dir)
    │
    ▼
Claude CLI spawned with --mcp-config --strict-mcp-config
    │
    ▼
Claude sees ONLY tools for current phase + goal.md context
    │
    ▼
Tool calls flow through MCP server (mcp/server.py)
    │
    ├─ write_file (code.py) ──► auto-detect build dir ──► CadQuery subprocess
    │                               │
    │                               ▼
    │                       result.stl + _buffer.stl updated
    │                       dimensions + topology + holes returned
    │                               │
    │                               ▼
    │                       f3d auto-reloads via --watch
    │
    ├─ screenshot_viewer ──► 6x headless f3d renders (60° increments)
    │                               │
    │                               ▼
    │                       6 base64 PNGs returned to Claude for inspection
    │
    ├─ import_step ──► copy + analyze geometry ──► generate wrapper code.py
    │
    ├─ read_file / list_files ──► Session directory I/O
    │
    └─ ask_clarification ──► Forwarded to user via TUI conversation
    │
    ▼
Claude verifies build against goal.md (functional → visual)
    │
    ▼
If check fails: fix and rebuild (up to 5 attempts)
If check passes: request_approval or describe result
    │
    ▼
Phase advances on user command ("advance" / "approve")
```

## Responsive Layout

The TUI adapts to terminal width:

- **100+ columns** — Full three-column layout
- **60-99 columns** — Sidebar hidden, conversation + right panel
- **40-59 columns** — Only conversation + input
- **Under 40** — "Terminal too narrow" message

Toggle panels manually with `Ctrl+L` (sidebar) and `Ctrl+R` (right panel).

## Running Tests

```bash
# Rust tests
cargo test

# Python tests (requires CadQuery venv)
source .venv-cadquery/bin/activate
cd python && pytest tests/ -v
```

## License

MIT
