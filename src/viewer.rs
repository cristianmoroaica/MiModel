//! External 3D viewer launcher (f3d with --watch on a stable _buffer.stl).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

pub struct Viewer {
    preferred: String,
    child: Option<Child>,
    /// Stable path that f3d watches. Overwritten on each build.
    working_stl: Option<PathBuf>,
    /// Session directory — when set, working files are written here.
    session_dir: Option<PathBuf>,
}

impl Viewer {
    pub fn new(preferred: &str) -> Self {
        Self {
            preferred: preferred.to_string(),
            child: None,
            working_stl: None,
            session_dir: None,
        }
    }

    /// Set the working STL directory. Creates `_buffer.stl` path inside it.
    pub fn set_working_dir(&mut self, dir: &Path) {
        self.working_stl = Some(dir.join("_buffer.stl"));
    }

    /// Set the session directory. When set, `update_working_stl` writes to
    /// `session_dir/_buffer.stl` instead of `working_dir/_buffer.stl`.
    pub fn set_session_dir(&mut self, dir: &Path) {
        self.session_dir = Some(dir.to_path_buf());
    }

    /// Get the stable _buffer.stl path.
    pub fn working_stl_path(&self) -> Option<&Path> {
        self.working_stl.as_deref()
    }

    /// Update the _buffer.stl with new content from the latest build.
    /// Uses write-to-temp + rename so f3d's file watcher detects the inode change.
    /// When a session directory is set, writes to `session_dir/_buffer.stl`.
    pub fn update_working_stl(&self, source_stl: &Path) -> Result<(), String> {
        let working = if let Some(ref sdir) = self.session_dir {
            sdir.join("_buffer.stl")
        } else if let Some(ref w) = self.working_stl {
            w.clone()
        } else {
            return Err("No working directory set".to_string());
        };
        let tmp = working.with_extension("stl.tmp");
        std::fs::copy(source_stl, &tmp)
            .map_err(|e| format!("Failed to copy to temp: {e}"))?;
        std::fs::rename(&tmp, &working)
            .map_err(|e| format!("Failed to update _buffer.stl: {e}"))?;
        Ok(())
    }

    /// Update `_buffer.step` with new content using the same atomic copy pattern.
    /// Writes to `working_dir/_buffer.step` (or `session_dir/_buffer.step` if set).
    pub fn update_working_step(&self, source: &Path) -> Result<(), String> {
        let working = if let Some(ref sdir) = self.session_dir {
            sdir.join("_buffer.step")
        } else if let Some(ref w) = self.working_stl {
            w.with_file_name("_buffer.step")
        } else {
            return Err("No working directory set".to_string());
        };
        let tmp = working.with_extension("step.tmp");
        std::fs::copy(source, &tmp)
            .map_err(|e| format!("Failed to copy to temp: {e}"))?;
        std::fs::rename(&tmp, &working)
            .map_err(|e| format!("Failed to update _buffer.step: {e}"))?;
        Ok(())
    }

    /// Launch f3d pointing at _buffer.stl. Only launches once — subsequent
    /// calls return Ok(false) if already running.
    pub fn show(&mut self) -> Result<bool, String> {
        // Check if already running
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(None) => return Ok(false), // still running
                _ => { self.child = None; }
            }
        }

        let stl_path = self.working_stl.as_ref()
            .ok_or("No working STL set. Build a model first.")?;

        if !stl_path.exists() {
            return Err("No model built yet.".to_string());
        }

        let (cmd, args) = self.resolve_viewer(stl_path)?;
        let child = Command::new(&cmd)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to launch {cmd}: {e}"))?;

        self.child = Some(child);
        Ok(true)
    }

    /// Check if the viewer is currently running.
    pub fn is_running(&mut self) -> bool {
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(None) => true,
                _ => { self.child = None; false }
            }
        } else {
            false
        }
    }

    fn resolve_viewer(&self, stl_path: &Path) -> Result<(String, Vec<String>), String> {
        let path_str = stl_path.to_string_lossy().to_string();

        if which(&self.preferred) {
            let args = if self.preferred == "f3d" {
                vec!["--watch".to_string(), path_str]
            } else {
                vec![path_str]
            };
            return Ok((self.preferred.clone(), args));
        }

        for viewer in ["f3d", "meshlab", "xdg-open"] {
            if viewer == self.preferred { continue; }
            if which(viewer) {
                let args = if viewer == "f3d" {
                    vec!["--watch".to_string(), path_str]
                } else {
                    vec![path_str]
                };
                return Ok((viewer.to_string(), args));
            }
        }

        Err("No 3D viewer found. Install f3d: pacman -S f3d".to_string())
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn which(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewer_new() {
        let v = Viewer::new("f3d");
        assert_eq!(v.preferred, "f3d");
        assert!(v.child.is_none());
        assert!(v.working_stl.is_none());
    }

    #[test]
    fn test_update_working_step() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut v = Viewer::new("f3d");
        v.set_working_dir(tmp.path());
        let src = tmp.path().join("test.step");
        std::fs::write(&src, b"step data").unwrap();
        v.update_working_step(&src).unwrap();
        assert!(tmp.path().join("_buffer.step").exists());
    }
}
