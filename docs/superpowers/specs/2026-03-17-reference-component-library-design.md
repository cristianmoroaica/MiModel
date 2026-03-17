# Reference Component Library

**Date:** 2026-03-17
**Status:** Design

## Problem

When users mention existing components during Spec phase (e.g. "NEMA23 motor", "Sellita SW280-1 movement"), the system has no way to look up, store, or reuse their technical specifications. Users end up manually typing dimensions into CadQuery constants. The same component gets re-researched across sessions and projects.

## Solution

A global reference component library at `~/MiModel/references/` that:

1. Auto-detects external component mentions during Spec phase and proposes them
2. Researches specs via Claude tool-use (web search) and user-provided datasheets on explicit `/ref` confirmation
3. Stores analyzed specs as TOML files, reusable across all projects
4. Injects active references into Claude messages so Claude designs around standard components
5. Directs Claude to prefer standard fasteners, bearings, and inserts over custom solutions

## Reference File Format

Location: `~/MiModel/references/<slug>.toml`

Slug normalization: retain `[a-z0-9 -]`, lowercase, collapse whitespace, replace spaces with underscores. Examples: `"NEMA 23" -> "nema_23"`, `"NEMA23" -> "nema23"`, `"M3x8 SHCS" -> "m3x8_shcs"`, `"Sellita SW280-1" -> "sellita_sw280-1"`.

Lookup is fuzzy: `/ref nema23` checks exact slug match first, then substring match against `identity.name` in all TOML files. On multiple matches, list all matches and ask the user to be more specific. On zero matches, treat as a new component and offer to research.

### Units policy
- `[dimensions]` values are always millimeters (stated once in a `units` field)
- `[constraints]` values carry unit suffixes in key names (`_g`, `_a`, `_nm`, `_c`, `_kn`, `_rpm`)

### Example: Motor

```toml
[identity]
name = "NEMA 23 Stepper Motor"
manufacturer = "Generic / multiple"
part_number = "23HS8430"
category = "motor"
created = "2026-03-17T14:00:00Z"
updated = "2026-03-17T14:00:00Z"

[dimensions]
units = "mm"
body_width = 57.2
body_height = 57.2
body_length = 56.0
shaft_diameter = 6.35
shaft_length = 20.6
mounting_hole_spacing = 47.14
mounting_hole_diameter = 5.0
pilot_diameter = 38.1
pilot_depth = 1.6
flange_thickness = 4.8

[constraints]
weight_g = 680
rated_voltage = 3.0
rated_current_a = 3.0
holding_torque_nm = 1.26
operating_temp_min_c = -20
operating_temp_max_c = 50

[sources]
urls = ["https://example.com/nema23-datasheet.pdf"]
notes = "Dimensions from NEMA MG-1 standard. Specific model values may vary by manufacturer."
```

### Example: Fastener

```toml
[identity]
name = "M3x8 Socket Head Cap Screw"
manufacturer = "Generic / ISO 4762"
part_number = ""
category = "fastener"
created = "2026-03-17T14:00:00Z"
updated = "2026-03-17T14:00:00Z"

[dimensions]
units = "mm"
thread_diameter = 3.0
thread_pitch = 0.5
length = 8.0
head_diameter = 5.5
head_height = 3.0
hex_size = 2.5
clearance_hole = 3.4
counterbore_diameter = 6.0
counterbore_depth = 3.2

[constraints]
tensile_strength_mpa = 1220
proof_load_kn = 6.78

[sources]
urls = ["https://www.iso.org/standard/11543.html"]
notes = "ISO 4762 / DIN 912. Class 12.9 property values."
```

### Example: Bearing

```toml
[identity]
name = "608ZZ Ball Bearing"
manufacturer = "Generic"
part_number = "608ZZ"
category = "bearing"
created = "2026-03-17T14:00:00Z"
updated = "2026-03-17T14:00:00Z"

[dimensions]
units = "mm"
bore_id = 8.0
outer_od = 22.0
width = 7.0

[constraints]
dynamic_load_rating_kn = 3.45
static_load_rating_kn = 1.37
max_rpm = 30000
weight_g = 12

[sources]
urls = []
notes = "Standard 608 series deep groove ball bearing. ZZ = metal shields both sides."
```

## Detection Flow

Auto-detection runs in `handle_spec_response` after every Claude response during **Spec phase only**. Detection does not run in other phases — only the `/ref` command is available there.

1. **REF markers** — Claude is instructed to wrap new component mentions in `REF[component name]` markers. The system parses these.
2. **Known patterns** — regex matches for standards: `NEMA\s?\d+`, `M\d+x[\d.]+` (metric fasteners), bearing codes (`\d{3}[A-Z]{2}`), etc.
3. **Library match** — checks if any detected name fuzzy-matches an existing slug in `~/MiModel/references/`.

On detection:
- Already in library: `"Reference available: NEMA 23 Stepper Motor (use /ref nema23 to load into spec)"`
- New component: `"Detected external component: NEMA23 stepper. Use /ref nema23 to research and save specs."`

No automatic research. Detection is cheap pattern matching. The user confirms with `/ref` before any web search.

## /ref Command

Works in any phase. The input dispatch in `submit_prompt` checks for `/ref` prefix **after** all pending-confirmation sub-state guards (`save_part_pending`, `delete_pending`, `rename_pending`, `new_project_pending`, `new_session_pending`, `ref_confirm_pending`) but **before** phase-specific dispatch. This ensures confirmation responses (`yes`/`no`) during reference save are handled by the `ref_confirm_pending` guard, not misrouted to `/ref` dispatch.

### Subcommands

