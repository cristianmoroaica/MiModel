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
    #[serde(flatten)]
    pub values: HashMap<String, toml::Value>,
}

fn default_units() -> String {
    "mm".to_string()
}

impl Dimensions {
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

/// Returns the global references directory: ~/MiModel/references/
pub fn references_dir() -> PathBuf {
    crate::storage::project::root_dir().join("references")
}

/// Ensures the global references directory exists, creating it if needed.
pub fn ensure_references_dir() -> Result<PathBuf, String> {
    let dir = references_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create references dir: {e}"))?;
    Ok(dir)
}

/// Write `<slug>.toml` for the component into the given directory.
pub fn save_to_dir(component: &ReferenceComponent, dir: &Path) -> Result<String, String> {
    let slug = slug_from_name(&component.identity.name);
    let path = dir.join(format!("{slug}.toml"));
    let toml_str = toml::to_string_pretty(component)
        .map_err(|e| format!("Failed to serialize component: {e}"))?;
    std::fs::write(&path, toml_str)
        .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
    Ok(slug)
}

/// Write the component to the global references directory.
pub fn save(component: &ReferenceComponent) -> Result<String, String> {
    let dir = ensure_references_dir()?;
    save_to_dir(component, &dir)
}

/// Load a single component by exact slug from the given directory.
pub fn load_one_from_dir(slug: &str, dir: &Path) -> Result<(ReferenceComponent, String), String> {
    let path = dir.join(format!("{slug}.toml"));
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    let component: ReferenceComponent = toml::from_str(&content)
        .map_err(|e| format!("Failed to parse {}: {e}", path.display()))?;
    Ok((component, slug.to_string()))
}

/// Load a single component by query from the global references directory.
///
/// Strategy: exact slug match first, then fuzzy substring match on identity.name.
/// Returns an error listing all matches if more than one fuzzy match is found.
pub fn load_one(query: &str) -> Result<(ReferenceComponent, String), String> {
    let dir = references_dir();
    // Try exact slug match first
    if let Ok(result) = load_one_from_dir(query, &dir) {
        return Ok(result);
    }
    // Fall back to fuzzy substring match on identity.name
    let library = load_library_from_dir(&dir)?;
    let query_lower = query.to_lowercase();
    let matches: Vec<(ReferenceComponent, String)> = library
        .into_iter()
        .filter(|(comp, _slug)| comp.identity.name.to_lowercase().contains(&query_lower))
        .collect();
    match matches.len() {
        0 => Err(format!("No component found matching '{query}'")),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => {
            let names: Vec<String> = matches.iter().map(|(c, _)| c.identity.name.clone()).collect();
            Err(format!(
                "Multiple components match '{query}': {}",
                names.join(", ")
            ))
        }
    }
}

/// Read all *.toml files from the given directory and return (component, slug) pairs.
pub fn load_library_from_dir(dir: &Path) -> Result<Vec<(ReferenceComponent, String)>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {}: {e}", dir.display()))?;
    let mut results = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read dir entry: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let slug = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        let component: ReferenceComponent = toml::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {e}", path.display()))?;
        results.push((component, slug));
    }
    Ok(results)
}

/// Read all components from the global references directory.
pub fn load_library() -> Result<Vec<(ReferenceComponent, String)>, String> {
    load_library_from_dir(&references_dir())
}

