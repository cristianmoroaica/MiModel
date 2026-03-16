# MiModel

An interactive terminal UI for generating functional 3D models from natural language. Describe what you need, Claude generates CadQuery code, and you get a printable STL.

Built for resin 3D printing workflows where you want to go from idea to STL without opening a CAD program.

```
┌─ Projects ────────┬─ Conversation ──────────────────────┬─ Model ──────────┐
│                   │                                     │                  │
│ ▼ Train Station   │  you:                               │ 92.0 x 62.0 x 6 │
│   ├─ enclosure    │  a mounting bracket for RPi 4       │                  │
│   ├─ servo mount  │  with 4 screw holes                 │ Features:        │
│   └─ arduino ◀    │                                     │  4x M2.5 holes   │
│                   │  claude:                            │  4x standoffs    │
│ ▶ Drone Parts     │  I'll create a mounting bracket...  │  fillet 2mm      │
│                   │                                     │                  │
│ + New Project     │  Built successfully                  │ Preview:         │
│                   │  92.0 x 62.0 x 6.5 mm               │  ⣀⣠⣤⣤⣤⣤⣄⣀      │
│                   │  - 4x M2.5 holes                    │  ⣿⣿⣿○⣿○⣿⣿⣿     │
│                   │  - 4x standoffs                     │  ⠈⠙⠛⠛⠛⠛⠋⠁      │
│                   │                                     │                  │
│                   │  you:                               │ Iterations: 2    │
│                   │  add rounded corners, 3mm           │ Engine: cadquery │
│                   │                                     │ Watertight: yes  │
├───────────────────┴─────────────────────────────────────┴──────────────────┤
│ Input  add ventilation slots on the top face                              │
├───────────────────────────────────────────────────────────────────────────┤
│ Enter Send  \+Enter Newline  Tab Switch  Ctrl+W Save part  q Quit        │
└───────────────────────────────────────────────────────────────────────────┘
```

## How It Works

