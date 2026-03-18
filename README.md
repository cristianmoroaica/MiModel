# MiModel

An interactive terminal UI for generating functional 3D models from natural language. Describe what you need, Claude orchestrates a multi-phase pipeline through MCP tools, and you get a printable STL with live 3D preview.

Built for resin 3D printing workflows where you want to go from idea to STL without opening a CAD program.

```
┌─ Projects ────────┬─ Conversation ──────────────────────┬─ Spec/Refs/Model ─┐
│                   │                                     │                   │
│ ▼ NEMA23 mount    │  you:                               │ [dimension]       │
│   • session_1 ◀   │  a clamp mount for a NEMA 23        │  motor_pocket =   │
│     ├ components/ │  stepper motor                      │  57.75mm          │
│     │ └ body/    │                                     │                   │
│     │   » code.py│  claude:                            │ [feature]         │
│     │   ◆ result │  The mount is ready for review.     │  clamping_slot    │
│     ◇ working.stp│  Key dimensions:                    │  mounting_holes   │
│     ◆ _buffer.stl│  • 70 x 90 x 73mm                  │                   │
│                   │  • 57.75mm motor pocket             │ [constraint]      │
│ ▶ Drone Parts     │                                     │  wall_min = 4mm   │
│                   │  > Opened in viewer                 │                   │
│ + New Project     │                                     │                   │
├───────────────────┴─────────────────────────────────────┴───────────────────┤
│ Input  Describe what you want to build...                                  │
├────────────────────────────────────────────────────────────────────────────┤
│ Spec ● ● ○ ○ ○  Enter Send  PgUp/Dn Scroll  Tab Panes  ^C Quit           │
└────────────────────────────────────────────────────────────────────────────┘
```

## How It Works

MiModel uses a **5-phase pipeline**, each with dedicated MCP tools that constrain Claude to the right task:

1. **Spec** — Describe your part. Claude asks clarifying questions and records dimensions, constraints, and features.
2. **Decompose** — Claude proposes a component tree (base shape + boolean cuts/unions). You approve or adjust.
3. **Component** — Claude generates CadQuery code for each component, builds the STL, screenshots the viewer to self-verify, and iterates up to 5 times before asking for approval.
4. **Assembly** — Claude combines approved components with boolean ops and transforms.
5. **Refinement** — Adjust parameters, add features, or modify geometry with guided feedback.

Each phase exposes only its own MCP tools — Claude cannot skip ahead or use tools from another phase.

## Features

- **Phase-gated MCP tools** — Each phase exposes only relevant tools; Claude can't skip steps
- **Self-verifying builds** — Claude screenshots the f3d viewer after each build to check geometry visually, iterating up to 5 times before asking for approval
- **Live 3D preview** — f3d opens once with `--watch`, auto-reloads on every build
- **File introspection** — Claude can read its own generated code, list session files, and review previous iterations
- **Three-column TUI** — Project tree, conversation, and tabbed info panel (spec/refs/model)
- **Natural language to STL** — Dimensions, holes, fillets, chamfers in plain English
- **Image input** — Paste from clipboard (`Ctrl+V`), drag-drop, or reference file paths
- **Component references** — `/ref nema23` researches real-world specs (thread pitch, clearance holes)
- **Project organization** — Sessions grouped into projects at `~/MiModel/`
- **Session resume** — Claude conversation context persists across restarts
- **Usage monitoring** — API usage stats (5h/7d limits) shown in the status bar

## Requirements