/// Format a slice of reference components as a concise prompt-ready summary.
///
/// Output format per component:
/// `- NEMA 23 Stepper Motor: body_width=57.2, shaft_diameter=6.35 (mm)`
pub fn summarize_for_prompt(refs: &[&ReferenceComponent]) -> String {
    refs.iter()
        .map(|c| {
            let mut keys: Vec<&String> = c.dimensions.values.keys().collect();
            keys.sort();
            let dims: Vec<String> = keys
                .iter()
                .filter_map(|k| c.dimensions.get_f64(k).map(|v| format!("{k}={v}")))
                .collect();
            if dims.is_empty() {
                format!("- {}", c.identity.name)
            } else {
                format!(
                    "- {}: {} ({})",
                    c.identity.name,
                    dims.join(", "),
                    c.dimensions.units
                )
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Format a slice of reference components as a simple name list.
///
/// Output format per component:
/// `- NEMA 23 Stepper Motor [motor]`
pub fn list_names(refs: &[&ReferenceComponent]) -> String {
    refs.iter()
        .map(|c| {
            if c.identity.category.is_empty() {
                format!("- {}", c.identity.name)
            } else {
                format!("- {} [{}]", c.identity.name, c.identity.category)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_bearing() -> ReferenceComponent {
        let mut dim_values = HashMap::new();
        dim_values.insert("outer_diameter".to_string(), toml::Value::Float(22.0));
        dim_values.insert("inner_diameter".to_string(), toml::Value::Float(8.0));
        dim_values.insert("width".to_string(), toml::Value::Float(7.0));

        ReferenceComponent {
            identity: Identity {
                name: "608ZZ Bearing".to_string(),
                manufacturer: "Generic".to_string(),
                part_number: "608ZZ".to_string(),
                category: "bearing".to_string(),
                created: "2026-03-17".to_string(),
                updated: String::new(),
            },
            dimensions: Dimensions {
                units: "mm".to_string(),
                values: dim_values,
            },
            constraints: HashMap::new(),
            sources: Sources {
                urls: vec!["https://example.com/608zz".to_string()],
                notes: "Standard skate bearing".to_string(),
            },
        }
    }

    #[test]
    fn test_slug_from_name() {
        assert_eq!(slug_from_name("NEMA 23"), "nema_23");
        assert_eq!(slug_from_name("NEMA23"), "nema23");
        assert_eq!(slug_from_name("M3x8 SHCS"), "m3x8_shcs");
        assert_eq!(slug_from_name("Sellita SW280-1"), "sellita_sw280-1");
        assert_eq!(slug_from_name("  Spaces   Everywhere  "), "spaces_everywhere");
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let tmp = TempDir::new().expect("tempdir");
        let component = make_bearing();

        let slug = save_to_dir(&component, tmp.path()).expect("save");
        assert_eq!(slug, "608zz_bearing");

        let (loaded, loaded_slug) = load_one_from_dir(&slug, tmp.path()).expect("load");
        assert_eq!(loaded_slug, slug);
        assert_eq!(loaded.identity.name, "608ZZ Bearing");
        assert_eq!(loaded.identity.manufacturer, "Generic");
        assert_eq!(loaded.identity.part_number, "608ZZ");
        assert_eq!(loaded.identity.category, "bearing");
        assert_eq!(loaded.identity.created, "2026-03-17");
        assert_eq!(loaded.dimensions.units, "mm");
        assert_eq!(loaded.dimensions.get_f64("outer_diameter"), Some(22.0));
        assert_eq!(loaded.dimensions.get_f64("inner_diameter"), Some(8.0));
        assert_eq!(loaded.dimensions.get_f64("width"), Some(7.0));
        assert_eq!(loaded.sources.notes, "Standard skate bearing");
        assert_eq!(loaded.sources.urls, vec!["https://example.com/608zz"]);
    }

    #[test]
    fn test_load_library() {
        let tmp = TempDir::new().expect("tempdir");

        let bearing = make_bearing();
        let mut motor_dims = HashMap::new();
        motor_dims.insert("body_width".to_string(), toml::Value::Float(57.2));
        motor_dims.insert("shaft_diameter".to_string(), toml::Value::Float(6.35));
        let motor = ReferenceComponent {
            identity: Identity {
                name: "NEMA 23 Stepper Motor".to_string(),
                manufacturer: String::new(),
                part_number: String::new(),
                category: "motor".to_string(),
                created: "2026-03-17".to_string(),
                updated: String::new(),
            },
            dimensions: Dimensions {
                units: "mm".to_string(),
                values: motor_dims,
            },
            constraints: HashMap::new(),
            sources: Sources {
                urls: Vec::new(),
                notes: String::new(),
            },
        };

        save_to_dir(&bearing, tmp.path()).expect("save bearing");
        save_to_dir(&motor, tmp.path()).expect("save motor");

        let library = load_library_from_dir(tmp.path()).expect("load library");
        assert_eq!(library.len(), 2);
    }

    #[test]
    fn test_summarize_for_prompt() {
        let tmp = TempDir::new().expect("tempdir");
        let mut motor_dims = HashMap::new();
        motor_dims.insert("body_width".to_string(), toml::Value::Float(57.2));
        motor_dims.insert("shaft_diameter".to_string(), toml::Value::Float(6.35));
        let motor = ReferenceComponent {
            identity: Identity {
                name: "NEMA 23 Stepper Motor".to_string(),
                manufacturer: String::new(),
                part_number: String::new(),
                category: "motor".to_string(),
                created: "2026-03-17".to_string(),
                updated: String::new(),
            },
            dimensions: Dimensions {
                units: "mm".to_string(),
                values: motor_dims,
            },
            constraints: HashMap::new(),
            sources: Sources {
                urls: Vec::new(),
                notes: String::new(),
            },
        };
        save_to_dir(&motor, tmp.path()).expect("save motor");

        let summary = summarize_for_prompt(&[&motor]);
        assert!(
            summary.contains("NEMA 23 Stepper Motor"),
            "summary missing name: {summary}"
        );
        assert!(
            summary.contains("57.2") || summary.contains("body_width"),
            "summary missing dimension: {summary}"
        );
        assert!(
            summary.contains("mm"),
            "summary missing units: {summary}"
        );
    }

    #[test]
    fn test_list_names() {
        let bearing = make_bearing();
        let output = list_names(&[&bearing]);
        assert!(
            output.contains("608ZZ Bearing"),
            "list_names missing name: {output}"
        );
        assert!(
            output.contains("bearing"),
            "list_names missing category: {output}"
        );
        assert!(
            output.contains('[') && output.contains(']'),
            "list_names missing brackets: {output}"
        );
    }
}
