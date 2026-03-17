//! Session directory CRUD and serialization.

use crate::component::ComponentState;
use crate::phase::Phase;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Per-scope Claude session ID storage
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClaudeSessionMap {
    pub spec: Option<String>,
    pub decompose: Option<String>,
    #[serde(default)]
    pub components: HashMap<String, String>, // component_id -> session_id
}

/// New session.json format for phase-machine sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseSessionData {
    pub name: String,
    pub created: String,
    pub phase: Phase,
    pub current_component: Option<String>,
    pub claude_sessions: ClaudeSessionMap,
    pub conversations: HashMap<String, Vec<ConversationEntry>>,
    pub component_states: Vec<ComponentState>,
}

/// A single conversation message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationEntry {
    pub role: String,
    pub content: String,
}

/// Create a new session directory inside a project.
pub fn create_session(project_path: &Path, name: &str) -> Result<PathBuf, String> {
    let path = project_path.join(name);
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("Failed to create session dir: {e}"))?;
    Ok(path)
}

/// Delete a session directory.
pub fn delete_session(session_path: &Path) -> Result<(), String> {
    if session_path.exists() {
        std::fs::remove_dir_all(session_path)
            .map_err(|e| format!("Failed to delete session: {e}"))?;
    }
    Ok(())
}

/// Rename a session directory.
pub fn rename_session(session_path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let new_path = session_path
        .parent()
        .ok_or("Invalid session path")?
        .join(new_name);
    std::fs::rename(session_path, &new_path)
        .map_err(|e| format!("Failed to rename session: {e}"))?;
    Ok(new_path)
}

/// Return the status of a session directory (reads PhaseSessionData).
pub fn session_status(session_path: &Path) -> SessionStatus {
    let json_path = session_path.join("session.json");
    if !json_path.exists() {
        return SessionStatus::Empty;
    }
    match std::fs::read_to_string(&json_path) {
        Ok(json) => match serde_json::from_str::<PhaseSessionData>(&json) {
            Ok(data) => SessionStatus::Ok {
                phase: data.phase.label().to_string(),
                created: data.created,
            },
            Err(_) => SessionStatus::Corrupted,
        },
        Err(_) => SessionStatus::Corrupted,
    }
}

#[derive(Debug)]
pub enum SessionStatus {
    Ok { phase: String, created: String },
    Corrupted,
    Empty,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_create_session() {
        let tmp = TempDir::new().unwrap();
        let project_path = tmp.path();

        let session_path = create_session(project_path, "my-session").unwrap();
        assert!(session_path.exists());
    }

    #[test]
    fn test_session_status_empty() {
        let tmp = TempDir::new().unwrap();
        let session_path = create_session(tmp.path(), "empty").unwrap();
        assert!(matches!(session_status(&session_path), SessionStatus::Empty));
    }

    #[test]
    fn test_delete_session() {
        let tmp = TempDir::new().unwrap();
        let session_path = create_session(tmp.path(), "to-delete").unwrap();
        delete_session(&session_path).unwrap();
        assert!(!session_path.exists());
    }

    #[test]
    fn test_rename_session() {
        let tmp = TempDir::new().unwrap();
        let session_path = create_session(tmp.path(), "old-name").unwrap();
        let new_path = rename_session(&session_path, "new-name").unwrap();
        assert!(new_path.exists());
        assert!(!session_path.exists());
    }

    #[test]
    fn test_serialize_phase_session_data() {
        let data = PhaseSessionData {
            name: "test_session".into(),
            created: "2026-03-16T12:00:00Z".into(),
            phase: Phase::Spec,
            current_component: None,
            claude_sessions: ClaudeSessionMap::default(),
            conversations: std::collections::HashMap::new(),
            component_states: vec![],
        };
        let json = serde_json::to_string_pretty(&data).unwrap();
        assert!(json.contains("\"phase\""));
        assert!(json.contains("Spec"));
    }

    #[test]
    fn test_deserialize_phase_session_data() {
        let json = r#"{
            "name": "test",
            "created": "2026-03-16T12:00:00Z",
            "phase": "Component",
            "current_component": "case_body",
            "claude_sessions": { "spec": "sid_123", "components": { "case_body": "sid_456" } },
            "conversations": {},
            "component_states": []
        }"#;
        let data: PhaseSessionData = serde_json::from_str(json).unwrap();
        assert_eq!(data.phase, Phase::Component);
        assert_eq!(data.current_component, Some("case_body".into()));
        assert_eq!(data.claude_sessions.spec, Some("sid_123".into()));
    }
}
