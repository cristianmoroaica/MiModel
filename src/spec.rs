use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSpec {
    pub model: Model,
    #[serde(default)]
    pub components: Vec<Component>,
    pub assembly: Option<Assembly>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub name: String,
    pub purpose: String,
    pub units: String,
    pub print_method: String,
    pub envelope: Envelope,
    pub features: ItemList,
    pub constraints: ItemList,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemList {
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Component {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub assembly_op: String,
    pub assembly_target: String,
    #[serde(default)]
    pub parameters: HashMap<String, Parameter>,
    pub constraints: ItemList,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    pub value: f64,
    pub unit: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assembly {
    pub order: Vec<String>,
    pub notes: String,
}

impl ModelSpec {
    /// Read a TOML file from disk and parse it into a ModelSpec.
    pub fn load(path: &Path) -> Result<Self, String> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        toml::from_str(&contents)
            .map_err(|e| format!("Failed to parse TOML in {}: {}", path.display(), e))
    }

    /// Serialize this ModelSpec to pretty TOML and write it to disk.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let contents = toml::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize spec: {}", e))?;
        std::fs::write(path, contents)
            .map_err(|e| format!("Failed to write {}: {}", path.display(), e))
    }

    /// Validate the spec:
    ///   - No duplicate component IDs.
    ///   - All `depends_on` entries reference existing component IDs.
    ///   - No dependency cycles (DFS-based).
    pub fn validate(&self) -> Result<(), String> {
        // Collect all IDs and check for duplicates.
        let mut seen: HashSet<&str> = HashSet::new();
        for comp in &self.components {
            if !seen.insert(comp.id.as_str()) {
                return Err(format!("Duplicate component id: {}", comp.id));
            }
        }

        // Check all depends_on refer to known IDs.
        for comp in &self.components {
            for dep in &comp.depends_on {
                if !seen.contains(dep.as_str()) {
                    return Err(format!(
                        "Component '{}' depends on unknown id '{}'",
                        comp.id, dep
                    ));
                }
            }
        }

        // Build adjacency list (id -> depends_on ids).
        let adj: HashMap<&str, &[String]> = self
            .components
            .iter()
            .map(|c| (c.id.as_str(), c.depends_on.as_slice()))
            .collect();

        // DFS cycle detection using three-color marking:
        //   White (0) = unvisited, Grey (1) = in current path, Black (2) = done.
        let mut color: HashMap<&str, u8> = HashMap::new();

        fn dfs<'a>(
            node: &'a str,
            adj: &HashMap<&'a str, &'a [String]>,
            color: &mut HashMap<&'a str, u8>,
        ) -> Result<(), String> {
            color.insert(node, 1); // grey
            if let Some(deps) = adj.get(node) {
                for dep in *deps {
                    let dep_str = dep.as_str();
                    match color.get(dep_str).copied().unwrap_or(0) {
                        1 => {
                            return Err(format!(
                                "Cycle detected: '{}' -> '{}'",
                                node, dep_str
                            ));
                        }
                        0 => dfs(dep_str, adj, color)?,
                        _ => {} // black = already fully processed
                    }
                }
            }
            color.insert(node, 2); // black
            Ok(())
        }

        for comp in &self.components {
            if color.get(comp.id.as_str()).copied().unwrap_or(0) == 0 {
                dfs(comp.id.as_str(), &adj, &mut color)?;
            }
        }

        Ok(())
    }

    /// Return components in build order.
    ///
    /// If `assembly.order` is present, use that ordering (filtering to
    /// components that exist).  Otherwise perform a topological sort.
    pub fn build_order(&self) -> Result<Vec<&Component>, String> {
        // Validate first so callers can rely on a consistent state.
        self.validate()?;

        if let Some(assembly) = &self.assembly {
            if !assembly.order.is_empty() {
                let by_id: HashMap<&str, &Component> = self
                    .components
                    .iter()
                    .map(|c| (c.id.as_str(), c))
                    .collect();
                let mut ordered = Vec::new();
                for id in &assembly.order {
                    if let Some(comp) = by_id.get(id.as_str()) {
                        ordered.push(*comp);
                    }
                }
                return Ok(ordered);
            }
        }

        // Topological sort (Kahn's algorithm).
        let n = self.components.len();
        let index: HashMap<&str, usize> = self
            .components
            .iter()
            .enumerate()
            .map(|(i, c)| (c.id.as_str(), i))
            .collect();

        let mut in_degree: Vec<usize> = vec![0; n];
        // adjacency: who comes after each node
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];

        for comp in &self.components {
            let ci = index[comp.id.as_str()];
            for dep in &comp.depends_on {
                let di = index[dep.as_str()];
                adj[di].push(ci);
                in_degree[ci] += 1;
            }
        }

        let mut queue: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
        let mut result: Vec<&Component> = Vec::new();

        while !queue.is_empty() {
            // Sort queue by id for deterministic ordering.
            queue.sort_by_key(|&i| self.components[i].id.as_str());
            let node = queue.remove(0);
            result.push(&self.components[node]);
            for &next in &adj[node] {
                in_degree[next] -= 1;
                if in_degree[next] == 0 {
                    queue.push(next);
                }
            }
        }

        if result.len() != n {
            return Err("Cycle detected during topological sort".to_string());
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: build a minimal valid Model.
    fn make_model(name: &str) -> Model {
        Model {
            name: name.to_string(),
            purpose: "testing".to_string(),
            units: "mm".to_string(),
            print_method: "FDM".to_string(),
            envelope: Envelope { max_x: 100.0, max_y: 100.0, max_z: 50.0 },
            features: ItemList { items: vec!["feature_a".to_string(), "feature_b".to_string()] },
            constraints: ItemList { items: vec!["must fit on 220mm bed".to_string()] },
        }
    }

    // Helper: build a minimal Component.
    fn make_component(id: &str, depends_on: Vec<&str>) -> Component {
        Component {
            id: id.to_string(),
            name: format!("{} name", id),
            description: format!("{} description", id),
            depends_on: depends_on.into_iter().map(String::from).collect(),
            assembly_op: "union".to_string(),
            assembly_target: "body".to_string(),
            parameters: {
                let mut m = HashMap::new();
                m.insert(
                    "width".to_string(),
                    Parameter { value: 42.0, unit: "mm".to_string(), description: "Width of part".to_string() },
                );
                m
            },
            constraints: ItemList { items: vec![] },
        }
    }

    #[test]
    fn test_parse_model_only_spec() {
        let toml_str = r#"
[model]
name = "Simple Box"
purpose = "store small items"
units = "mm"
print_method = "FDM"

[model.envelope]
max_x = 80.0
max_y = 60.0
max_z = 40.0

[model.features]
items = ["removable lid", "snap-fit closure"]

[model.constraints]
items = ["must fit 220mm bed", "wall thickness >= 2mm"]
"#;
        let spec: ModelSpec = toml::from_str(toml_str).unwrap();

        assert_eq!(spec.model.name, "Simple Box");
        assert_eq!(spec.model.envelope.max_x, 80.0);
        assert_eq!(spec.model.envelope.max_y, 60.0);
        assert_eq!(spec.model.envelope.max_z, 40.0);
        assert_eq!(spec.model.features.items.len(), 2);
        assert!(spec.components.is_empty(), "expected no components");
        assert!(spec.assembly.is_none(), "expected no assembly");
    }

    #[test]
    fn test_parse_full_spec_with_components() {
        let toml_str = r#"
[model]
name = "Watch Case"
purpose = "protect movement"
units = "mm"
print_method = "SLA"

[model.envelope]
max_x = 45.0
max_y = 45.0
max_z = 15.0

[model.features]
items = ["crown hole", "crystal seat"]

[model.constraints]
items = ["water resistant"]

[[components]]
id = "case_body"
name = "Case Body"
description = "Main case shell"
depends_on = []
assembly_op = "base"
assembly_target = ""

[components.parameters.wall_thickness]
value = 1.5
unit = "mm"
description = "Wall thickness"

[components.constraints]
items = []

[[components]]
id = "bezel"
name = "Bezel Ring"
description = "Decorative ring around crystal"
depends_on = ["case_body"]
assembly_op = "union"
assembly_target = "case_body"

[components.parameters.bezel_width]
value = 3.0
unit = "mm"
description = "Width of bezel"

[components.constraints]
items = []

[assembly]
order = ["case_body", "bezel"]
notes = "Assemble bezel onto case body after curing"
"#;
        let spec: ModelSpec = toml::from_str(toml_str).unwrap();

        assert_eq!(spec.components.len(), 2);
        assert_eq!(spec.components[0].id, "case_body");
        assert_eq!(
            spec.components[0].parameters["wall_thickness"].value,
            1.5
        );
        assert_eq!(spec.components[1].id, "bezel");
        assert_eq!(spec.components[1].depends_on, vec!["case_body"]);
        assert!(spec.assembly.is_some());
        let asm = spec.assembly.as_ref().unwrap();
        assert_eq!(asm.order, vec!["case_body", "bezel"]);
    }

    #[test]
    fn test_roundtrip_serialize() {
        let original = ModelSpec {
            model: make_model("Roundtrip Model"),
            components: vec![make_component("part_a", vec![])],
            assembly: Some(Assembly {
                order: vec!["part_a".to_string()],
                notes: "single part".to_string(),
            }),
        };

        let toml_str = toml::to_string_pretty(&original).expect("serialize failed");
        let parsed: ModelSpec = toml::from_str(&toml_str).expect("parse failed");

        assert_eq!(parsed.model.name, original.model.name);
        assert_eq!(parsed.model.envelope.max_x, original.model.envelope.max_x);
        assert_eq!(parsed.components.len(), 1);
        assert_eq!(parsed.components[0].id, "part_a");
        assert_eq!(parsed.components[0].parameters["width"].value, 42.0);
        let asm = parsed.assembly.as_ref().unwrap();
        assert_eq!(asm.order, vec!["part_a"]);
        assert_eq!(asm.notes, "single part");
    }

    #[test]
    fn test_validate_no_cycles() {
        // a has no deps; b depends on a — valid DAG.
        let spec = ModelSpec {
            model: make_model("DAG Model"),
            components: vec![
                make_component("a", vec![]),
                make_component("b", vec!["a"]),
            ],
            assembly: None,
        };
        assert!(spec.validate().is_ok(), "expected Ok for valid DAG");
    }

    #[test]
    fn test_validate_detects_cycle() {
        // a depends on b, b depends on a — cycle.
        let spec = ModelSpec {
            model: make_model("Cycle Model"),
            components: vec![
                make_component("a", vec!["b"]),
                make_component("b", vec!["a"]),
            ],
            assembly: None,
        };
        let result = spec.validate();
        assert!(result.is_err(), "expected Err for cyclic deps");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("ycle"),
            "error message should mention cycle, got: {}",
            msg
        );
    }

    #[test]
    fn test_validate_duplicate_ids() {
        let spec = ModelSpec {
            model: make_model("Dup Model"),
            components: vec![
                make_component("dup", vec![]),
                make_component("dup", vec![]),
            ],
            assembly: None,
        };
        let result = spec.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Duplicate"));
    }

    #[test]
    fn test_validate_unknown_dependency() {
        let spec = ModelSpec {
            model: make_model("Unknown Dep Model"),
            components: vec![make_component("a", vec!["ghost"])],
            assembly: None,
        };
        let result = spec.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown id"));
    }

    #[test]
    fn test_build_order_uses_assembly_order() {
        let spec = ModelSpec {
            model: make_model("Build Order Model"),
            components: vec![
                make_component("a", vec![]),
                make_component("b", vec!["a"]),
                make_component("c", vec!["b"]),
            ],
            assembly: Some(Assembly {
                order: vec!["a".to_string(), "b".to_string(), "c".to_string()],
                notes: "".to_string(),
            }),
        };
        let order = spec.build_order().unwrap();
        let ids: Vec<&str> = order.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_build_order_topological_without_assembly() {
        let spec = ModelSpec {
            model: make_model("Topo Model"),
            components: vec![
                make_component("b", vec!["a"]),
                make_component("a", vec![]),
            ],
            assembly: None,
        };
        let order = spec.build_order().unwrap();
        let ids: Vec<&str> = order.iter().map(|c| c.id.as_str()).collect();
        // a has no deps so must come before b.
        assert_eq!(ids[0], "a");
        assert_eq!(ids[1], "b");
    }

    #[test]
    fn test_load_save_roundtrip() {
        let original = ModelSpec {
            model: make_model("IO Model"),
            components: vec![],
            assembly: None,
        };

        let tmp = tempfile::NamedTempFile::new().unwrap();
        original.save(tmp.path()).expect("save failed");
        let loaded = ModelSpec::load(tmp.path()).expect("load failed");

        assert_eq!(loaded.model.name, "IO Model");
        assert_eq!(loaded.model.units, "mm");
        assert!(loaded.components.is_empty());
        assert!(loaded.assembly.is_none());
    }
}
