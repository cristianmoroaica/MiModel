//! Key, mouse, and paste event handling extracted from App.
//!
//! These methods remain on `impl App` but live in a separate file
//! to keep main.rs focused on struct definitions and the event loop.

use crossterm::event::{KeyCode, KeyModifiers};

use super::*;

impl<'a> App<'a> {
    pub(crate) fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        use KeyCode::*;

        // Global keybindings regardless of focus
        match (key.code, key.modifiers) {
            // Scroll conversation from any pane with PageUp/PageDown
            (PageUp, _) => {
                self.conversation.scroll_up(10);
                return;
            }
            (PageDown, _) => {
                self.conversation.scroll_down(10);
                return;
            }
            (Char('l'), KeyModifiers::CONTROL) => {
                self.layout_config.show_sidebar = !self.layout_config.show_sidebar;
                return;
            }
            (Char('r'), KeyModifiers::CONTROL) => {
                self.layout_config.show_model_panel = !self.layout_config.show_model_panel;
                return;
            }
            (Char('o'), KeyModifiers::CONTROL) => {
                match self.viewer.show() {
                    Ok(true) => self.conversation.add("system", "Opened in viewer (auto-reloads on each build)."),
                    Ok(false) => {} // already running, silent
                    Err(e) => self.conversation.add("system", &format!("Viewer: {e}")),
                }
                return;
            }
            (Char('z'), KeyModifiers::CONTROL) => {
                if self.claude.busy == BusyState::Idle {
                    if self.session.undo() {
                        self.conversation.add("system", "Undone last iteration.");
                        self.model_panel.clear();
                        if let Some(meta) = self.session.current_metadata.clone() {
                            self.model_panel.update(&meta, None, 0);
                            let model_summary = format!(
                                "{:.1} x {:.1} x {:.1} mm\nIterations: 0\nEngine: {}\nWatertight: {}",
                                meta.dimensions.x, meta.dimensions.y, meta.dimensions.z,
                                meta.engine.as_str(),
                                if meta.watertight { "yes" } else { "no" }
                            );
                            self.right_panel.set_model(&model_summary);
                        }
                    } else {
                        self.conversation.add("system", "Nothing to undo.");
                    }
                }
                return;
            }
            (Char('c'), KeyModifiers::CONTROL) => {
                if self.claude.busy != BusyState::Idle {
                    // Kill background process
                    self.claude.cancel();
                    self.conversation.add("system", "(cancelled)");
                    self.claude.busy = BusyState::Idle;
                    self.last_ctrl_c = None;
                } else {
                    // Double Ctrl+C to quit
                    let now = std::time::Instant::now();
                    if let Some(last) = self.last_ctrl_c {
                        if now.duration_since(last).as_millis() < 500 {
                            self.session.save(self.phase);
                            self.cleanup();
                            self.should_quit = true;
                        } else {
                            self.last_ctrl_c = Some(now);
                            self.conversation.add("system", "Press Ctrl+C again to quit.");
                        }
                    } else {
                        self.last_ctrl_c = Some(now);
                        self.conversation.add("system", "Press Ctrl+C again to quit.");
                    }
                }
                return;
            }
            (Char('x'), KeyModifiers::CONTROL) => {
                // Clear all pending attachments
                if !self.pending_images.is_empty() {
                    let count = self.pending_images.len();
                    self.pending_images.clear();
                    self.model_panel.pending_files.clear();
                    self.conversation.add("system", &format!("Cleared {count} pending file(s)."));
                }
                return;
            }
            (Char('w'), KeyModifiers::CONTROL) => {
                // Save current model as a named part
                if self.session.latest_stl_path().is_some() {
                    self.conversation.add("system", "Save as part: type a name and press Enter.");
                    self.save_part_pending = true;
                    self.focus = Focus::Input;
                } else {
                    self.conversation.add("system", "No model to save yet.");
                }
                return;
            }
            (Char('v'), KeyModifiers::CONTROL) => {
                // Paste clipboard image
                let img_dir = self.session.active_dir
                    .as_ref()
                    .map(|d| d.join("images"))
                    .unwrap_or_else(|| {
                        self.session.temp_dir().join("images")
                    });
                let _ = std::fs::create_dir_all(&img_dir);
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let dest = img_dir.join(format!("clipboard_{timestamp}.png"));
                match image::paste_clipboard_image(&dest) {
                    Ok(()) => {
                        let size_kb = std::fs::metadata(&dest).map(|m| m.len() / 1024).unwrap_or(0);
                        self.conversation.add("system", &format!("Attached image ({size_kb}KB)"));
                        self.model_panel.pending_files.push(dest.clone());
                        self.pending_images.push(dest);
                    }
                    Err(e) => {
                        self.conversation.add("system", &format!("Clipboard: {e}"));
                    }
                }
                return;
            }
            (Char('n'), KeyModifiers::CONTROL) => {
                self.new_session_pending = true;
                self.conversation.add("system", "Next prompt will start a new session.");
                return;
            }
            (Char('p'), KeyModifiers::CONTROL) => {
                self.new_project_pending = true;
                self.conversation.add("system", "Next prompt will create a new project.");
                return;
            }
            (Char('s'), KeyModifiers::CONTROL) => {
                // Export current STL
                if self.session.latest_stl_path().is_some() {
                    let export_dest = dirs::home_dir()
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join("model_export.stl");
                    match self.session.export(&export_dest) {
                        Ok(()) => {
                            self.conversation.add("system", &format!("Exported to {}", export_dest.display()));
                        }
                        Err(e) => {
                            self.conversation.add("system", &format!("Export failed: {e}"));
                        }
                    }
                } else {
                    self.conversation.add("system", "No model to export yet.");
                }
                return;
            }
            // Phase navigation: Alt+1 through Alt+3
            (Char('1'), KeyModifiers::ALT) => {
                self.try_switch_phase(Phase::Spec);
                return;
            }
            (Char('2'), KeyModifiers::ALT) => {
                self.try_switch_phase(Phase::Build);
                return;
            }
            (Char('3'), KeyModifiers::ALT) => {
                self.try_switch_phase(Phase::Refine);
                return;
            }
            // Component navigation: Ctrl+Left/Right (only in Component phase)
            (Left, KeyModifiers::CONTROL) => {
                if self.phase == Phase::Build {
                    self.component_list.select_prev();
                }
                return;
            }
            (Right, KeyModifiers::CONTROL) => {
                if self.phase == Phase::Build {
                    self.component_list.select_next();
                }
                return;
            }
            (Tab, _) => {
                // Auto-complete if focused on Input and typing a command
                if self.focus == Focus::Input {
                    let current = self.input_bar.text();
                    if current.starts_with("/ref") || current.starts_with("/import") {
                        self.try_autocomplete();
                        return;
                    }
                }
                self.focus = match self.focus {
                    Focus::Input => Focus::Conversation,
                    Focus::Conversation => Focus::RightPanel,
                    Focus::RightPanel => Focus::ProjectTree,
                    Focus::ProjectTree => Focus::Input,
                };
                return;
            }
            (Esc, _) => {
                self.focus = Focus::Input;
                return;
            }
            _ => {}
        }

