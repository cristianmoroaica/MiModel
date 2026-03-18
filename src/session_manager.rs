//! SessionManager — single owner of conversation state and session lifecycle.

use crate::model_session::PhaseSession;
use crate::phase::Phase;
use crate::python::{self, BuildResult, Engine};
use crate::storage::session::ConversationEntry;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct SessionManager {
    pub active_dir: Option<PathBuf>,
    pub active_name: Option<String>,
    pub project_idx: Option<usize>,
    pub phase_session: Option<PhaseSession>,

    // Build state (replaces LegacySession's build capability)
    temp_dir: PathBuf,
    build_timeout: Duration,
    python_path: String,
    iteration: u32,
    pub current_metadata: Option<crate::python::ModelMetadata>,
    pub current_code: Option<String>,
    pub current_engine: Option<Engine>,
    undo_snapshot: Option<BuildSnapshot>,
}

#[derive(Debug, Clone)]
struct BuildSnapshot {
    iteration: u32,
    metadata: Option<crate::python::ModelMetadata>,
    code: Option<String>,
    engine: Option<Engine>,
}

/// Empty slice constant for returning from conversations() when no data.
static EMPTY_ENTRIES: &[ConversationEntry] = &[];

impl SessionManager {
    pub fn new(build_timeout: u64, python_path: String) -> Self {
        #[allow(deprecated)]
        let temp_dir = tempfile::tempdir()
            .expect("Failed to create temp directory")
            .into_path();

        SessionManager {
            active_dir: None,
            active_name: None,
            project_idx: None,
            phase_session: None,
            temp_dir,
            build_timeout: Duration::from_secs(build_timeout),
            python_path,
            iteration: 0,
            current_metadata: None,
            current_code: None,
            current_engine: None,
            undo_snapshot: None,
        }
    }

    /// Returns conversation entries for a given phase from the phase session,
    /// or an empty slice if no session or no entries for that phase.
    pub fn conversations(&self, phase: Phase) -> &[ConversationEntry] {
        match &self.phase_session {
            Some(ps) => {
                let key = phase.label();
                ps.conversations.get(key).map(|v| v.as_slice()).unwrap_or(EMPTY_ENTRIES)
            }
            None => EMPTY_ENTRIES,
        }
    }

    /// Append a message to the phase session's conversation for the given phase.
    /// Does NOT auto-save.
    pub fn add_message(&mut self, phase: Phase, role: &str, content: &str) {
        if let Some(ref mut ps) = self.phase_session {
            let key = phase.label().to_string();
            ps.conversations
                .entry(key)
                .or_default()
                .push(ConversationEntry {
                    role: role.to_string(),
                    content: content.to_string(),
                });
        }
    }

    /// Sync phase into the phase session and save to disk.
    pub fn save(&mut self, phase: Phase) {
        if let Some(ref mut ps) = self.phase_session {
            ps.phase = phase;
            if let Err(e) = ps.save() {
                eprintln!("Warning: auto-save failed: {e}");
            }
        }
    }

    /// Create a new PhaseSession at the given directory.
    pub fn create(&mut self, dir: PathBuf, build_timeout: u64, python_path: String) {
        self.phase_session = Some(PhaseSession::new(dir.clone(), build_timeout, python_path));
        self.active_dir = Some(dir);
    }

    /// Load an existing PhaseSession from disk.
    pub fn load(&mut self, dir: &Path, build_timeout: u64, python_path: String) -> Result<(), String> {
        let ps = PhaseSession::load(dir, build_timeout, python_path)?;
        self.phase_session = Some(ps);
        self.active_dir = Some(dir.to_path_buf());
        Ok(())
    }

    /// Clear all state — reset to fresh.
    pub fn reset(&mut self) {
        // Clean temp dir
        if let Ok(entries) = fs::read_dir(&self.temp_dir) {
            for entry in entries.flatten() {
                let _ = fs::remove_file(entry.path());
            }
        }
        self.active_dir = None;
        self.active_name = None;
        self.project_idx = None;
        self.phase_session = None;
        self.iteration = 0;
        self.current_metadata = None;
        self.current_code = None;
        self.current_engine = None;
        self.undo_snapshot = None;
    }

    /// Returns true if a session is currently active (has a directory).
    pub fn is_active(&self) -> bool {
        self.active_dir.is_some()
    }

    // -- Build methods (migrated from LegacySession) --

    pub fn temp_dir(&self) -> &Path {
        &self.temp_dir
    }

    pub fn iteration(&self) -> u32 {
        self.iteration
    }

    pub fn build(&mut self, code: &str, engine: Engine) -> BuildResult {
        // Snapshot before build
        self.undo_snapshot = Some(BuildSnapshot {
            iteration: self.iteration,
            metadata: self.current_metadata.clone(),
            code: self.current_code.clone(),
            engine: self.current_engine,
        });
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
                self.update_symlink();
            }
            BuildResult::Timeout | BuildResult::BuildError(_) | BuildResult::SyntaxError(_) => {}
        }

        result
    }

    pub fn undo(&mut self) -> bool {
        if let Some(snap) = self.undo_snapshot.take() {
            self.iteration = snap.iteration;
            self.current_metadata = snap.metadata;
            self.current_code = snap.code;
            self.current_engine = snap.engine;
            self.update_symlink();
            true
        } else {
            false
        }
    }

    fn update_symlink(&self) {
        let symlink = self.temp_dir.join("current.stl");
        let _ = fs::remove_file(&symlink);
        let target = self.temp_dir.join(format!("iter_{:03}.stl", self.iteration));
        if target.exists() {
            #[cfg(unix)]
            {
                let _ = std::os::unix::fs::symlink(&target, &symlink);
            }
        }
    }

    pub fn latest_stl_path(&self) -> Option<PathBuf> {
        // First check _buffer.stl in session dir (MCP builds write here)
        if let Some(ref dir) = self.active_dir {
            let buffer = dir.join("_buffer.stl");
            if buffer.exists() {
                return Some(buffer);
            }
        }
        // Fallback: legacy iter_NNN.stl in temp dir
        let p = self.temp_dir.join(format!("iter_{:03}.stl", self.iteration));
        if p.exists() { Some(p) } else { None }
    }

    pub fn export(&self, dest: &Path) -> Result<(), String> {
        let src = self.latest_stl_path().ok_or("No model to export")?;
        fs::copy(&src, dest).map_err(|e| format!("Export failed: {e}"))?;
        Ok(())
    }
}

impl Drop for SessionManager {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.temp_dir);
    }
}
