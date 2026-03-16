//! Project tree pane — collapsible project/session list.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use crate::storage::Project;

#[derive(Debug, Clone)]
pub struct TreeEntry {
    pub label: String,
    pub is_project: bool,
    pub is_expanded: bool,
    pub project_idx: usize,
    pub session_name: Option<String>,
}

pub struct ProjectTreePane {
    pub entries: Vec<TreeEntry>,
    pub state: ListState,
    pub active_project: Option<usize>,
    pub active_session: Option<String>,
}

impl ProjectTreePane {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            state: ListState::default(),
            active_project: None,
            active_session: None,
        }
    }

    /// Rebuild tree entries from project list.
    pub fn refresh(&mut self, projects: &[Project]) {
        self.entries.clear();
        for (i, project) in projects.iter().enumerate() {
            let is_expanded = self.active_project == Some(i);
            let marker = if is_expanded { "▼" } else { "▶" };
            self.entries.push(TreeEntry {
                label: format!("{marker} {}", project.meta.name),
                is_project: true,
                is_expanded,
                project_idx: i,
                session_name: None,
            });
            if is_expanded {
                if project.sessions.is_empty() {
                    self.entries.push(TreeEntry {
                        label: "  (no sessions)".to_string(),
                        is_project: false,
                        is_expanded: false,
                        project_idx: i,
                        session_name: None,
                    });
                } else {
                    for session_info in &project.sessions {
                        let active = self.active_session.as_deref() == Some(session_info.name.as_str());
                        let marker = if active { "◀" } else { "" };
                        let label = if session_info.is_legacy {
                            format!("  ├─ {} [legacy] {marker}", session_info.name)
                        } else {
                            format!("  ├─ {} {marker}", session_info.name)
                        };
                        self.entries.push(TreeEntry {
                            label,
                            is_project: false,
                            is_expanded: false,
                            project_idx: i,
                            session_name: Some(session_info.name.clone()),
                        });
                    }
                }
            }
        }
        // Add "New Project" at bottom
        self.entries.push(TreeEntry {
            label: "+ New Project".to_string(),
            is_project: true,
            is_expanded: false,
            project_idx: usize::MAX,
            session_name: None,
        });
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
            let style = if entry.is_project {
                Style::default().fg(Color::Blue).bold()
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(entry.label.clone()).style(style)
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(Color::DarkGray));

        frame.render_stateful_widget(list, area, &mut self.state);
    }
}
