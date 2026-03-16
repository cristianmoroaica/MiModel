//! Session manager — conversation, iterations, undo, temp files.

use crate::claude::Message;
use crate::component::ComponentState;
use crate::phase::Phase;
use crate::python::{self, BuildResult, Engine, ModelMetadata};
use crate::spec::ModelSpec;
use crate::storage::session::{ClaudeSessionMap, ConversationEntry, PhaseSessionData};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

// ---------------------------------------------------------------------------
// LegacySession (was "Session") — flat iteration-based model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum SessionState {
    Idle,
    Reviewing,
    Error(String),
}

#[derive(Debug, Clone)]
struct Snapshot {
    iteration: u32,
    messages: Vec<Message>,
    metadata: Option<ModelMetadata>,
    code: Option<String>,
    engine: Option<Engine>,
}

pub struct LegacySession {
    pub state: SessionState,
    pub messages: Vec<Message>,
    pub current_metadata: Option<ModelMetadata>,
    pub current_code: Option<String>,
    pub current_engine: Option<Engine>,
    iteration: u32,
    undo_snapshot: Option<Snapshot>,
    temp_dir: PathBuf,
    build_timeout: Duration,
    python_path: String,
}

impl LegacySession {
    pub fn new(build_timeout: u64, python_path: String) -> Self {
        #[allow(deprecated)]
        let temp_dir = tempfile::tempdir()
            .expect("Failed to create temp directory")
            .into_path();

        LegacySession {
            state: SessionState::Idle,
            messages: Vec::new(),
            current_metadata: None,
            current_code: None,
            current_engine: None,
            iteration: 0,
            undo_snapshot: None,
            temp_dir,
            build_timeout: Duration::from_secs(build_timeout),
            python_path,
        }
    }

    fn snapshot(&mut self) {
        self.undo_snapshot = Some(Snapshot {
            iteration: self.iteration,
            messages: self.messages.clone(),
            metadata: self.current_metadata.clone(),
            code: self.current_code.clone(),
            engine: self.current_engine,
        });
    }

    pub fn undo(&mut self) -> bool {
        if let Some(snap) = self.undo_snapshot.take() {
            self.iteration = snap.iteration;
            self.messages = snap.messages;
            self.current_metadata = snap.metadata;
            self.current_code = snap.code;
            self.current_engine = snap.engine;
            self.update_symlink();
            self.state = if self.current_metadata.is_some() {
                SessionState::Reviewing
            } else {
                SessionState::Idle
            };
            true
        } else {
            false
        }
    }

    pub fn add_user_message(&mut self, content: &str) {
        self.messages.push(Message { role: "user".to_string(), content: content.to_string() });
    }

    pub fn add_assistant_message(&mut self, content: &str) {
        self.messages.push(Message { role: "assistant".to_string(), content: content.to_string() });
    }

    pub fn build(&mut self, code: &str, engine: Engine) -> BuildResult {
        self.snapshot();
        self.iteration += 1;

        let ext = engine.file_extension();
        let code_path = self.temp_dir.join(format!("iter_{:03}.{}", self.iteration, ext));
        let stl_path = self.temp_dir.join(format!("iter_{:03}.stl", self.iteration));

        fs::write(&code_path, code).expect("Failed to write code file");

        let result = python::build(&self.python_path, &code_path, &stl_path, engine, self.build_timeout);

        match &result {
            BuildResult::Success(meta) => {
                self.current_metadata = Some(meta.clone());
                self.current_code = Some(code.to_string());
                self.current_engine = Some(engine);
                self.state = SessionState::Reviewing;
                self.update_symlink();
            }
            BuildResult::Timeout => {
                self.state = SessionState::Error(format!(
                    "Build timed out after {}s", self.build_timeout.as_secs()
                ));
            }
            BuildResult::BuildError(e) | BuildResult::SyntaxError(e) => {
                self.state = SessionState::Error(e.error.clone());
            }
        }

        result
    }

    fn update_symlink(&self) {
        let symlink = self.temp_dir.join("current.stl");
        let _ = fs::remove_file(&symlink);
        let target = self.temp_dir.join(format!("iter_{:03}.stl", self.iteration));
        if target.exists() {
            #[cfg(unix)]
            { let _ = std::os::unix::fs::symlink(&target, &symlink); }
        }
    }

    pub fn current_stl_path(&self) -> PathBuf {
        self.temp_dir.join("current.stl")
    }

    pub fn latest_stl_path(&self) -> Option<PathBuf> {
        let p = self.temp_dir.join(format!("iter_{:03}.stl", self.iteration));
        if p.exists() { Some(p) } else { None }
    }

    pub fn export(&self, dest: &Path) -> Result<(), String> {
        let src = self.latest_stl_path().ok_or("No model to export")?;
        fs::copy(&src, dest).map_err(|e| format!("Export failed: {e}"))?;
        Ok(())
    }