- **Rust** (1.70+)
- **Python 3.11** with CadQuery (see [Python Setup](#python-setup))
- **Claude CLI** (`claude`) — [Install Claude Code](https://docs.anthropic.com/en/docs/claude-code)
- **f3d** (recommended) — `pacman -S f3d` / `brew install f3d`
- **grim + hyprctl** (for visual self-verification on Wayland/Hyprland)
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

Claude walks through the phases automatically — spec, decompose, component builds, assembly.

### Phase commands

| Command | Action |
|---------|--------|
| `advance` | Move to the next phase (after requirements met) |
| `approve` | Approve the current component or tree |
| `undo` | Revert to the previous iteration |
| `/ref nema23` | Research component specs from datasheets |

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

## Keybindings

### Input (default focus)

| Key | Action |
|-----|--------|
| `Enter` | Send prompt |
| `\` + `Enter` | Continue on next line |
| `Tab` | Switch to Projects pane |
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
│   ├── clamp_mount.py               # Source for saved part
│   └── Lets_build_a_NEMA23_mount/
│       ├── session.json             # Phase state, conversations, component states
│       ├── spec.toml                # Recorded specifications
│       ├── _buffer.stl              # Current model (f3d watches this)
│       ├── _buffer.step             # STEP export
│       ├── components/
│       │   └── cradle_body/
│       │       ├── code.py          # Approved CadQuery code
│       │       ├── result.stl       # Built STL
│       │       └── result.step      # STEP for assembly
│       ├── assembly/
│       │   ├── code.py
│       │   ├── result.stl
│       │   └── result.step
│       ├── refinement/
│       │   ├── code.py
│       │   ├── result.stl
│       │   └── result.step
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
| `read_file` | Read text files from the session directory |
| `list_files` | List session directory tree |
| `open_viewer` | Open f3d on the current model |
| `screenshot_viewer` | Capture the f3d window for visual verification |

### Spec

| Tool | Description |
|------|-------------|
| `ask_question` | Ask the user a clarifying question |
| `record_spec_field` | Record a dimension, constraint, feature, or component ref |
| `mark_spec_complete` | Signal spec is ready for decomposition |

### Decompose

| Tool | Description |
|------|-------------|
| `ask_clarification` | Clarify decomposition details |
| `propose_component_tree` | Submit component tree for review |

### Component

| Tool | Description |
|------|-------------|
| `submit_cadquery_code` | Build a component — executes code, exports STL, updates viewer |
| `request_approval` | Ask user to approve after visual self-verification |

### Assembly

| Tool | Description |
|------|-------------|
| `submit_assembly_code` | Build assembly — combines components with boolean ops |

### Refinement

| Tool | Description |
|------|-------------|
| `update_parameter` | Tweak a parameter value |
| `submit_code_patch` | Submit modified code for rebuild |

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
├── event_handler.rs     # Key, mouse, paste event dispatch
├── phase_dispatch.rs    # Phase-aware prompt routing
├── claude_bridge.rs     # Background Claude CLI orchestration, MCP config generation
├── claude.rs            # Claude CLI subprocess (--resume, --mcp-config, streaming)
├── tui/
│   ├── layout.rs        # Three-column responsive layout
│   ├── input_bar.rs     # Text input with tui-textarea
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
├── usage.rs             # Claude API usage monitoring (OAuth)
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
└── server.py            # MCP server — phase-gated tools, CadQuery build execution,
                         #   file I/O, viewer screenshots (hyprctl + grim)

python/src/ai3d_cad/
├── builder.py           # CadQuery code execution + STL/STEP export
├── assembler.py         # Assembly operations
└── __main__.py          # CLI interface

prompts/
├── spec.md              # Spec phase system prompt
├── decompose.md         # Decompose phase system prompt
├── component.md         # Component phase — build + screenshot + verify loop
├── assembly.md          # Assembly phase system prompt
└── refinement.md        # Refinement phase system prompt
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
Claude sees ONLY tools for current phase
    │
    ▼
Tool calls flow through MCP server (mcp/server.py)
    │
    ├─ submit_cadquery_code ──► CadQuery subprocess ──► STL + STEP
    │                               │
    │                               ▼
    │                       _buffer.stl updated (atomic rename)
    │                               │
    │                               ▼
    │                       f3d auto-reloads via --watch
    │
    ├─ screenshot_viewer ──► hyprctl finds f3d window ──► grim captures PNG
    │                               │
    │                               ▼
    │                       Base64 image returned to Claude for inspection
    │
    ├─ read_file / list_files ──► Session directory I/O
    │
    └─ ask_clarification ──► Forwarded to user via TUI conversation
    │
    ▼
Rust TUI intercepts tool_use blocks from Claude's stream
    │
    ▼
Updates conversation, spec panel, component list, viewer state
    │
    ▼
Phase advances on user command ("advance" / "approve")
```

Claude maintains conversation context via `--resume <session_id>`. If the session expires, MiModel falls back to a fresh session with the phase prompt re-injected.

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
