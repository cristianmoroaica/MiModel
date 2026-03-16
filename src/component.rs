use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComponentStatus {
    Pending,
    Building,
    Reviewing,
    Approved,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentState {
    pub id: String,
    pub name: String,
    pub status: ComponentStatus,
    pub iteration: u32,
    pub current_code: Option<String>,
    pub error_count: u32,
    #[serde(skip)]
    dir: Option<PathBuf>,
    #[serde(skip)]
    history: Vec<String>, // previous code versions for undo
}

impl ComponentState {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            status: ComponentStatus::Pending,
            iteration: 0,
            current_code: None,
            error_count: 0,
            dir: None,
            history: Vec::new(),
        }
    }

    pub fn set_dir(&mut self, dir: PathBuf) {
        self.dir = Some(dir);
    }

    pub fn dir(&self) -> Option<&PathBuf> {
        self.dir.as_ref()
    }

    /// Save `code` as the next iteration, push the previous code onto the undo
    /// history, and write `history/iter_NNN.py` to disk.
    pub fn record_iteration(&mut self, code: impl Into<String>) -> Result<(), String> {
        let code = code.into();

        // Push the current code onto undo history before overwriting it.
        if let Some(prev) = self.current_code.take() {
            self.history.push(prev);
        }

        self.current_code = Some(code.clone());
        self.iteration += 1;

        if let Some(dir) = &self.dir {
            let hist_dir = dir.join("history");
            std::fs::create_dir_all(&hist_dir)
                .map_err(|e| format!("create history dir: {e}"))?;
            let filename = hist_dir.join(format!("iter_{:03}.py", self.iteration));
            std::fs::write(&filename, &code)
                .map_err(|e| format!("write {}: {e}", filename.display()))?;
        }

        Ok(())
    }

    /// Write the current code as `<id>.py` in the component directory and mark
    /// the component as Approved.
    pub fn approve(&mut self) -> Result<(), String> {
        let code = self
            .current_code
            .as_ref()
            .ok_or_else(|| "no current code to approve".to_string())?;

        let dir = self
            .dir
            .as_ref()
            .ok_or_else(|| "no directory set".to_string())?;

        let out = dir.join(format!("{}.py", self.id));
        std::fs::write(&out, code)
            .map_err(|e| format!("write {}: {e}", out.display()))?;

        self.status = ComponentStatus::Approved;
        Ok(())
    }

    /// Undo the last iteration: restore the previous code version and decrement
    /// the iteration counter.
    pub fn undo(&mut self) {
        if let Some(prev) = self.history.pop() {
            self.current_code = Some(prev);
            if self.iteration > 0 {
                self.iteration -= 1;
            }
        }
    }

    /// Increment the error counter.
    pub fn record_error(&mut self) {
        self.error_count += 1;
    }

    /// Return `true` (and set status to Error) when the error count reaches 2.
    pub fn two_strikes(&mut self) -> bool {
        if self.error_count >= 2 {
            self.status = ComponentStatus::Error;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_initial_status() {
        let cs = ComponentState::new("case_body", "Case Body");
        assert_eq!(cs.status, ComponentStatus::Pending);
        assert_eq!(cs.iteration, 0);
    }

    #[test]
    fn test_record_iteration() {
        let tmp = TempDir::new().unwrap();
        let mut cs = ComponentState::new("case_body", "Case Body");
        cs.set_dir(tmp.path().to_path_buf());
        let code = "import cadquery as cq\nresult = cq.Workplane('XY').box(10,10,10)";
        cs.record_iteration(code).unwrap();
        assert_eq!(cs.iteration, 1);
        assert!(tmp.path().join("history/iter_001.py").exists());
    }

    #[test]
    fn test_approve() {
        let tmp = TempDir::new().unwrap();
        let mut cs = ComponentState::new("test", "Test");
        cs.set_dir(tmp.path().to_path_buf());
        cs.current_code = Some("code".into());
        cs.approve().unwrap();
        assert_eq!(cs.status, ComponentStatus::Approved);
        assert!(tmp.path().join("test.py").exists());
    }

    #[test]
    fn test_undo() {
        let tmp = TempDir::new().unwrap();
        let mut cs = ComponentState::new("test", "Test");
        cs.set_dir(tmp.path().to_path_buf());
        cs.record_iteration("code_v1").unwrap();
        cs.record_iteration("code_v2").unwrap();
        assert_eq!(cs.iteration, 2);
        cs.undo();
        assert_eq!(cs.iteration, 1);
        assert_eq!(cs.current_code.as_deref(), Some("code_v1"));
    }

    #[test]
    fn test_two_strikes() {
        let mut cs = ComponentState::new("test", "Test");
        assert!(!cs.two_strikes());
        cs.record_error();
        assert!(!cs.two_strikes());
        cs.record_error();
        assert!(cs.two_strikes());
        assert_eq!(cs.status, ComponentStatus::Error);
    }
}