    pub fn reset(&mut self) {
        if let Ok(entries) = fs::read_dir(&self.temp_dir) {
            for entry in entries.flatten() {
                let _ = fs::remove_file(entry.path());
            }
        }
        self.messages.clear();
        self.current_metadata = None;
        self.current_code = None;
        self.current_engine = None;
        self.iteration = 0;
        self.undo_snapshot = None;
        self.state = SessionState::Idle;
    }

    #[allow(dead_code)]
    pub fn exchange_count(&self) -> usize { self.messages.len() / 2 }

    pub fn temp_dir(&self) -> &Path { &self.temp_dir }

    pub fn iteration(&self) -> u32 { self.iteration }
}

/// Data that gets serialized to session.json (legacy format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub name: String,
    pub created: String,
    pub modified: String,
    pub iteration_count: u32,
    pub claude_session_id: Option<String>,
    pub current_iteration: u32,
    pub engine: Option<String>,
    pub conversation: Vec<Message>,
}

impl LegacySession {
    /// Save current session state to a directory.
    /// Copies all iteration files (code, STL, metadata) and writes session.json.
    pub fn save_to(&self, dir: &Path, name: &str, claude_session_id: Option<&str>) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("Failed to create session dir: {e}"))?;

        // Copy iteration files from temp_dir to session dir
        for i in 1..=self.iteration {
            for ext in &["py", "scad", "stl", "stl.json"] {
                let src = self.temp_dir.join(format!("iter_{i:03}.{ext}"));
                if src.exists() {
                    let dest = dir.join(format!("iter_{i:03}.{ext}"));
                    let _ = std::fs::copy(&src, &dest);
                }
            }
            // Also copy .json metadata sidecar
            let meta_src = self.temp_dir.join(format!("iter_{i:03}.json"));
            if meta_src.exists() {
                let meta_dest = dir.join(format!("iter_{i:03}.json"));
                let _ = std::fs::copy(&meta_src, &meta_dest);
            }
        }

        // Write session.json
        let data = SessionData {
            name: name.to_string(),
            created: chrono::Utc::now().to_rfc3339(),
            modified: chrono::Utc::now().to_rfc3339(),
            iteration_count: self.iteration,
            claude_session_id: claude_session_id.map(|s| s.to_string()),
            current_iteration: self.iteration,
            engine: self.current_engine.map(|e| e.as_str().to_string()),
            conversation: self.messages.clone(),
        };
        let json = serde_json::to_string_pretty(&data)
            .map_err(|e| format!("Failed to serialize session: {e}"))?;
        std::fs::write(dir.join("session.json"), json)
            .map_err(|e| format!("Failed to write session.json: {e}"))?;

        Ok(())
    }

    /// Load session state from a directory.
    pub fn load_from(dir: &Path, build_timeout: u64, python_path: String) -> Result<Self, String> {
        let json_path = dir.join("session.json");
        let json = std::fs::read_to_string(&json_path)
            .map_err(|e| format!("Failed to read session.json: {e}"))?;
        let data: SessionData = serde_json::from_str(&json)
            .map_err(|e| format!("Failed to parse session.json: {e}"))?;

        let mut session = LegacySession::new(build_timeout, python_path);
        session.messages = data.conversation;
        session.iteration = data.current_iteration;

        // Copy iteration files into temp dir for active use
        for i in 1..=data.iteration_count {
            for ext in &["py", "scad", "stl", "stl.json", "json"] {
                let src = dir.join(format!("iter_{i:03}.{ext}"));
                if src.exists() {
                    let dest = session.temp_dir.join(format!("iter_{i:03}.{ext}"));
                    let _ = std::fs::copy(&src, &dest);
                }
            }
        }

        // Load latest metadata
        let meta_path = session.temp_dir.join(format!("iter_{:03}.stl.json", data.current_iteration));
        if meta_path.exists() {
            if let Ok(meta_json) = std::fs::read_to_string(&meta_path) {
                if let Ok(meta) = serde_json::from_str::<crate::python::ModelMetadata>(&meta_json) {
                    session.current_metadata = Some(meta);
                }
            }
        }

        // Load latest code
        let code_path = session.temp_dir.join(format!("iter_{:03}.py", data.current_iteration));
        if code_path.exists() {
            if let Ok(code) = std::fs::read_to_string(&code_path) {
                session.current_code = Some(code);
                session.current_engine = Some(crate::python::Engine::CadQuery);
            }
        } else {
            let scad_path = session.temp_dir.join(format!("iter_{:03}.scad", data.current_iteration));
            if scad_path.exists() {
                if let Ok(code) = std::fs::read_to_string(&scad_path) {
                    session.current_code = Some(code);
                    session.current_engine = Some(crate::python::Engine::OpenSCAD);
                }
            }
        }

        if session.current_metadata.is_some() {
            session.state = SessionState::Reviewing;
        }

        session.update_symlink();
        Ok(session)
    }
}

