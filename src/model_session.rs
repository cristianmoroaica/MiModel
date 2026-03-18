//! PhaseSession — phase-machine session with per-component directories.

use crate::component::ComponentState;
use crate::phase::Phase;
use crate::spec::ModelSpec;
use crate::storage::session::{ClaudeSessionMap, ConversationEntry, PhaseSessionData};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

// ---------------------------------------------------------------------------
// PhaseSession — phase-machine session with per-component directories
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct PhaseSession {
    pub base_dir: PathBuf,
    pub phase: Phase,
    pub spec: Option<ModelSpec>,
    pub components: Vec<ComponentState>,
    pub current_component_idx: Option<usize>,
    pub conversations: HashMap<String, Vec<ConversationEntry>>,
    pub claude_sessions: ClaudeSessionMap,
    pub build_timeout: Duration,
    pub python_path: String,
}

impl PhaseSession {
    /// Create a new phase session, setting up base_dir, components/, and assembly/ directories.
    pub fn new(base_dir: PathBuf, build_timeout: u64, python_path: String) -> Self {
        // Ensure base directory and subdirs exist
        fs::create_dir_all(&base_dir)
            .expect("Failed to create session directory");
        fs::create_dir_all(base_dir.join("components"))
            .expect("Failed to create components directory");
        fs::create_dir_all(base_dir.join("assembly"))
            .expect("Failed to create assembly directory");

        PhaseSession {
            base_dir,
            phase: Phase::Spec,
            spec: None,
            components: Vec::new(),
            current_component_idx: None,
            conversations: HashMap::new(),
            claude_sessions: ClaudeSessionMap::default(),
            build_timeout: Duration::from_secs(build_timeout),
            python_path,
        }
    }

    /// Initialize component directories and populate self.components.
    /// Each entry in `ids_and_names` is `(id, display_name)`.
    pub fn init_components(&mut self, ids_and_names: &[(&str, &str)]) -> Result<(), String> {
        self.components.clear();

        for &(id, name) in ids_and_names {
            let comp_dir = self.base_dir.join("components").join(id);
            let hist_dir = comp_dir.join("history");
            fs::create_dir_all(&hist_dir)
                .map_err(|e| format!("Failed to create component dir {}: {e}", id))?;

            let mut cs = ComponentState::new(id, name);
            cs.set_dir(comp_dir);
            self.components.push(cs);
        }

        Ok(())
    }

    /// Return the directory for a given component id.
    pub fn component_dir(&self, id: &str) -> PathBuf {
        self.base_dir.join("components").join(id)
    }

    /// Return the assembly directory.
    pub fn assembly_dir(&self) -> PathBuf {
        self.base_dir.join("assembly")
    }

    /// Atomic copy: write to _buffer.stl.tmp then rename to _buffer.stl.
    pub fn update_working_stl(&self, src: &Path) -> Result<(), String> {
        let tmp = self.base_dir.join("_buffer.stl.tmp");
        let dest = self.base_dir.join("_buffer.stl");
        fs::copy(src, &tmp)
            .map_err(|e| format!("Failed to copy STL to tmp: {e}"))?;
        fs::rename(&tmp, &dest)
            .map_err(|e| format!("Failed to rename _buffer.stl.tmp: {e}"))?;
        Ok(())
    }

    /// Atomic copy: write to _buffer.step.tmp then rename to _buffer.step.
    pub fn update_working_step(&self, src: &Path) -> Result<(), String> {
        let tmp = self.base_dir.join("_buffer.step.tmp");
        let dest = self.base_dir.join("_buffer.step");
        fs::copy(src, &tmp)
            .map_err(|e| format!("Failed to copy STEP to tmp: {e}"))?;
        fs::rename(&tmp, &dest)
            .map_err(|e| format!("Failed to rename _buffer.step.tmp: {e}"))?;
        Ok(())
    }

    /// Save session state to session.json (PhaseSessionData format) and spec.toml if spec exists.
    pub fn save(&self) -> Result<(), String> {
        let current_component = self.current_component_idx
            .and_then(|i| self.components.get(i))
            .map(|c| c.id.clone());

        let data = PhaseSessionData {
            name: self.base_dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unnamed".to_string()),
            created: chrono::Utc::now().to_rfc3339(),
            phase: self.phase,
            current_component,
            claude_sessions: self.claude_sessions.clone(),
            conversations: self.conversations.clone(),
            component_states: self.components.clone(),
        };

        let json = serde_json::to_string_pretty(&data)
            .map_err(|e| format!("Failed to serialize session: {e}"))?;
        fs::write(self.base_dir.join("session.json"), json)
            .map_err(|e| format!("Failed to write session.json: {e}"))?;

        if let Some(spec) = &self.spec {
            spec.save(&self.base_dir.join("spec.toml"))?;
        }

        Ok(())
    }

