use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssemblyManifest {
    pub components: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub id: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform: Option<Transform>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transform {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translate: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotate: Option<Rotation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rotation {
    pub axis: [f64; 3],
    pub degrees: f64,
}

impl AssemblyManifest {
    /// Build a manifest from component tree data.
    /// `approved` is a list of (id, assembly_op, assembly_target) for approved components.
    /// `base_dir` is the session directory containing components/<id>/<id>.py
    pub fn from_approved(
        approved: &[(String, String, String)], // (id, op, target)
        base_dir: &Path,
    ) -> Self {
        let mut entries = Vec::new();
        for (id, op, target) in approved {
            let path = base_dir
                .join("components")
                .join(id)
                .join(format!("{}.py", id))
                .display()
                .to_string();

            if op == "none" || target.is_empty() {
                entries.push(ManifestEntry {
                    id: id.clone(),
                    path,
                    role: Some("base".into()),
                    op: None,
                    from: None,
                    to: None,
                    transform: None,
                });
            } else {
                let (from_field, to_field) = if op == "subtract" {
                    (Some(target.clone()), None)
                } else {
                    (None, Some(target.clone()))
                };
                entries.push(ManifestEntry {
                    id: id.clone(),
                    path,
                    role: None,
                    op: Some(op.clone()),
                    from: from_field,
                    to: to_field,
                    transform: None,
                });
            }
        }
        Self { components: entries }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize manifest: {e}"))?;
        std::fs::write(path, json)
            .map_err(|e| format!("Failed to write manifest: {e}"))?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let json = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read manifest: {e}"))?;
        serde_json::from_str(&json)
            .map_err(|e| format!("Failed to parse manifest: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_single_base() {
        let manifest = AssemblyManifest::from_approved(
            &[("body".into(), "none".into(), "".into())],
            Path::new("/tmp/session"),
        );
        assert_eq!(manifest.components.len(), 1);
        assert_eq!(manifest.components[0].role, Some("base".into()));
        assert!(manifest.components[0].op.is_none());
    }

    #[test]
    fn test_manifest_with_operations() {
        let manifest = AssemblyManifest::from_approved(
            &[
                ("body".into(), "none".into(), "".into()),
                ("cavity".into(), "subtract".into(), "body".into()),
                ("lugs".into(), "fuse".into(), "body".into()),
            ],
            Path::new("/tmp/session"),
        );
        assert_eq!(manifest.components.len(), 3);
        assert_eq!(manifest.components[1].op, Some("subtract".into()));
        assert_eq!(manifest.components[1].from, Some("body".into()));
        assert_eq!(manifest.components[2].op, Some("fuse".into()));
        assert_eq!(manifest.components[2].to, Some("body".into()));
    }

    #[test]
    fn test_manifest_serialization() {
        let manifest = AssemblyManifest::from_approved(
            &[("body".into(), "none".into(), "".into())],
            Path::new("/tmp"),
        );
        let json = serde_json::to_string_pretty(&manifest).unwrap();
        assert!(json.contains("\"role\": \"base\""));
        // Fields that are None should not appear
        assert!(!json.contains("\"op\""));
    }

    #[test]
    fn test_manifest_save_and_load() {
        let tmp = tempfile::TempDir::new().unwrap();
        let manifest = AssemblyManifest::from_approved(
            &[("body".into(), "none".into(), "".into())],
            Path::new("/tmp"),
        );
        let path = tmp.path().join("manifest.json");
        manifest.save(&path).unwrap();
        let loaded = AssemblyManifest::load(&path).unwrap();
        assert_eq!(loaded.components.len(), 1);
    }
}