        // Focus-specific handling
        match self.focus {
            Focus::Input => self.handle_input_key(key),
            Focus::ProjectTree => self.handle_tree_key(key),
            Focus::Conversation => self.handle_conversation_key(key),
            Focus::RightPanel => self.handle_right_panel_key(key),
        }
    }

    pub(crate) fn handle_input_key(&mut self, key: crossterm::event::KeyEvent) {
        // Convert key event to tui_textarea Input and handle
        let input = tui_textarea::Input::from(key);
        if let Some(text) = self.input_bar.handle_input(input) {
            self.submit_prompt(text);
        }
    }

    pub(crate) fn handle_tree_key(&mut self, key: crossterm::event::KeyEvent) {
        use KeyCode::*;
        use crate::tui::project_tree::{TreeEntryKind, FileAction};

        match key.code {
            Up | Char('k') => self.project_tree.select_prev(),
            Down | Char('j') => self.project_tree.select_next(),
            Right | Char('l') => {
                // Expand session file tree or project without loading
                if let Some(entry) = self.project_tree.selected_entry() {
                    match entry.kind {
                        TreeEntryKind::Session => {
                            if let Some(ref name) = entry.session_name {
                                let name = name.clone();
                                if !self.project_tree.expanded_sessions.contains(&name) {
                                    self.project_tree.toggle_session_expand(&name);
                                    let projects = self.projects.clone();
                                    self.project_tree.refresh(&projects);
                                }
                            }
                        }
                        TreeEntryKind::Project => {
                            let idx = entry.project_idx;
                            if self.project_tree.active_project != Some(idx) {
                                self.project_tree.active_project = Some(idx);
                                let projects = self.projects.clone();
                                self.project_tree.refresh(&projects);
                            }
                        }
                        _ => {}
                    }
                }
                return;
            }
            Left | Char('h') => {
                // Collapse session file tree or project
                if let Some(entry) = self.project_tree.selected_entry() {
                    match entry.kind {
                        TreeEntryKind::Session => {
                            if let Some(ref name) = entry.session_name {
                                let name = name.clone();
                                if self.project_tree.expanded_sessions.contains(&name) {
                                    self.project_tree.toggle_session_expand(&name);
                                    let projects = self.projects.clone();
                                    self.project_tree.refresh(&projects);
                                }
                            }
                        }
                        TreeEntryKind::Project => {
                            let idx = entry.project_idx;
                            if self.project_tree.active_project == Some(idx) {
                                self.project_tree.active_project = None;
                                let projects = self.projects.clone();
                                self.project_tree.refresh(&projects);
                            }
                        }
                        TreeEntryKind::File => {
                            // Navigate up to parent session/project
                            self.project_tree.select_prev();
                        }
                        _ => {}
                    }
                }
                return;
            }
            Char('e') => {
                // Rename — only switch to Input for rename/delete (needs text input)
                if let Some(entry) = self.project_tree.selected_entry() {
                    if entry.kind == TreeEntryKind::NewProject || entry.kind == TreeEntryKind::File {
                        return;
                    }
                    if let Some(ref session_name) = entry.session_name {
                        self.conversation.add("system", &format!("Rename session '{session_name}': type new name and press Enter."));
                        self.rename_pending = Some(RenameTarget::Session {
                            project_idx: entry.project_idx,
                            old_name: session_name.clone(),
                        });
                    } else {
                        let project_name = self.projects.get(entry.project_idx)
                            .map(|p| p.meta.name.clone())
                            .unwrap_or_default();
                        self.conversation.add("system", &format!("Rename project '{project_name}': type new name and press Enter."));
                        self.rename_pending = Some(RenameTarget::Project {
                            project_idx: entry.project_idx,
                            old_name: project_name,
                        });
                    }
                    self.focus = Focus::Input;
                }
                return;
            }
            Char('d') => {
                // Delete — switch to Input for confirmation
                if let Some(entry) = self.project_tree.selected_entry() {
                    if entry.kind == TreeEntryKind::NewProject || entry.kind == TreeEntryKind::File {
                        return;
                    }
                    if let Some(ref session_name) = entry.session_name {
                        self.conversation.add("system", &format!("Delete session '{session_name}'? Type 'yes' to confirm."));
                        self.delete_pending = Some(DeleteTarget::Session {
                            project_idx: entry.project_idx,
                            name: session_name.clone(),
                        });
                    } else {
                        let project_name = self.projects.get(entry.project_idx)
                            .map(|p| p.meta.name.clone())
                            .unwrap_or_default();
                        self.conversation.add("system", &format!("Delete project '{project_name}' and all its sessions? Type 'yes' to confirm."));
                        self.delete_pending = Some(DeleteTarget::Project {
                            project_idx: entry.project_idx,
                            name: project_name,
                        });
                    }
                    self.focus = Focus::Input;
                }
                return;
            }
            Enter => {
                if let Some(entry) = self.project_tree.selected_entry() {
                    let kind = entry.kind.clone();
                    let project_idx = entry.project_idx;
                    let session_name = entry.session_name.clone();
                    let file_path = entry.file_path.clone();

                    match kind {
                        TreeEntryKind::NewProject => {
                            self.new_project_pending = true;
                            self.conversation.add("system", "Type project name and press Enter.");
                            self.focus = Focus::Input;
                        }
                        TreeEntryKind::Project => {
                            // Toggle project expansion — stay in tree
                            let expanding = self.project_tree.active_project != Some(project_idx);
                            self.project_tree.active_project = if expanding {
                                Some(project_idx)
                            } else {
                                None
                            };
                            let projects = self.projects.clone();
                            self.project_tree.refresh(&projects);
                            if expanding {
                                self.open_project(project_idx);
                            }
                        }
                        TreeEntryKind::Session => {
                            if let Some(ref name) = session_name {
                                // Load session AND expand its file tree
                                if !self.project_tree.expanded_sessions.contains(name) {
                                    self.project_tree.toggle_session_expand(name);
                                }
                                self.load_session(project_idx, name.clone());
                                let projects = self.projects.clone();
                                self.project_tree.refresh(&projects);
                            }
                        }
                        TreeEntryKind::File => {
                            if let Some(ref path) = file_path {
                                match ProjectTreePane::file_action(path) {
                                    FileAction::OpenViewer(p) => {
                                        if let Err(e) = self.viewer.update_working_stl(&p) {
                                            self.conversation.add("system", &format!("Viewer error: {e}"));
                                        } else if !self.viewer.is_running() {
                                            match self.viewer.show() {
                                                Ok(true) => self.conversation.add("system", &format!("Opened in viewer: {}", p.file_name().unwrap_or_default().to_string_lossy())),
                                                Ok(false) => {} // already running
                                                Err(e) => self.conversation.add("system", &format!("Viewer: {e}")),
                                            }
                                        }
                                        // If viewer already running, f3d --watch picks up the change silently
                                    }
                                    FileAction::LoadText(p) => {
                                        match std::fs::read_to_string(&p) {
                                            Ok(content) => {
                                                let filename = p.file_name().unwrap_or_default().to_string_lossy();
                                                self.conversation.add("system", &format!("─── {} ───", filename));
                                                self.conversation.add("system", &content);
                                                self.conversation.scroll_to_bottom();
                                                // Switch to conversation to see the content
                                                self.focus = Focus::Conversation;
                                            }
                                            Err(e) => {
                                                self.conversation.add("system", &format!("Failed to read: {e}"));
                                            }
                                        }
                                    }
                                    FileAction::AttachFile(p) => {
                                        let kind = if crate::image::is_pdf(&p) { "PDF" } else { "image" };
                                        let size_kb = std::fs::metadata(&p).map(|m| m.len() / 1024).unwrap_or(0);
                                        self.conversation.add("system", &format!("Attached {kind} ({size_kb}KB): {}", p.display()));
                                        self.pending_images.push(p);
                                    }
                                    FileAction::None => {
                                        self.conversation.add("system", &format!("File type not supported for opening: {}", path.display()));
                                    }
                                }
                            }
                        }
                        TreeEntryKind::Placeholder => {}
                    }
                    // Note: focus stays in ProjectTree unless explicitly changed above
                }
            }
            _ => {}
        }
    }

    pub(crate) fn handle_conversation_key(&mut self, key: crossterm::event::KeyEvent) {
        use KeyCode::*;
        match key.code {
            Up | Char('k') => self.conversation.scroll_up(1),
            Down | Char('j') => self.conversation.scroll_down(1),
            Char('u') => self.conversation.scroll_up(10),
            Char('d') => self.conversation.scroll_down(10),
            _ => {}
        }
    }

    pub(crate) fn handle_right_panel_key(&mut self, key: crossterm::event::KeyEvent) {
        use KeyCode::*;
        match key.code {
            Left | Char('h') => self.right_panel.prev_tab(),
            Right | Char('l') => self.right_panel.next_tab(),
            Up | Char('k') => self.right_panel.scroll_up(1),
            Down | Char('j') => self.right_panel.scroll_down(1),
            _ => {}
        }
    }

    /// Auto-complete /ref and /import commands on Tab.
    fn try_autocomplete(&mut self) {
        let current = self.input_bar.text();

        // Handle /ref — with or without trailing space/query
        if current.starts_with("/ref") {
            let query = current.strip_prefix("/ref").unwrap_or("").trim_start();
            let query_lower = query.to_lowercase();
            let library = reference::load_library().unwrap_or_default();

            if library.is_empty() {
                self.conversation.add("system", "Reference library is empty. Use /ref <name> to research a component.");
                return;
            }

            // Match against both names and slugs
            let mut matches: Vec<String> = library.iter()
                .filter(|(comp, slug)| {
                    if query_lower.is_empty() { return true; }
                    comp.identity.name.to_lowercase().contains(&query_lower)
                        || slug.to_lowercase().contains(&query_lower)
                })
                .map(|(comp, _slug)| comp.identity.name.clone())
                .collect();
            matches.sort();
            matches.dedup();

            if matches.len() == 1 {
                self.input_bar.set_content(&format!("/ref {}", matches[0]));
            } else if !matches.is_empty() {
                let list = matches.iter().take(15)
                    .map(|m| format!("  {m}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                self.conversation.add("system", &format!("References:\n{list}"));
                // Add space if user typed just "/ref"
                if !current.contains(' ') {
                    self.input_bar.set_content("/ref ");
                }
            }
            return;
        }

        // Handle /import — with or without trailing space/path
        if current.starts_with("/import") {
            let raw = current.strip_prefix("/import").unwrap_or("");
            let partial = raw.trim_start();
            let partial = if partial.starts_with("~/") {
                dirs::home_dir()
                    .map(|h| h.join(&partial[2..]).to_string_lossy().to_string())
                    .unwrap_or_else(|| partial.to_string())
            } else if partial.is_empty() {
                // No path yet — start from home and add space to input
                if !current.contains(' ') {
                    self.input_bar.set_content("/import ~/");
                    return;
                }
                dirs::home_dir()
                    .map(|h| h.to_string_lossy().to_string())
                    .unwrap_or_else(|| "/".to_string())
            } else {
                partial.to_string()
            };

            let path = std::path::Path::new(&partial);
            let (dir, prefix) = if path.is_dir() {
                (path.to_path_buf(), String::new())
            } else {
                let dir = path.parent().unwrap_or(std::path::Path::new("/"));
                let prefix = path.file_name()
                    .map(|f| f.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                (dir.to_path_buf(), prefix)
            };

            if let Ok(entries) = std::fs::read_dir(&dir) {
                let mut matches: Vec<std::path::PathBuf> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        let name = p.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                        if !prefix.is_empty() && !name.starts_with(&prefix) {
                            return false;
                        }
                        // Show directories and .step/.stp files
                        if p.is_dir() { return true; }
                        matches!(p.extension().and_then(|e| e.to_str()),
                            Some("step") | Some("stp") | Some("STEP") | Some("STP"))
                    })
                    .collect();
                matches.sort();

                if matches.len() == 1 {
                    let m = &matches[0];
                    let display = m.to_string_lossy();
                    let suffix = if m.is_dir() { "/" } else { "" };
                    self.input_bar.set_content(&format!("/import {display}{suffix}"));
                } else if !matches.is_empty() {
                    let list = matches.iter().take(15)
                        .map(|m| {
                            let name = m.file_name().unwrap_or_default().to_string_lossy();
                            if m.is_dir() { format!("  {name}/") } else { format!("  {name}") }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.conversation.add("system", &format!("Files:\n{list}"));

                    // Find longest common prefix for partial completion
                    let strs: Vec<String> = matches.iter()
                        .map(|m| m.to_string_lossy().to_string())
                        .collect();
                    if let Some(lcp) = longest_common_prefix(&strs) {
                        if lcp.len() > partial.len() {
                            self.input_bar.set_content(&format!("/import {lcp}"));
                        }
                    }
                }
            }
        }
    }

    /// Handle pasted text (from bracketed paste / drag-and-drop).
    /// Detects file paths and attaches them; inserts remaining text into input.
    pub(crate) fn handle_paste(&mut self, pasted: String) {
        let mut remaining_text = Vec::new();

        for raw_line in pasted.lines() {
            let line = raw_line.trim();
            if line.is_empty() { continue; }

            // Try to resolve as a file path
            let path_str = if let Some(stripped) = line.strip_prefix("file://") {
                // Decode percent-encoded URI
                percent_decode(stripped)
            } else {
                line.to_string()
            };
            let expanded = image::expand_tilde(&path_str);
            let path = std::path::Path::new(&expanded);

            if path.exists() && (image::is_image(path) || image::is_pdf(path)) {
                let kind = if image::is_pdf(path) { "PDF" } else { "image" };
                let size_kb = std::fs::metadata(path).map(|m| m.len() / 1024).unwrap_or(0);
                self.conversation.add("system", &format!("Attached {kind} ({size_kb}KB): {}", path.display()));
                self.pending_images.push(path.to_path_buf());
            } else {
                remaining_text.push(raw_line);
            }
        }

        // Insert any non-file text into the input bar
        if !remaining_text.is_empty() {
            let text = remaining_text.join("\n");
            for ch in text.chars() {
                self.input_bar.textarea.input(tui_textarea::Input {
                    key: tui_textarea::Key::Char(ch),
                    ctrl: false,
                    alt: false,
                    shift: false,
                });
            }
        }
    }
}

/// Find the longest common prefix among a set of strings.
fn longest_common_prefix(strings: &[String]) -> Option<String> {
    if strings.is_empty() { return None; }
    let first = &strings[0];
    let mut len = first.len();
    for s in &strings[1..] {
        len = len.min(s.len());
        for (i, (a, b)) in first.bytes().zip(s.bytes()).enumerate() {
            if a != b {
                len = len.min(i);
                break;
            }
        }
    }
    if len == 0 { None } else { Some(first[..len].to_string()) }
}