    /// Load a PhaseSession from an existing directory.
    pub fn load(dir: &Path, build_timeout: u64, python_path: String) -> Result<Self, String> {
        let json_path = dir.join("session.json");
        let json_str = fs::read_to_string(&json_path)
            .map_err(|e| format!("Failed to read session.json: {e}"))?;

        let data: PhaseSessionData = serde_json::from_str(&json_str)
            .map_err(|e| format!("Failed to parse session.json: {e}"))?;

        // Reconstruct component dirs from on-disk directories
        let mut components = data.component_states;
        for cs in &mut components {
            let comp_dir = dir.join("components").join(&cs.id);
            cs.set_dir(comp_dir);
        }

        let current_component_idx = data.current_component.and_then(|ref id| {
            components.iter().position(|c| c.id == *id)
        });

        // Load spec if it exists
        let spec_path = dir.join("spec.toml");
        let spec = if spec_path.exists() {
            Some(ModelSpec::load(&spec_path)?)
        } else {
            None
        };

        Ok(PhaseSession {
            base_dir: dir.to_path_buf(),
            phase: data.phase,
            spec,
            components,
            current_component_idx,
            conversations: data.conversations,
            claude_sessions: data.claude_sessions,
            build_timeout: Duration::from_secs(build_timeout),
            python_path,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_session_creates_dirs() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _session = PhaseSession::new(
            tmp.path().join("my_session"),
            60,
            "python".to_string(),
        );
        assert!(tmp.path().join("my_session/components").is_dir());
        assert!(tmp.path().join("my_session/assembly").is_dir());
    }

    #[test]
    fn test_init_components() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut session = PhaseSession::new(
            tmp.path().join("sess"),
            60,
            "python".to_string(),
        );
        session.init_components(&[("body", "Case Body"), ("cavity", "Cavity")]).unwrap();
        assert!(tmp.path().join("sess/components/body").is_dir());
        assert!(tmp.path().join("sess/components/body/history").is_dir());
        assert!(tmp.path().join("sess/components/cavity").is_dir());
        assert_eq!(session.components.len(), 2);
        assert_eq!(session.components[0].id, "body");
    }

    #[test]
    fn test_component_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let session = PhaseSession::new(
            tmp.path().join("sess"),
            60,
            "python".to_string(),
        );
        assert_eq!(
            session.component_dir("body"),
            tmp.path().join("sess/components/body"),
        );
    }

    #[test]
    fn test_assembly_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let session = PhaseSession::new(
            tmp.path().join("sess"),
            60,
            "python".to_string(),
        );
        assert_eq!(
            session.assembly_dir(),
            tmp.path().join("sess/assembly"),
        );
    }

    #[test]
    fn test_update_working_stl() {
        let tmp = tempfile::TempDir::new().unwrap();
        let session = PhaseSession::new(
            tmp.path().join("sess"),
            60,
            "python".to_string(),
        );
        // Create a dummy STL
        let src = tmp.path().join("test.stl");
        std::fs::write(&src, b"dummy stl").unwrap();
        session.update_working_stl(&src).unwrap();
        assert!(tmp.path().join("sess/_buffer.stl").exists());
        assert_eq!(std::fs::read(tmp.path().join("sess/_buffer.stl")).unwrap(), b"dummy stl");
    }

    #[test]
    fn test_update_working_step() {
        let tmp = tempfile::TempDir::new().unwrap();
        let session = PhaseSession::new(
            tmp.path().join("sess"),
            60,
            "python".to_string(),
        );
        let src = tmp.path().join("test.step");
        std::fs::write(&src, b"dummy step").unwrap();
        session.update_working_step(&src).unwrap();
        assert!(tmp.path().join("sess/_buffer.step").exists());
        assert_eq!(std::fs::read(tmp.path().join("sess/_buffer.step")).unwrap(), b"dummy step");
    }

    #[test]
    fn test_save_and_load() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut session = PhaseSession::new(
            tmp.path().join("sess"),
            60,
            "python".to_string(),
        );
        session.phase = Phase::Decompose;
        session.init_components(&[("body", "Body")]).unwrap();
        session.save().unwrap();

        assert!(tmp.path().join("sess/session.json").exists());

        let loaded = PhaseSession::load(
            &tmp.path().join("sess"),
            60,
            "python".to_string(),
        ).unwrap();
        assert_eq!(loaded.phase, Phase::Decompose);
        assert_eq!(loaded.components.len(), 1);
        assert_eq!(loaded.components[0].id, "body");
    }

    #[test]
    fn test_save_with_spec() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut session = PhaseSession::new(
            tmp.path().join("sess"),
            60,
            "python".to_string(),
        );
        session.spec = Some(ModelSpec {
            model: crate::spec::Model {
                name: "Test Model".to_string(),
                purpose: "testing".to_string(),
                units: "mm".to_string(),
                print_method: "FDM".to_string(),
                envelope: crate::spec::Envelope { max_x: 100.0, max_y: 100.0, max_z: 50.0 },
                features: crate::spec::ItemList { items: vec![] },
                constraints: crate::spec::ItemList { items: vec![] },
            },
            components: vec![],
            assembly: None,
        });
        session.save().unwrap();
        assert!(tmp.path().join("sess/spec.toml").exists());

        // Reload and verify spec is restored
        let loaded = PhaseSession::load(
            &tmp.path().join("sess"),
            60,
            "python".to_string(),
        ).unwrap();
        assert!(loaded.spec.is_some());
        assert_eq!(loaded.spec.unwrap().model.name, "Test Model");
    }
}
