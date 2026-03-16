//! Project directory CRUD operations.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    pub created: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub path: PathBuf,
    pub meta: ProjectMeta,
    pub sessions: Vec<String>, // session directory names
}

/// Get the root storage directory: ~/MiModel/
pub fn root_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("MiModel")
}

/// Ensure ~/MiModel/ exists. Creates with a default "Untitled" project if missing.
pub fn ensure_root() -> Result<PathBuf, String> {
    let root = root_dir();
    if !root.exists() {
        std::fs::create_dir_all(&root)
            .map_err(|e| format!("Failed to create ~/MiModel/: {e}"))?;
        create_project("Untitled", "")?;
    }
    Ok(root)
}

/// List all projects in ~/MiModel/.
pub fn list_projects() -> Result<Vec<Project>, String> {
    let root = root_dir();
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut projects = Vec::new();
    let entries = std::fs::read_dir(&root)
        .map_err(|e| format!("Failed to read ~/MiModel/: {e}"))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() { continue; }

        let meta_path = path.join("project.json");
        let meta = if meta_path.exists() {
            let json = std::fs::read_to_string(&meta_path).unwrap_or_default();
            serde_json::from_str(&json).unwrap_or(ProjectMeta {
                name: path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                created: String::new(),
                description: String::new(),
            })
        } else {
            ProjectMeta {
                name: path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                created: String::new(),
                description: String::new(),
            }
        };

        // List session subdirectories
        let mut sessions = Vec::new();
        if let Ok(sub_entries) = std::fs::read_dir(&path) {
            for sub in sub_entries.flatten() {
                let sub_path = sub.path();
                if sub_path.is_dir() && sub_path.join("session.json").exists() {
                    if let Some(name) = sub_path.file_name() {
                        sessions.push(name.to_string_lossy().to_string());
                    }
                }
            }
        }
        sessions.sort();

        projects.push(Project { path, meta, sessions });
    }

    projects.sort_by(|a, b| a.meta.name.cmp(&b.meta.name));
    Ok(projects)
}

/// Create a new project directory with project.json.
pub fn create_project(name: &str, description: &str) -> Result<PathBuf, String> {
    let path = root_dir().join(name);
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("Failed to create project dir: {e}"))?;

    let meta = ProjectMeta {
        name: name.to_string(),
        created: chrono::Utc::now().to_rfc3339(),
        description: description.to_string(),
    };
    let json = serde_json::to_string_pretty(&meta)
        .map_err(|e| format!("Failed to serialize project: {e}"))?;
    std::fs::write(path.join("project.json"), json)
        .map_err(|e| format!("Failed to write project.json: {e}"))?;

    Ok(path)
}

/// Delete a project and all its sessions.
pub fn delete_project(name: &str) -> Result<(), String> {
    let path = root_dir().join(name);
    if path.exists() {
        std::fs::remove_dir_all(&path)
            .map_err(|e| format!("Failed to delete project: {e}"))?;
    }
    Ok(())
}

/// Rename a project.
pub fn rename_project(old_name: &str, new_name: &str) -> Result<(), String> {
    let old_path = root_dir().join(old_name);
    let new_path = root_dir().join(new_name);
    std::fs::rename(&old_path, &new_path)
        .map_err(|e| format!("Failed to rename project: {e}"))?;

    // Update project.json
    let meta_path = new_path.join("project.json");
    if meta_path.exists() {
        if let Ok(json) = std::fs::read_to_string(&meta_path) {
            if let Ok(mut meta) = serde_json::from_str::<ProjectMeta>(&json) {
                meta.name = new_name.to_string();
                if let Ok(updated) = serde_json::to_string_pretty(&meta) {
                    let _ = std::fs::write(&meta_path, updated);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::TempDir;

    // Serialize tests that mutate HOME env var to avoid race conditions.
    static HOME_LOCK: Mutex<()> = Mutex::new(());

    fn with_test_root(f: impl FnOnce()) {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = TempDir::new().unwrap();
        std::env::set_var("HOME", tmp.path());
        f();
        std::env::remove_var("HOME");
    }

    #[test]
    fn test_ensure_root_creates_default_project() {
        with_test_root(|| {
            let root = ensure_root().unwrap();
            assert!(root.exists());
            assert!(root.join("Untitled/project.json").exists());
        });
    }

    #[test]
    fn test_create_and_list_projects() {
        with_test_root(|| {
            ensure_root().unwrap();
            create_project("Test Project", "A test").unwrap();
            let projects = list_projects().unwrap();
            assert!(projects.iter().any(|p| p.meta.name == "Test Project"));
        });
    }

    #[test]
    fn test_delete_project() {
        with_test_root(|| {
            ensure_root().unwrap();
            create_project("ToDelete", "").unwrap();
            delete_project("ToDelete").unwrap();
            let projects = list_projects().unwrap();
            assert!(!projects.iter().any(|p| p.meta.name == "ToDelete"));
        });
    }

    #[test]
    fn test_rename_project() {
        with_test_root(|| {
            ensure_root().unwrap();
            create_project("OldName", "").unwrap();
            rename_project("OldName", "NewName").unwrap();
            let projects = list_projects().unwrap();
            assert!(projects.iter().any(|p| p.meta.name == "NewName"));
            assert!(!projects.iter().any(|p| p.meta.name == "OldName"));
        });
    }
}
