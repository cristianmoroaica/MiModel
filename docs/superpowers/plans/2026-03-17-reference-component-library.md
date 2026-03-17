# Reference Component Library Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Auto-detect external component mentions during Spec phase, research their specs, and store them as reusable TOML reference files that get injected into Claude prompts.

**Architecture:** Two new modules (`reference.rs`, `reference_detect.rs`) handle TOML load/save and pattern detection respectively. The `/ref` command dispatches in `submit_prompt` before phase dispatch. Research uses the existing Claude CLI pipeline. Active references are injected into spec prompts via user message context blocks.

**Tech Stack:** Rust, ratatui, serde/toml, regex

**Spec:** `docs/superpowers/specs/2026-03-17-reference-component-library-design.md`

---

## Chunk 1: Reference Data Model and Storage

### Task 1: Create reference.rs with data structures and slug_from_name

**Files:**
- Create: `src/reference.rs`
- Test: `src/reference.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Write failing tests for slug_from_name**

```rust
// In src/reference.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slug_from_name() {
        assert_eq!(slug_from_name("NEMA 23"), "nema_23");
        assert_eq!(slug_from_name("NEMA23"), "nema23");
        assert_eq!(slug_from_name("M3x8 SHCS"), "m3x8_shcs");
        assert_eq!(slug_from_name("Sellita SW280-1"), "sellita_sw280-1");
        assert_eq!(slug_from_name("  Spaces   Everywhere  "), "spaces_everywhere");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib reference::tests::test_slug_from_name`
Expected: FAIL — module doesn't exist

- [ ] **Step 3: Implement data structures and slug_from_name**

```rust
//! Reference component library — load, save, and query reusable component specs.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceComponent {
    pub identity: Identity,
    pub dimensions: Dimensions,
    #[serde(default)]
    pub constraints: HashMap<String, toml::Value>,
    pub sources: Sources,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub name: String,
    #[serde(default)]
    pub manufacturer: String,
    #[serde(default)]
    pub part_number: String,
    #[serde(default)]
    pub category: String,
    pub created: String,
    #[serde(default)]
    pub updated: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dimensions {
    #[serde(default = "default_units")]
    pub units: String,
    /// All remaining keys under [dimensions] are dimension name -> value.
    /// Uses toml::Value to coexist with the `units` string field via flatten.
    #[serde(flatten)]
    pub values: HashMap<String, toml::Value>,
}

fn default_units() -> String { "mm".to_string() }