impl Drop for LegacySession {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.temp_dir);
    }
}

// Compatibility alias — will be removed when main.rs migrates to PhaseSession
pub type Session = LegacySession;

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

    /// Atomic copy: write to working.stl.tmp then rename to working.stl.
    pub fn update_working_stl(&self, src: &Path) -> Result<(), String> {
        let tmp = self.base_dir.join("working.stl.tmp");
        let dest = self.base_dir.join("working.stl");
        fs::copy(src, &tmp)
            .map_err(|e| format!("Failed to copy STL to tmp: {e}"))?;
        fs::rename(&tmp, &dest)
            .map_err(|e| format!("Failed to rename working.stl.tmp: {e}"))?;
        Ok(())
    }

    /// Atomic copy: write to working.step.tmp then rename to working.step.
    pub fn update_working_step(&self, src: &Path) -> Result<(), String> {
        let tmp = self.base_dir.join("working.step.tmp");
        let dest = self.base_dir.join("working.step");
        fs::copy(src, &tmp)
            .map_err(|e| format!("Failed to copy STEP to tmp: {e}"))?;
        fs::rename(&tmp, &dest)
            .map_err(|e| format!("Failed to rename working.step.tmp: {e}"))?;
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
    /// Detects legacy vs new format; returns an error for legacy sessions.
    pub fn load(dir: &Path, build_timeout: u64, python_path: String) -> Result<Self, String> {
        let json_path = dir.join("session.json");
        let json_str = fs::read_to_string(&json_path)
            .map_err(|e| format!("Failed to read session.json: {e}"))?;

        if crate::storage::session::is_legacy_session_json(&json_str) {
            return Err("Legacy session format detected; use LegacySession::load_from instead".to_string());
        }

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

    // ---- LegacySession tests ----

    #[test]
    fn test_new_session() {
        let s = LegacySession::new(60, "python".to_string());
        assert_eq!(s.state, SessionState::Idle);
        assert!(s.messages.is_empty());
        assert!(s.temp_dir.exists());
    }

    #[test]
    fn test_add_messages() {
        let mut s = LegacySession::new(60, "python".to_string());
        s.add_user_message("make a box");
        s.add_assistant_message("here's a box");
        assert_eq!(s.messages.len(), 2);
        assert_eq!(s.exchange_count(), 1);
    }

    #[test]
    fn test_reset() {
        let mut s = LegacySession::new(60, "python".to_string());
        s.add_user_message("make a box");
        s.reset();
        assert!(s.messages.is_empty());
        assert_eq!(s.state, SessionState::Idle);
    }

    #[test]
    fn test_save_to_and_session_data() {
        let tmp = tempfile::TempDir::new().unwrap();
        let session_dir = tmp.path().join("test_session");

        let mut s = LegacySession::new(60, "python".to_string());
        s.add_user_message("make a box");
        s.add_assistant_message("here's a box");

        s.save_to(&session_dir, "test_session", Some("sid-123")).unwrap();

        assert!(session_dir.join("session.json").exists());

        let json = std::fs::read_to_string(session_dir.join("session.json")).unwrap();
        let data: SessionData = serde_json::from_str(&json).unwrap();
        assert_eq!(data.name, "test_session");
        assert_eq!(data.claude_session_id, Some("sid-123".to_string()));
        assert_eq!(data.conversation.len(), 2);
    }

    // ---- PhaseSession tests ----

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
        assert!(tmp.path().join("sess/working.stl").exists());
        assert_eq!(std::fs::read(tmp.path().join("sess/working.stl")).unwrap(), b"dummy stl");
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
        assert!(tmp.path().join("sess/working.step").exists());
        assert_eq!(std::fs::read(tmp.path().join("sess/working.step")).unwrap(), b"dummy step");
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
    fn test_load_rejects_legacy_format() {
        let tmp = tempfile::TempDir::new().unwrap();
        let session_dir = tmp.path().join("legacy_sess");
        std::fs::create_dir_all(&session_dir).unwrap();

        // Write a legacy-format session.json
        let legacy_json = r#"{
            "name": "old",
            "created": "2026-03-15T00:00:00Z",
            "modified": "2026-03-15T00:00:00Z",
            "iteration_count": 3,
            "claude_session_id": null,
            "current_iteration": 3,
            "engine": null,
            "conversation": []
        }"#;
        std::fs::write(session_dir.join("session.json"), legacy_json).unwrap();

        let result = PhaseSession::load(&session_dir, 60, "python".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Legacy"));
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