1. You describe a part in plain English
2. Claude generates [CadQuery](https://cadquery.readthedocs.io/) Python code
3. The code is executed to produce an STL file
4. [f3d](https://f3d.app/) opens automatically and live-reloads on each iteration
5. You refine by typing more instructions

The conversation is persistent. Close the app, come back tomorrow, pick up where you left off.

## Features

- **Three-column TUI** - Projects, conversation, and model info side by side
- **Natural language to STL** - Describe dimensions, features, holes, fillets in plain English
- **Live 3D preview** - f3d opens once, auto-reloads on each build via `--watch`
- **Terminal preview** - Braille character wireframe right in the TUI
- **Project organization** - Group sessions into projects at `~/MiModel/`
- **Full history** - Every iteration saved: code, STL, metadata, conversation
- **Image input** - Paste from clipboard (`Ctrl+V`) or reference file paths
- **CadQuery + OpenSCAD** - CadQuery by default, OpenSCAD when CSG is more natural
- **Session resume** - Claude remembers the conversation context across restarts
- **Save parts** - `Ctrl+W` saves the current model as a named `.stl` + `.py`

## Requirements

- **Rust** (1.70+)
- **Python 3.11** with CadQuery (see [Python Setup](#python-setup))
- **Claude CLI** (`claude`) - [Install Claude Code](https://docs.anthropic.com/en/docs/claude-code)
- **f3d** (optional, recommended) - `pacman -S f3d` / `brew install f3d`
- **wl-clipboard** (optional, for image paste on Wayland) - `pacman -S wl-clipboard`

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

Alternatively, use conda/mamba:

```bash
mamba create -n mimodel python=3.11
mamba activate mimodel
mamba install -c cadquery cadquery
pip install trimesh numpy
cd python && pip install -e . && cd ..
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

MiModel spawns `claude` with `--dangerously-skip-permissions` for non-interactive use. It strips `ANTHROPIC_API_KEY` from the environment so Claude uses its own OAuth auth.

## Usage

```bash
mimodel
```

Type what you want to build and press Enter:

```
> a 20x15mm mounting bracket with 4 M3 screw holes and rounded corners
```

Claude generates the CadQuery code, builds the STL, and f3d opens with the result. Keep refining:

```
> make it 3mm thick instead of 2
> add a 10mm standoff on each corner
> chamfer the bottom edges 1mm
```

### Multi-line input

End a line with `\` to continue on the next line:

```
> a complex bracket with \
  4 mounting holes on the base \
  and a vertical wall with cable routing slots
```

### Image input

Paste an image from the clipboard:

```
Ctrl+V
> make a holder that fits this component
```

Or reference an image file directly:

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
| `Ctrl+Z` | Undo last iteration |
| `Ctrl+N` | New session |
| `Ctrl+P` | New project |
| `Ctrl+S` | Export STL |
| `Ctrl+L` | Toggle project sidebar |
| `Ctrl+R` | Toggle model panel |
| `Ctrl+C` | Cancel in-flight request |
| `q` | Quit (when input is empty) |

### Projects pane

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate up/down |
| `Enter` | Open session / expand project |
| `e` | Rename selected item |
| `d` | Delete (prompts for confirmation) |
| `Tab` | Switch to Conversation pane |

### Conversation pane

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll up/down |
| `PgUp` / `PgDn` | Page scroll |
| `Tab` | Switch to Input |

## Project Structure

All projects are saved to `~/MiModel/`:

```
~/MiModel/
├── Train Station/
│   ├── project.json
│   ├── bracket.stl              # Saved part (Ctrl+W)
│   ├── bracket.py               # CadQuery source for saved part
│   ├── enclosure/
│   │   ├── session.json          # Conversation + metadata
│   │   ├── iter_001.py           # CadQuery code, iteration 1
│   │   ├── iter_001.stl          # Built STL
│   │   ├── iter_001.json         # Dimensions, features
│   │   ├── iter_002.py           # Iteration 2 (refinement)
│   │   ├── iter_002.stl
│   │   ├── iter_002.json
│   │   └── images/
│   │       └── clipboard_123.png # Pasted reference image
│   └── servo mount/
│       ├── session.json
│       └── ...
└── Drone Parts/
    ├── project.json
    └── ...
```

- **project.json** - Project name, creation date, description
- **session.json** - Full conversation history, iteration count, Claude session ID
- **iter_NNN.py** - CadQuery source code for each iteration
- **iter_NNN.stl** - Built STL for each iteration
- **iter_NNN.json** - Metadata: dimensions, volume, triangle count, features, watertight status

Everything is auto-saved after each successful build.

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

## System Prompt

The CAD engineer prompt is at `prompts/system.md`. You can edit it to change Claude's behavior — for example, to add material-specific constraints or default dimensions.

## Architecture

```
src/
├── main.rs              # TUI app, event loop, keybindings, background threading
├── tui/
│   ├── layout.rs        # Three-column responsive layout
│   ├── input_bar.rs     # Text input with history
│   ├── conversation.rs  # Scrollable styled messages
│   ├── project_tree.rs  # Collapsible project/session tree
│   └── model_panel.rs   # Dimensions, features, braille preview
├── storage/
│   ├── project.rs       # Project CRUD (~/MiModel/)
│   └── session.rs       # Session serialization
├── model_session.rs     # Runtime build state, undo, save/load
├── claude.rs            # Claude CLI subprocess (--resume for sessions)
├── python.rs            # ai3d-cad subprocess (CadQuery execution)
├── parser.rs            # Extract code blocks from Claude responses
├── preview.rs           # Braille character 3D wireframe
├── stl.rs               # Binary STL reader
├── image.rs             # Clipboard paste, image path detection
├── viewer.rs            # f3d launcher with --watch
└── config.rs            # TOML config loading

python/src/ai3d_cad/
├── builder.py           # CadQuery code execution + STL export
├── analyzer.py          # Mesh analysis (dimensions, watertight)
└── openscad.py          # OpenSCAD fallback engine
```

The Rust binary handles the TUI, Claude orchestration, and session management. Python handles CadQuery code execution (via subprocess). They communicate through temp files and JSON on stdout.

## How the AI Pipeline Works

```
User prompt
    │
    ▼
Claude CLI (--system-prompt prompts/system.md)
    │
    ▼
Response with ```cadquery code block
    │
    ▼
Parser extracts code
    │
    ▼
Python subprocess: ai3d-cad build --engine cadquery
    │
    ▼
CadQuery executes code → STL + metadata JSON
    │
    ▼
working.stl updated → f3d auto-reloads
    │
    ▼
Model panel shows dimensions, features, preview
    │
    ▼
Auto-save to ~/MiModel/<project>/<session>/
```

Claude maintains conversation context via `--resume <session_id>`, so each refinement builds on the previous code. If the session expires, MiModel falls back to a fresh session with the current code injected as context.

## Responsive Layout

The TUI adapts to terminal width:

- **100+ columns** - Full three-column layout
- **60-99 columns** - Sidebar hidden, conversation + model panel
- **40-59 columns** - Only conversation + input
- **Under 40** - "Terminal too narrow" message

Toggle panels manually with `Ctrl+L` (sidebar) and `Ctrl+R` (model panel).

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
