//! External 3D viewer launcher (f3d with --watch on a stable working.stl).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

pub struct Viewer {
    preferred: String,
    child: Option<Child>,
    /// Stable path that f3d watches. Overwritten on each build.
    working_stl: Option<PathBuf>,
}

impl Viewer {
    pub fn new(preferred: &str) -> Self {
        Self {
            preferred: preferred.to_string(),
            child: None,
            working_stl: None,
        }
    }

    /// Set the working STL directory. Creates `working.stl` path inside it.
    pub fn set_working_dir(&mut self, dir: &Path) {
        self.working_stl = Some(dir.join("working.stl"));
    }

    /// Get the stable working.stl path.
    pub fn working_stl_path(&self) -> Option<&Path> {
        self.working_stl.as_deref()
    }

    /// Update the working.stl with new content from the latest build.
    /// f3d will auto-reload via --watch.
    pub fn update_working_stl(&self, source_stl: &Path) -> Result<(), String> {
        if let Some(ref working) = self.working_stl {
            std::fs::copy(source_stl, working)
                .map_err(|e| format!("Failed to update working.stl: {e}"))?;
            Ok(())
        } else {
            Err("No working directory set".to_string())
        }
    }

    /// Launch f3d pointing at working.stl. Only launches once — subsequent
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
}
