//! Project tree pane — collapsible project/session/file browser.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use crate::storage::Project;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum TreeEntryKind {
    Project,
    Session,
    File,
    NewProject,
    Placeholder,
}

/// What should happen when a file entry is activated.
#[derive(Debug, Clone)]
pub enum FileAction {
    /// Open in f3d viewer
    OpenViewer(PathBuf),
    /// Load content into conversation panel
    LoadText(PathBuf),
    /// Attach as image/PDF
    AttachFile(PathBuf),
    /// No action (ignored file type)
    None,
}

#[derive(Debug, Clone)]
pub struct TreeEntry {
    pub label: String,
    pub kind: TreeEntryKind,
    pub depth: u16,
    pub is_expanded: bool,
    pub project_idx: usize,
    pub session_name: Option<String>,
    pub file_path: Option<PathBuf>,
}

pub struct ProjectTreePane {
    pub entries: Vec<TreeEntry>,
    pub state: ListState,
    pub active_project: Option<usize>,
    pub active_session: Option<String>,
    /// Which sessions have their file tree expanded
    pub expanded_sessions: std::collections::HashSet<String>,
}

impl ProjectTreePane {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            state: ListState::default(),
            active_project: None,
            active_session: None,
            expanded_sessions: std::collections::HashSet::new(),
        }
    }

    /// Rebuild tree entries from project list, including files.
    pub fn refresh(&mut self, projects: &[Project]) {
        self.entries.clear();
        for (i, project) in projects.iter().enumerate() {
            let is_expanded = self.active_project == Some(i);
            let marker = if is_expanded { "▼" } else { "▶" };
            self.entries.push(TreeEntry {
                label: format!("{marker} {}", project.meta.name),
                kind: TreeEntryKind::Project,
                depth: 0,
                is_expanded,
                project_idx: i,
                session_name: None,
                file_path: None,
            });
            if is_expanded {
                if project.sessions.is_empty() {
                    self.entries.push(TreeEntry {
                        label: "(no sessions)".to_string(),
                        kind: TreeEntryKind::Placeholder,
                        depth: 1,
                        is_expanded: false,
                        project_idx: i,
                        session_name: None,
                        file_path: None,
                    });
                } else {
                    for session_info in &project.sessions {
                        let active = self.active_session.as_deref() == Some(session_info.name.as_str());
                        let session_expanded = self.expanded_sessions.contains(&session_info.name);
                        let active_marker = if active { " ◀" } else { "" };
                        let expand_marker = if session_expanded { "▾" } else { "▸" };
                        self.entries.push(TreeEntry {
                            label: format!("{expand_marker} {}{active_marker}", session_info.name),
                            kind: TreeEntryKind::Session,
                            depth: 1,
                            is_expanded: session_expanded,
                            project_idx: i,
                            session_name: Some(session_info.name.clone()),
                            file_path: None,
                        });

                        // Show files if session is expanded
                        if session_expanded {
                            let session_dir = project.path.join(&session_info.name);
                            self.add_files_recursive(&session_dir, &session_dir, i, &session_info.name, 2);
                        }
                    }
                }

                // Show project-level files (not inside sessions)
                self.add_project_files(&project.path, i);
            }
        }
        // Add "New Project" at bottom
        self.entries.push(TreeEntry {
            label: "+ New Project".to_string(),
            kind: TreeEntryKind::NewProject,
            depth: 0,
            is_expanded: false,
            project_idx: usize::MAX,
            session_name: None,
            file_path: None,
        });
    }

    /// Add files from a session directory recursively.
    fn add_files_recursive(&mut self, dir: &Path, session_root: &Path, project_idx: usize, session_name: &str, depth: u16) {
        let mut entries: Vec<_> = match std::fs::read_dir(dir) {
            Ok(rd) => rd.flatten().collect(),
            Err(_) => return,
        };
        entries.sort_by_key(|e| e.file_name());

        // Directories first, then files
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        for entry in entries {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            // Skip hidden files and session.json
            if name.starts_with('.') || name == "session.json" {
                continue;
            }
            if path.is_dir() {
                dirs.push((name, path));
            } else {
                files.push((name, path));
            }
        }

        for (name, path) in dirs {
            let icon = file_icon_dir(&name);
            self.entries.push(TreeEntry {
                label: format!("{icon} {name}/"),
                kind: TreeEntryKind::File,
                depth,
                is_expanded: false,
                project_idx,
                session_name: Some(session_name.to_string()),
                file_path: Some(path.clone()),
            });
            // Always expand subdirectories (component dirs are small)
            self.add_files_recursive(&path, session_root, project_idx, session_name, depth + 1);
        }

        for (name, path) in files {
            let icon = file_icon(&name);
            self.entries.push(TreeEntry {
                label: format!("{icon} {name}"),
                kind: TreeEntryKind::File,
                depth,
                is_expanded: false,
                project_idx,
                session_name: Some(session_name.to_string()),
                file_path: Some(path),
            });
        }
    }

    /// Add project-level files (like .stl, .py files in project root, not inside sessions).
    fn add_project_files(&mut self, project_dir: &Path, project_idx: usize) {
        let entries: Vec<_> = match std::fs::read_dir(project_dir) {
            Ok(rd) => rd.flatten().collect(),
            Err(_) => return,
        };

        let mut files: Vec<(String, PathBuf)> = Vec::new();
        for entry in entries {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_file() && name != "project.json" && !name.starts_with('.') {
                files.push((name, path));
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));

        for (name, path) in files {
            let icon = file_icon(&name);
            self.entries.push(TreeEntry {
                label: format!("{icon} {name}"),
                kind: TreeEntryKind::File,
                depth: 1,
                is_expanded: false,
                project_idx,
                session_name: None,
                file_path: Some(path),
            });
        }
    }

    /// Toggle session file expansion.
    pub fn toggle_session_expand(&mut self, session_name: &str) {
        if self.expanded_sessions.contains(session_name) {
            self.expanded_sessions.remove(session_name);
        } else {
            self.expanded_sessions.insert(session_name.to_string());
        }
    }

    /// Determine what action to take for a file entry.
    pub fn file_action(path: &Path) -> FileAction {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        match ext {
            "stl" => FileAction::OpenViewer(path.to_path_buf()),
            "py" | "md" | "txt" | "toml" | "log" => FileAction::LoadText(path.to_path_buf()),
            "png" | "jpg" | "jpeg" | "pdf" => FileAction::AttachFile(path.to_path_buf()),
            // step, json — no action
            _ => FileAction::None,
        }
    }

    pub fn select_next(&mut self) {
        let i = self.state.selected().map(|i| (i + 1).min(self.entries.len().saturating_sub(1))).unwrap_or(0);
        self.state.select(Some(i));
    }

    pub fn select_prev(&mut self) {
        let i = self.state.selected().map(|i| i.saturating_sub(1)).unwrap_or(0);
        self.state.select(Some(i));
    }

    /// Get the currently selected entry.
    pub fn selected_entry(&self) -> Option<&TreeEntry> {
        self.state.selected().and_then(|i| self.entries.get(i))
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let border_color = if focused { Color::Cyan } else { Color::DarkGray };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(" Projects ");

        let items: Vec<ListItem> = self.entries.iter().map(|entry| {
            let indent = "  ".repeat(entry.depth as usize);
            let style = match entry.kind {
                TreeEntryKind::Project | TreeEntryKind::NewProject => {
                    Style::default().fg(Color::Blue).bold()
                }
                TreeEntryKind::Session => {
                    if self.active_session.as_deref() == entry.session_name.as_deref() {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default().fg(Color::White)
                    }
                }
                TreeEntryKind::File => {
                    Style::default().fg(Color::DarkGray)
                }
                TreeEntryKind::Placeholder => {
                    Style::default().fg(Color::DarkGray).italic()
                }
            };
            ListItem::new(format!("{indent}{}", entry.label)).style(style)
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(Color::DarkGray));

        frame.render_stateful_widget(list, area, &mut self.state);
    }
}

/// Get icon for a file based on extension.
fn file_icon(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("");
    match ext {
        "stl" => "◆",
        "step" | "stp" => "◇",
        "py" => "⚙",
        "toml" => "☰",
        "json" => "{ }",
        "md" | "txt" => "📄",
        "png" | "jpg" | "jpeg" => "🖼",
        "pdf" => "📋",
        _ => "·",
    }
}

/// Get icon for a directory based on name.
fn file_icon_dir(name: &str) -> &'static str {
    match name {
        "components" => "🔧",
        "assembly" => "🔩",
        "images" => "🖼",
        "references" => "📚",
        _ => "📁",
    }
}