- `/ref <name>` — load existing reference or research new one
- `/ref list` — show all references in the library
- `/ref remove <name>` — remove a reference from the library
- `/ref refresh <name>` — re-research an existing reference (web search + overwrite)

### /ref <name> — existing reference:
Load `~/MiModel/references/<slug>.toml`, show summary in conversation, add slug to active references list.

### /ref <name> — new reference:
1. Set `BusyState::Thinking`, spawn background Claude call
2. Prompt Claude with web search enabled: "Research {component name}. Find official datasheet or technical drawing. Extract all mechanical dimensions and key constraints. Return structured data."
3. If user attached a PDF/image alongside the command, include it as context
4. Claude returns findings via existing `BackgroundResult::ClaudeResponse` path
5. System presents findings in conversation with: `"Save as reference? (yes/no)"`
6. User types `yes` — system parses Claude's structured output into TOML, saves to library, adds to active references. If TOML parsing fails (missing fields, wrong types), show the error and ask Claude to retry with a correction prompt.
7. User types `no` — findings stay in conversation for reference but nothing is saved

### Web search implementation
Research uses the existing Claude CLI pipeline (`send_prompt` / `send_with_phase_prompt`). Claude has tool-use capabilities including web search. The research prompt asks Claude to search for and analyze the component's technical specs. No new HTTP client or search tool is needed — this reuses the infrastructure already in place.

## Active References and Prompt Injection

### Active references state
The `App` struct holds `active_refs: Vec<String>` — a list of slugs for references loaded in the current session. This is intentionally not persisted in `session.json` — references are easy to re-add with `/ref` on resume, and keeping them ephemeral avoids stale context. Populated by:
- `/ref <name>` (explicit load)
- Auto-detection when user confirms

### Injection mechanism
The system prompt (`prompts/spec.md`) is only sent on the first message in a Claude session. For references loaded mid-conversation, injection happens in the **user message** instead. Concretely:

1. **First message** — `send_with_phase_prompt` loads the spec system prompt. If `active_refs` is non-empty, appends reference summaries to the system prompt.
2. **Subsequent messages** — `send_spec_prompt` prepends a context block to the user message:
   ```
   [Active references: NEMA 23 (57.2x57.2x56mm, 6.35mm shaft), M3 SHCS (3.0mm thread, 5.5mm head)]

   <user's actual message>
   ```
3. **Library-wide preference** — the system prompt always includes the design preference directive (prefer standard components, use threaded inserts, etc.) regardless of active references. A compact list of ALL available reference names (not full specs) is included so Claude knows what's available.

This keeps prompt size bounded: full specs only for active references, just names for the rest.

### New BackgroundResult variant
Add `ReferenceResearch { name: String, response: String }` to `BackgroundResult` enum. The `handle_bg_result` match gets a new arm that presents findings and enters a confirmation sub-state.

## Slash-Command Dispatch

The `/ref` command is the first slash command in the app. To support future commands, the input dispatch in `submit_prompt` checks for a `/` prefix before phase-specific handling:

```
if text.starts_with("/ref ") || text == "/ref" {
    handle_ref_command(text);
    return;
}
// ... existing phase dispatch
```

This is a simple prefix check, not a full command framework. Future commands (if any) follow the same pattern. No need for a generic dispatcher until there are 3+ commands.

## Code Organization

### New files

**`src/reference.rs`**
- `ReferenceComponent` struct with `Identity`, `Dimensions`, `Constraints`, `Sources` sub-structs
- `load_library()` — reads all `*.toml` from `~/MiModel/references/`
- `load_one(slug)` — loads single reference by slug (exact match, then fuzzy)
- `save(component)` — writes TOML to references directory
- `summarize_for_prompt(refs)` — builds compact text for active reference injection
- `list_names(refs)` — builds compact name-only list for library-wide injection
- `slug_from_name(name)` — normalizes name to filesystem slug
- `ensure_references_dir()` — creates `~/MiModel/references/` if it doesn't exist

**`src/reference_detect.rs`**
- `detect_references(text, known_slugs)` — scans text for REF markers and known patterns
- Returns `Vec<DetectedRef>` with name, detected source (marker vs pattern), and library status
- Small regex pattern set for common standards (NEMA, metric fasteners, bearings)

### Modified files

**`src/main.rs`**
- Add `active_refs: Vec<String>` to `App` struct
- Add `ref_confirm_pending: Option<PendingReference>` for the save confirmation sub-state, where `PendingReference` holds `{ name: String, raw_response: String }` — the component name and Claude's full research response, ready to be parsed into TOML on confirmation
- Handle `/ref` command prefix in `submit_prompt` before phase dispatch
- Call `detect_references` in `handle_spec_response`
- Handle `BackgroundResult::ReferenceResearch` in `handle_bg_result`
- Handle `yes`/`no` confirmation when `ref_confirm_pending` is set

**`src/tui/mod.rs`**
- Add `ReferenceResearch { name: String, response: String }` to `BackgroundResult`

**`src/claude.rs` / `src/prompt_builder.rs`**
- `send_with_phase_prompt` accepts optional `&str` reference context to append to system prompt
- New `build_ref_research_prompt(name, user_docs)` for the research Claude call

**`prompts/spec.md`**
- Add design preference directive (prefer standard components, threaded inserts, etc.)
- Add REF marker instruction
- Add placeholder for `{REFERENCE_CONTEXT}` that gets replaced at runtime

### Unchanged files
`spec.rs`, `storage/`, `tui/conversation.rs`, `tui/layout.rs` — references are orthogonal to spec data model, session storage, and UI layout.
