//! Session directory CRUD and serialization.

use crate::model_session::SessionData;
use std::path::{Path, PathBuf};

/// Create a new session directory inside a project.
pub fn create_session(project_path: &Path, name: &str) -> Result<PathBuf, String> {
    let path = project_path.join(name);
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("Failed to create session dir: {e}"))?;
    Ok(path)
}

/// Load session metadata from a session directory.
pub fn load_session_data(session_path: &Path) -> Result<SessionData, String> {
    let json_path = session_path.join("session.json");
    let json = std::fs::read_to_string(&json_path)
        .map_err(|e| format!("Failed to read session.json: {e}"))?;
    serde_json::from_str(&json)
        .map_err(|e| format!("Corrupted session.json: {e}"))
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

/// Return the status of a session directory.
pub fn session_status(session_path: &Path) -> SessionStatus {
    let json_path = session_path.join("session.json");
    if !json_path.exists() {
        return SessionStatus::Empty;
    }
    match std::fs::read_to_string(&json_path) {
        Ok(json) => match serde_json::from_str::<SessionData>(&json) {
            Ok(data) => SessionStatus::Ok {
                iteration_count: data.iteration_count,
                modified: data.modified,
            },
            Err(_) => SessionStatus::Corrupted,
        },
        Err(_) => SessionStatus::Corrupted,
    }
}

#[derive(Debug)]
pub enum SessionStatus {
    Ok { iteration_count: u32, modified: String },
    Corrupted,
    Empty,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_session::Session;
    use tempfile::TempDir;

    #[test]
    fn test_create_and_load_session() {
        let tmp = TempDir::new().unwrap();
        let project_path = tmp.path();

        let session_path = create_session(project_path, "my-session").unwrap();
        assert!(session_path.exists());

        // Write a session.json so we can load it
        let mut s = Session::new(60, "python".to_string());
        s.add_user_message("hello");
        s.save_to(&session_path, "my-session", None).unwrap();

        let data = load_session_data(&session_path).unwrap();
        assert_eq!(data.name, "my-session");
        assert_eq!(data.conversation.len(), 1);
    }

    #[test]
    fn test_session_status_ok() {
        let tmp = TempDir::new().unwrap();
        let session_path = create_session(tmp.path(), "s1").unwrap();

        let mut s = Session::new(60, "python".to_string());
        s.save_to(&session_path, "s1", None).unwrap();

        match session_status(&session_path) {
            SessionStatus::Ok { iteration_count, .. } => assert_eq!(iteration_count, 0),
            other => panic!("Expected Ok, got {other:?}"),
        }
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
}
