//! Session manager — conversation, iterations, undo, temp files.

use crate::claude::Message;
use crate::python::{self, BuildResult, Engine, ModelMetadata};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

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

pub struct Session {
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

impl Session {
    pub fn new(build_timeout: u64, python_path: String) -> Self {
        #[allow(deprecated)]
        let temp_dir = tempfile::tempdir()
            .expect("Failed to create temp directory")
            .into_path();

        Session {
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

/// Data that gets serialized to session.json.
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

impl Session {
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

        let mut session = Session::new(build_timeout, python_path);
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

impl Drop for Session {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.temp_dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_session() {
        let s = Session::new(60, "python".to_string());
        assert_eq!(s.state, SessionState::Idle);
        assert!(s.messages.is_empty());
        assert!(s.temp_dir.exists());
    }

    #[test]
    fn test_add_messages() {
        let mut s = Session::new(60, "python".to_string());
        s.add_user_message("make a box");
        s.add_assistant_message("here's a box");
        assert_eq!(s.messages.len(), 2);
        assert_eq!(s.exchange_count(), 1);
    }

    #[test]
    fn test_reset() {
        let mut s = Session::new(60, "python".to_string());
        s.add_user_message("make a box");
        s.reset();
        assert!(s.messages.is_empty());
        assert_eq!(s.state, SessionState::Idle);
    }

    #[test]
    fn test_save_to_and_session_data() {
        let tmp = tempfile::TempDir::new().unwrap();
        let session_dir = tmp.path().join("test_session");

        let mut s = Session::new(60, "python".to_string());
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
}