impl Dimensions {
    /// Get a dimension value as f64, ignoring non-numeric entries.
    pub fn get_f64(&self, key: &str) -> Option<f64> {
        self.values.get(key).and_then(|v| match v {
            toml::Value::Float(f) => Some(*f),
            toml::Value::Integer(i) => Some(*i as f64),
            _ => None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sources {
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

/// Normalize a component name to a filesystem-safe slug.
/// Retains [a-z0-9 -], lowercases, collapses whitespace, replaces spaces with underscores.
pub fn slug_from_name(name: &str) -> String {
    let lower = name.to_lowercase();
    let filtered: String = lower
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '-')
        .collect();
    filtered.split_whitespace().collect::<Vec<_>>().join("_")
}

/// Return the references directory path: ~/MiModel/references/
pub fn references_dir() -> PathBuf {
    crate::storage::project::root_dir().join("references")
}

/// Ensure ~/MiModel/references/ exists.
pub fn ensure_references_dir() -> Result<PathBuf, String> {
    let dir = references_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create references dir: {e}"))?;
    Ok(dir)
}
```

- [ ] **Step 4: Add `mod reference;` to main.rs**

Add `mod reference;` after the existing `mod python;` line in `src/main.rs` (around line 11).

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib reference::tests::test_slug_from_name`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/reference.rs src/main.rs
git commit -m "feat(reference): add data structures and slug_from_name"
```

### Task 2: Add load/save/library functions to reference.rs

**Files:**
- Modify: `src/reference.rs`

- [ ] **Step 1: Write failing tests for save and load_one**

```rust
#[test]
fn test_save_and_load_roundtrip() {
    let tmp = tempfile::TempDir::new().unwrap();
    let component = ReferenceComponent {
        identity: Identity {
            name: "608ZZ Ball Bearing".into(),
            manufacturer: "Generic".into(),
            part_number: "608ZZ".into(),
            category: "bearing".into(),
            created: "2026-03-17T14:00:00Z".into(),
            updated: "2026-03-17T14:00:00Z".into(),
        },
        dimensions: Dimensions {
            units: "mm".into(),
            values: [("bore_id".into(), toml::Value::Float(8.0)), ("outer_od".into(), toml::Value::Float(22.0)), ("width".into(), toml::Value::Float(7.0))]
                .into_iter().collect(),
        },
        constraints: [("weight_g".into(), toml::Value::Integer(12))]
            .into_iter().collect(),
        sources: Sources {
            urls: vec![],
            notes: "Standard 608 bearing.".into(),
        },
    };

    save_to_dir(&component, tmp.path()).unwrap();
    let slug = slug_from_name(&component.identity.name);
    let loaded = load_one_from_dir(&slug, tmp.path()).unwrap();
    assert_eq!(loaded.identity.name, "608ZZ Ball Bearing");
    assert_eq!(loaded.dimensions.get_f64("bore_id").unwrap(), 8.0);
}

#[test]
fn test_load_library() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Create two reference files
    let c1 = ReferenceComponent {
        identity: Identity {
            name: "M3 SHCS".into(), manufacturer: "".into(), part_number: "".into(),
            category: "fastener".into(), created: "".into(), updated: "".into(),
        },
        dimensions: Dimensions { units: "mm".into(), values: [("thread_diameter".into(), toml::Value::Float(3.0))].into_iter().collect() },
        constraints: HashMap::new(),
        sources: Sources { urls: vec![], notes: "".into() },
    };
    let c2 = ReferenceComponent {
        identity: Identity {
            name: "M5 SHCS".into(), manufacturer: "".into(), part_number: "".into(),
            category: "fastener".into(), created: "".into(), updated: "".into(),
        },
        dimensions: Dimensions { units: "mm".into(), values: [("thread_diameter".into(), toml::Value::Float(5.0))].into_iter().collect() },
        constraints: HashMap::new(),
        sources: Sources { urls: vec![], notes: "".into() },
    };
    save_to_dir(&c1, tmp.path()).unwrap();
    save_to_dir(&c2, tmp.path()).unwrap();

    let library = load_library_from_dir(tmp.path()).unwrap();
    assert_eq!(library.len(), 2);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib reference::tests`
Expected: FAIL — functions not defined

- [ ] **Step 3: Implement save, load_one, load_library**

```rust
/// Save a reference component to a directory as <slug>.toml.
pub fn save_to_dir(component: &ReferenceComponent, dir: &Path) -> Result<(), String> {
    let slug = slug_from_name(&component.identity.name);
    let path = dir.join(format!("{slug}.toml"));
    let toml_str = toml::to_string_pretty(component)
        .map_err(|e| format!("Failed to serialize reference: {e}"))?;
    std::fs::write(&path, toml_str)
        .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
    Ok(())
}

/// Save a reference component to the global references directory.
pub fn save(component: &ReferenceComponent) -> Result<(), String> {
    let dir = ensure_references_dir()?;
    save_to_dir(component, &dir)
}

/// Load a single reference by exact slug match from a directory.
pub fn load_one_from_dir(slug: &str, dir: &Path) -> Result<ReferenceComponent, String> {
    let path = dir.join(format!("{slug}.toml"));
    if path.exists() {
        let contents = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        return toml::from_str(&contents)
            .map_err(|e| format!("Failed to parse {}: {e}", path.display()));
    }
    Err(format!("Reference '{slug}' not found"))
}

/// Load a reference by slug — exact match first, then fuzzy substring on identity.name.
/// Returns (component, slug) on success.
pub fn load_one(query: &str) -> Result<(ReferenceComponent, String), String> {
    let dir = references_dir();
    if !dir.exists() {
        return Err(format!("Reference '{query}' not found (no references directory)"));
    }
    let slug = slug_from_name(query);

    // Exact match
    if let Ok(c) = load_one_from_dir(&slug, &dir) {
        return Ok((c, slug));
    }

    // Fuzzy: substring match on identity.name
    let query_lower = query.to_lowercase();
    let mut matches: Vec<(ReferenceComponent, String)> = Vec::new();
    if let Ok(library) = load_library_from_dir(&dir) {
        for (comp, comp_slug) in library {
            if comp.identity.name.to_lowercase().contains(&query_lower) {
                matches.push((comp, comp_slug));
            }
        }
    }

    match matches.len() {
        0 => Err(format!("Reference '{query}' not found")),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => {
            let names: Vec<String> = matches.iter()
                .map(|(c, s)| format!("  {} ({})", c.identity.name, s))
                .collect();
            Err(format!("Multiple matches for '{query}':\n{}\nBe more specific.", names.join("\n")))
        }
    }
}

/// Load all references from a directory. Returns Vec<(component, slug)>.
pub fn load_library_from_dir(dir: &Path) -> Result<Vec<(ReferenceComponent, String)>, String> {
    let mut results = Vec::new();
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("Failed to read references dir: {e}"))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            let slug = path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if let Ok(contents) = std::fs::read_to_string(&path) {
                if let Ok(comp) = toml::from_str::<ReferenceComponent>(&contents) {
                    results.push((comp, slug));
                }
            }
        }
    }
    Ok(results)
}

/// Load all references from the global references directory.
pub fn load_library() -> Result<Vec<(ReferenceComponent, String)>, String> {
    let dir = references_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    load_library_from_dir(&dir)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib reference::tests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/reference.rs
git commit -m "feat(reference): add save, load_one, load_library functions"
```

### Task 3: Add prompt summarization functions

**Files:**
- Modify: `src/reference.rs`

- [ ] **Step 1: Write failing tests for summarize_for_prompt and list_names**

```rust
#[test]
fn test_summarize_for_prompt() {
    let comp = ReferenceComponent {
        identity: Identity {
            name: "NEMA 23 Stepper Motor".into(), manufacturer: "".into(),
            part_number: "".into(), category: "motor".into(),
            created: "".into(), updated: "".into(),
        },
        dimensions: Dimensions {
            units: "mm".into(),
            values: [
                ("body_width".into(), toml::Value::Float(57.2)),
                ("body_height".into(), toml::Value::Float(57.2)),
                ("shaft_diameter".into(), toml::Value::Float(6.35)),
            ].into_iter().collect(),
        },
        constraints: HashMap::new(),
        sources: Sources { urls: vec![], notes: "".into() },
    };
    let summary = summarize_for_prompt(&[&comp]);
    assert!(summary.contains("NEMA 23 Stepper Motor"));
    assert!(summary.contains("57.2"));
    assert!(summary.contains("shaft_diameter"));
}

#[test]
fn test_list_names() {
    let c1 = ReferenceComponent {
        identity: Identity {
            name: "M3 SHCS".into(), manufacturer: "".into(), part_number: "".into(),
            category: "fastener".into(), created: "".into(), updated: "".into(),
        },
        dimensions: Dimensions { units: "mm".into(), values: HashMap::new() },
        constraints: HashMap::new(),
        sources: Sources { urls: vec![], notes: "".into() },
    };
    let names = list_names(&[&c1]);
    assert!(names.contains("M3 SHCS"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib reference::tests`
Expected: FAIL — functions not defined

- [ ] **Step 3: Implement summarize_for_prompt and list_names**

```rust
/// Build a compact summary of active references for prompt injection.
/// Format: "- NEMA 23 Stepper Motor: body_width=57.2, shaft_diameter=6.35 (mm)"
pub fn summarize_for_prompt(refs: &[&ReferenceComponent]) -> String {
    let mut lines = Vec::new();
    for r in refs {
        let mut dim_keys: Vec<&String> = r.dimensions.values.keys().collect();
        dim_keys.sort(); // Deterministic ordering
        let dims: Vec<String> = dim_keys.iter()
            .filter_map(|k| r.dimensions.get_f64(k).map(|v| format!("{k}={v}")))
            .collect();
        let dims_str = if dims.is_empty() {
            String::new()
        } else {
            format!(": {} ({})", dims.join(", "), r.dimensions.units)
        };
        lines.push(format!("- {}{}", r.identity.name, dims_str));
    }
    lines.join("\n")
}

/// Build a compact name-only list for library-wide injection.
pub fn list_names(refs: &[&ReferenceComponent]) -> String {
    refs.iter()
        .map(|r| format!("- {} [{}]", r.identity.name, r.identity.category))
        .collect::<Vec<_>>()
        .join("\n")
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib reference::tests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/reference.rs
git commit -m "feat(reference): add prompt summarization functions"
```

## Chunk 2: Detection Module

### Task 4: Create reference_detect.rs with pattern detection

**Files:**
- Create: `src/reference_detect.rs`

- [ ] **Step 1: Write failing tests for detect_references**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_ref_markers() {
        let text = "We should use a REF[NEMA 23 stepper] for the drive.";
        let results = detect_references(text, &[]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "NEMA 23 stepper");
        assert_eq!(results[0].source, DetectionSource::Marker);
    }

    #[test]
    fn test_detect_known_patterns() {
        let text = "Mount with M3x8 screws on a NEMA17 motor.";
        let results = detect_references(text, &[]);
        assert!(results.iter().any(|r| r.name.contains("M3")));
        assert!(results.iter().any(|r| r.name.contains("NEMA")));
    }

    #[test]
    fn test_detect_in_library() {
        let text = "We'll need a nema_23 motor.";
        let known = vec!["nema_23".to_string()];
        let results = detect_references(text, &known);
        assert!(results.iter().any(|r| r.in_library));
    }

    #[test]
    fn test_no_false_positives_on_plain_text() {
        let text = "The case is 38mm wide with a 10.5mm height.";
        let results = detect_references(text, &[]);
        assert!(results.is_empty());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib reference_detect::tests`
Expected: FAIL — module doesn't exist

- [ ] **Step 3: Implement detect_references**

```rust
//! Reference detection — scan text for external component mentions.

use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq)]
pub struct DetectedRef {
    pub name: String,
    pub source: DetectionSource,
    pub in_library: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DetectionSource {
    Marker,  // REF[...] from Claude
    Pattern, // regex match
}

static REF_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"REF\[([^\]]+)\]").unwrap()
});

static NEMA_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bNEMA\s?\d{1,2}\b").unwrap()
});

static METRIC_FASTENER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bM\d+x[\d.]+\b").unwrap()
});

static BEARING_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    // Match bearing codes like 608ZZ, 6201RS, 6305ZZ — NOT units like 100MHz
    Regex::new(r"\b\d{3,4}[A-Z]{2,3}\b(?![a-z])").unwrap()
});

/// Scan text for external component references.
/// `known_slugs` is the list of slugs already in ~/MiModel/references/.
pub fn detect_references(text: &str, known_slugs: &[String]) -> Vec<DetectedRef> {
    let mut results: Vec<DetectedRef> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // 1. REF[...] markers
    for cap in REF_MARKER.captures_iter(text) {
        let name = cap[1].trim().to_string();
        let slug = crate::reference::slug_from_name(&name);
        if seen.insert(slug.clone()) {
            let in_library = known_slugs.iter().any(|s| *s == slug);
            results.push(DetectedRef { name, source: DetectionSource::Marker, in_library });
        }
    }

    // 2. Known patterns
    let patterns: Vec<(&LazyLock<Regex>, &str)> = vec![
        (&NEMA_PATTERN, "motor"),
        (&METRIC_FASTENER, "fastener"),
        (&BEARING_PATTERN, "bearing"),
    ];

    for (regex, _category) in patterns {
        for mat in regex.find_iter(text) {
            let name = mat.as_str().to_string();
            let slug = crate::reference::slug_from_name(&name);
            if seen.insert(slug.clone()) {
                let in_library = known_slugs.iter().any(|s| *s == slug);
                results.push(DetectedRef { name, source: DetectionSource::Pattern, in_library });
            }
        }
    }

    results
}
```

- [ ] **Step 4: Add `mod reference_detect;` and `regex` dependency**

Add `mod reference_detect;` after `mod reference;` in `src/main.rs`.

Add `regex = "1"` to `[dependencies]` in `Cargo.toml`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib reference_detect::tests`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/reference_detect.rs src/main.rs Cargo.toml Cargo.lock
git commit -m "feat(reference): add detection module with REF markers and pattern matching"
```

## Chunk 3: /ref Command and App Integration

### Task 5: Add BackgroundResult variant and App state fields

**Files:**
- Modify: `src/tui/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add ReferenceResearch variant to BackgroundResult**

In `src/tui/mod.rs`, add to the `BackgroundResult` enum:

```rust
pub enum BackgroundResult {
    ClaudeResponse {
        result: Result<String, String>,
        session_id: Option<String>,
    },
    BuildComplete(crate::python::BuildResult),
    ReferenceResearch {
        name: String,
        result: Result<String, String>,
    },
}
```

- [ ] **Step 2: Add PendingReference struct and App fields**

In `src/main.rs`, add near the other structs (after `DeleteTarget`):

```rust
#[derive(Debug, Clone)]
struct PendingReference {
    name: String,
    raw_response: String,
}
```

Add fields to the `App` struct (after `save_part_pending`):

```rust
    active_refs: Vec<String>,
    ref_confirm_pending: Option<PendingReference>,
```

Initialize them in both `App::new()` and the test fixture:

```rust
    active_refs: Vec::new(),
    ref_confirm_pending: None,
```

Also add `self.active_refs.clear();` and `self.ref_confirm_pending = None;` to the session reset block in `submit_prompt` (inside the `new_session_pending` handler, around line 900, alongside the existing `self.conversation.clear()` etc.).

- [ ] **Step 3: Build to verify compilation**

Run: `cargo build 2>&1 | rg "^error" || echo "BUILD OK"`
Expected: BUILD OK

- [ ] **Step 4: Commit**

```bash
git add src/tui/mod.rs src/main.rs
git commit -m "feat(reference): add App state fields and BackgroundResult variant"
```

### Task 6: Implement /ref command dispatch in submit_prompt

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add ref_confirm_pending guard in submit_prompt**

In `submit_prompt`, after the `new_session_pending` block (around line 909) and before the `// Auto-create session name` section, add:

```rust
        // Handle reference save confirmation
        if let Some(pending) = self.ref_confirm_pending.take() {
            if text.trim().eq_ignore_ascii_case("yes") {
                self.save_pending_reference(pending);
            } else {
                self.conversation.add("system", "Reference not saved.");
            }
            return;
        }

        // Handle /ref commands — extract attached images first since we return early
        if text.starts_with("/ref") {
            let (_clean, mut images) = image::extract_attachment_paths(&text);
            images.extend(self.pending_images.drain(..));
            self.handle_ref_command(&text, images);
            return;
        }
```

- [ ] **Step 2: Implement handle_ref_command**

Add this method to `impl App`:

```rust
    fn handle_ref_command(&mut self, text: &str, attached_images: Vec<PathBuf>) {
        let args = text.strip_prefix("/ref").unwrap_or("").trim();

        if args.is_empty() || args == "list" {
            // List all references
            match reference::load_library() {
                Ok(library) if library.is_empty() => {
                    self.conversation.add("system", "Reference library is empty.");
                }
                Ok(library) => {
                    let list: Vec<String> = library.iter()
                        .map(|(c, s)| format!("  {} — {} [{}]", s, c.identity.name, c.identity.category))
                        .collect();
                    self.conversation.add("system", &format!("References:\n{}", list.join("\n")));
                }
                Err(e) => self.conversation.add("system", &format!("Error: {e}")),
            }
            return;
        }

        if let Some(name) = args.strip_prefix("remove ") {
            let name = name.trim();
            let slug = reference::slug_from_name(name);
            let path = reference::references_dir().join(format!("{slug}.toml"));
            if path.exists() {
                if let Err(e) = std::fs::remove_file(&path) {
                    self.conversation.add("system", &format!("Failed to remove: {e}"));
                } else {
                    self.active_refs.retain(|s| s != &slug);
                    self.conversation.add("system", &format!("Removed reference '{slug}'."));
                }
            } else {
                self.conversation.add("system", &format!("Reference '{slug}' not found."));
            }
            return;
        }

        let is_refresh = args.starts_with("refresh ");
        let query = if is_refresh {
            args.strip_prefix("refresh ").unwrap().trim()
        } else {
            args
        };

        // Try to load existing (unless refresh)
        if !is_refresh {
            match reference::load_one(query) {
                Ok((comp, slug)) => {
                    if !self.active_refs.contains(&slug) {
                        self.active_refs.push(slug.clone());
                    }
                    let summary = reference::summarize_for_prompt(&[&comp]);
                    self.conversation.add("system", &format!("Loaded reference:\n{summary}"));
                    return;
                }
                Err(e) if e.contains("Multiple matches") => {
                    self.conversation.add("system", &e);
                    return;
                }
                Err(_) => {} // Not found — research below
            }
        }

        // Research new component
        self.conversation.add("system", &format!("Researching '{query}'..."));
        self.busy = BusyState::Thinking;
        self.streaming_text.clear();

        let model = self.claude_model.clone();
        let tx = self.bg_tx.clone();
        let stream_tx = self.stream_tx.clone();
        let bg_pid = Arc::clone(&self.bg_pid);
        let name = query.to_string();
        let images = attached_images;

        std::thread::spawn(move || {
            let research_prompt = format!(
                "Research the component: {name}\n\
                 Find official datasheet or technical drawing.\n\
                 Extract ALL mechanical dimensions in millimeters and key constraints.\n\
                 Return the data as a TOML block in this exact format:\n\
                 ```toml\n\
                 [identity]\n\
                 name = \"full component name\"\n\
                 manufacturer = \"...\"\n\
                 part_number = \"...\"\n\
                 category = \"motor|fastener|bearing|connector|other\"\n\n\
                 [dimensions]\n\
                 units = \"mm\"\n\
                 key_name = value\n\n\
                 [constraints]\n\
                 key_with_unit_suffix = value\n\n\
                 [sources]\n\
                 urls = [\"...\"]\n\
                 notes = \"...\"\n\
                 ```\n\
                 Return ONLY the TOML block, nothing else."
            );

            let result = claude::send_prompt(
                &model,
                "You are a technical reference researcher. Search for component datasheets and extract precise mechanical specifications.",
                None,
                &research_prompt,
                &images,
                Some(&stream_tx),
                Some(&bg_pid),
            );
            bg_pid.store(0, Ordering::SeqCst);
            let _ = tx.send(BackgroundResult::ReferenceResearch {
                name,
                result: result.map(|(response, _sid)| response),
            });
        });
    }
```

- [ ] **Step 3: Handle ReferenceResearch in handle_bg_result**

In `handle_bg_result`, add a new match arm after the `BuildComplete` arm:

```rust
            BackgroundResult::ReferenceResearch { name, result } => {
                match result {
                    Ok(response) => {
                        self.conversation.add("assistant", &response);
                        self.conversation.add("system", "Save as reference? (yes/no)");
                        self.ref_confirm_pending = Some(PendingReference {
                            name,
                            raw_response: response,
                        });
                    }
                    Err(e) => {
                        self.conversation.add("system", &format!("Research failed: {e}"));
                    }
                }
                self.busy = BusyState::Idle;
            }
```

- [ ] **Step 4: Implement save_pending_reference**

```rust
    fn save_pending_reference(&mut self, pending: PendingReference) {
        // Try to extract TOML from the response (look for ```toml block)
        let toml_str = if let Ok(extracted) = parser::parse_toml_response(&pending.raw_response) {
            extracted
        } else {
            // Try the whole response as TOML
            pending.raw_response.clone()
        };

        // Add timestamps
        let now = chrono::Utc::now().to_rfc3339();
        let toml_with_timestamps = toml_str
            .replace("created = \"\"", &format!("created = \"{now}\""))
            .replace("updated = \"\"", &format!("updated = \"{now}\""));
        let toml_str = if toml_with_timestamps.contains("created =") {
            toml_with_timestamps
        } else {
            toml_str
        };

        match toml::from_str::<reference::ReferenceComponent>(&toml_str) {
            Ok(mut comp) => {
                if comp.identity.created.is_empty() {
                    comp.identity.created = now.clone();
                }
                if comp.identity.updated.is_empty() {
                    comp.identity.updated = now;
                }
                let slug = reference::slug_from_name(&comp.identity.name);
                match reference::save(&comp) {
                    Ok(()) => {
                        if !self.active_refs.contains(&slug) {
                            self.active_refs.push(slug.clone());
                        }
                        self.conversation.add("system",
                            &format!("Saved reference '{}' as {slug}.toml", comp.identity.name));
                    }
                    Err(e) => self.conversation.add("system", &format!("Failed to save: {e}")),
                }
            }
            Err(e) => {
                self.conversation.add("system",
                    &format!("Failed to parse reference TOML: {e}\nTry `/ref refresh {}` to retry.", pending.name));
            }
        }
    }
```

- [ ] **Step 5: Build to verify compilation**

Run: `cargo build 2>&1 | rg "^error" || echo "BUILD OK"`
Expected: BUILD OK

- [ ] **Step 6: Commit**

```bash
git add src/main.rs src/tui/mod.rs
git commit -m "feat(reference): implement /ref command with research and save flow"
```

## Chunk 4: Spec Phase Integration

### Task 7: Wire auto-detection into handle_spec_response

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add detection call in handle_spec_response**

In `handle_spec_response`, after the code block stripping logic and before the `SPEC_COMPLETE` check, add:

```rust
        // Auto-detect external component references
        let known_slugs: Vec<String> = reference::load_library()
            .unwrap_or_default()
            .iter()
            .map(|(_, slug)| slug.clone())
            .collect();
        let detected = reference_detect::detect_references(clean_response, &known_slugs);
        for det in &detected {
            if det.in_library {
                self.conversation.add("system",
                    &format!("Reference available: {} (use /ref {} to load)", det.name,
                        reference::slug_from_name(&det.name)));
            } else {
                self.conversation.add("system",
                    &format!("Detected component: {}. Use /ref {} to research and save.",
                        det.name, reference::slug_from_name(&det.name)));
            }
        }
```

- [ ] **Step 2: Build to verify compilation**

Run: `cargo build 2>&1 | rg "^error" || echo "BUILD OK"`
Expected: BUILD OK

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat(reference): wire auto-detection into Spec phase response handler"
```

### Task 8: Add reference context injection into spec prompts

**Files:**
- Modify: `src/main.rs` (in `send_spec_prompt`)
- Modify: `src/claude.rs` (in `send_with_phase_prompt`)
- Modify: `prompts/spec.md`

- [ ] **Step 1: Update send_with_phase_prompt to accept optional ref context**

In `src/claude.rs`, change the signature:

```rust
pub fn send_with_phase_prompt(
    model: &Option<String>,
    phase_name: &str,
    session_id: Option<&str>,
    prompt: &str,
    image_paths: &[PathBuf],
    on_text: Option<&std::sync::mpsc::Sender<String>>,
    pid_out: Option<&std::sync::Arc<std::sync::atomic::AtomicU32>>,
    ref_context: Option<&str>,
) -> Result<(String, Option<String>), String> {
    let mut system_prompt = crate::prompt_builder::load_phase_system_prompt(phase_name)?;

    if let Some(ctx) = ref_context {
        system_prompt.push_str("\n\n");
        system_prompt.push_str(ctx);
    }
    // ... rest unchanged
```

Update all 6 existing call sites to pass `None` for the new parameter:
- `send_spec_prompt` — will pass `ref_context.as_deref()` (see Step 2)
- `send_decompose_prompt` — pass `None`
- `send_component_prompt` — pass `None`
- `send_component_feedback` — pass `None`
- `send_assembly_feedback` — pass `None` (if exists)
- `send_refinement_feedback` — pass `None` (if exists)

- [ ] **Step 2: Build reference context in send_spec_prompt**

In `send_spec_prompt` in `src/main.rs`, before spawning the thread, build the context:

```rust
        // Build reference context for prompt injection
        let ref_context = self.build_ref_context();
```

And add the helper method:

```rust
    fn build_ref_context(&self) -> Option<String> {
        let library = reference::load_library().unwrap_or_default();
        if library.is_empty() && self.active_refs.is_empty() {
            return None;
        }

        let mut parts = Vec::new();

        // Active references — full specs
        if !self.active_refs.is_empty() {
            let active: Vec<&reference::ReferenceComponent> = library.iter()
                .filter(|(_, slug)| self.active_refs.contains(slug))
                .map(|(comp, _)| comp)
                .collect();
            if !active.is_empty() {
                parts.push(format!(
                    "## Active Reference Components (use these dimensions)\n{}",
                    reference::summarize_for_prompt(&active)
                ));
            }
        }

        // All references — names only
        let all_refs: Vec<&reference::ReferenceComponent> = library.iter()
            .map(|(comp, _)| comp)
            .collect();
        if !all_refs.is_empty() {
            parts.push(format!(
                "## Available in Reference Library\n{}",
                reference::list_names(&all_refs)
            ));
        }

        if parts.is_empty() { None } else { Some(parts.join("\n\n")) }
    }
```

Pass `ref_context.as_deref()` to `send_with_phase_prompt` in the spec prompt thread.

For subsequent messages (when `session_id` is Some), prepend the context to the user message instead:

```rust
        let prompt = if self.claude_session_id.is_some() {
            // Subsequent message — inject as user message context
            if let Some(ref ctx) = ref_context {
                format!("[Reference context: {}]\n\n{}", ctx, text)
            } else {
                text.to_string()
            }
        } else {
            text.to_string()
        };
```

- [ ] **Step 3: Update prompts/spec.md**

Append to `prompts/spec.md`:

```markdown

When designing, prefer standard components from the reference library:
- Use standard metric fasteners (M2, M3, M4, M5) where appropriate
- Recommend threaded inserts (heat-set brass) for 3D printed assemblies
- Design mounting features around known reference dimensions
- Flag when a custom part could be replaced by a standard one

When you mention an external component (motor, bearing, fastener, connector, etc.)
that is NOT already listed as a reference, wrap it in a REF marker like: REF[component name]
This helps the system detect components that should be researched.
```

- [ ] **Step 4: Build to verify compilation**

Run: `cargo build 2>&1 | rg "^error" || echo "BUILD OK"`
Expected: BUILD OK

- [ ] **Step 5: Run all tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add src/main.rs src/claude.rs prompts/spec.md
git commit -m "feat(reference): inject reference context into spec prompts"
```

### Task 9: Seed the reference library with common components

**Files:**
- Create: `references/m3_shcs.toml` (seed file, copied to ~/MiModel/references/ on first run)
- Create: `references/m3x5x4_threaded_insert.toml`
- Modify: `src/main.rs` (seed on startup)

- [ ] **Step 1: Create seed reference files in the repo**

Create `references/m3_shcs.toml` and `references/m3x5x4_threaded_insert.toml` with the TOML content from the design spec examples. These live in the repo as seed data.

- [ ] **Step 2: Add seed logic to App::new or startup**

In `src/main.rs`, add a helper that copies seed references on first run:

```rust
fn seed_references() {
    let dir = reference::references_dir();
    if dir.exists() && std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0) > 0 {
        return; // Already has references
    }
    let _ = reference::ensure_references_dir();

    // Embed seed references
    let seeds: &[(&str, &str)] = &[
        ("m3_shcs.toml", include_str!("../references/m3_shcs.toml")),
        ("m3x5x4_threaded_insert.toml", include_str!("../references/m3x5x4_threaded_insert.toml")),
    ];
    for (name, content) in seeds {
        let path = dir.join(name);
        if !path.exists() {
            let _ = std::fs::write(&path, content);
        }
    }
}
```

Call `seed_references()` in `App::new()`, right after the `storage::project::ensure_root()?` call.

- [ ] **Step 3: Build and verify**

Run: `cargo build 2>&1 | rg "^error" || echo "BUILD OK"`
Expected: BUILD OK

- [ ] **Step 4: Commit**

```bash
git add references/ src/main.rs
git commit -m "feat(reference): seed library with M3 SHCS and threaded insert"
```

### Task 10: Final integration test

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 2: Manual smoke test**

Run: `cargo run`

1. Verify `/ref list` shows the seeded M3 references
2. Verify `/ref m3_shcs` loads and shows the reference summary
3. Start a Spec conversation mentioning "M3 screws" — verify detection message appears
4. Try `/ref nema23` — verify it enters research mode

- [ ] **Step 3: Commit any fixes from smoke test**

```bash
git add -A
git commit -m "fix: address issues found during smoke testing"
```
