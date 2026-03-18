mod assembly;
mod claude;
mod claude_bridge;
mod component;
mod config;
mod event_handler;
mod phase_dispatch;
mod image;
mod model_session;
mod parser;
mod phase;
mod preview;
mod prompt_builder;
mod render;
mod python;
mod reference;
mod reference_detect;
mod session_manager;
mod spec;
mod stl;
mod storage;
mod tui;
mod usage;
mod viewer;

use crate::config::Config;
use crate::phase::Phase;
use crate::storage::Project;
use crate::claude_bridge::BusyState;
use crate::session_manager::SessionManager;
use crate::tui::{BackgroundResult, Focus};
use crate::tui::layout::{LayoutConfig, PanelRects, compute_layout};
use crate::tui::input_bar::InputBar;
use crate::tui::conversation::ConversationPane;
use crate::tui::project_tree::ProjectTreePane;
use crate::tui::model_panel::ModelPanel;
use crate::tui::spec_panel::SpecPanel;
use crate::tui::component_tree::ComponentTreePanel;
use crate::tui::component_list::ComponentListPanel;
use crate::tui::right_panel::RightPanel;
use crate::viewer::Viewer;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use std::path::PathBuf;
use std::time::Duration;

const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

#[derive(Debug, Clone)]
enum RenameTarget {
    Project { project_idx: usize, old_name: String },
    Session { project_idx: usize, old_name: String },
}

struct App<'a> {
    // Focus and state
    focus: Focus,
    layout_config: LayoutConfig,
    phase: Phase,

    // Panes
    project_tree: ProjectTreePane,
    conversation: ConversationPane,
    model_panel: ModelPanel,
    input_bar: InputBar<'a>,
    spec_panel: SpecPanel,
    component_tree_panel: ComponentTreePanel,
    component_list: ComponentListPanel,
    right_panel: RightPanel,

    // Backend
    session: SessionManager,
    claude_system_prompt: String,
    claude: claude_bridge::ClaudeBridge,
    viewer: Viewer,
    pending_images: Vec<PathBuf>,
    python_path: String,

    // Storage
    projects: Vec<Project>,

    // App state
    should_quit: bool,
    dirty: bool,
    spinner_frame: usize,
    /// Cached panel Rects for mouse hit-testing
    panel_rects: PanelRects,
    /// Timestamp of last Ctrl+C press (for double-tap quit)
    last_ctrl_c: Option<std::time::Instant>,

    // Session creation flags
    new_session_pending: bool,
    new_project_pending: bool,
    #[allow(dead_code)]
    export_pending: bool,
    rename_pending: Option<RenameTarget>,
    delete_pending: Option<DeleteTarget>,
    save_part_pending: bool,
    active_refs: Vec<String>,
    ref_confirm_pending: Option<PendingReference>,
    build_timeout: u64,
    /// Claude API usage monitor (5h / 7d limits)
    usage_monitor: usage::UsageMonitor,
}

#[derive(Debug, Clone)]
enum DeleteTarget {
    Project { project_idx: usize, name: String },
    Session { project_idx: usize, name: String },
}

#[derive(Debug, Clone)]
struct PendingReference {
    name: String,
    raw_response: String,
}

impl<'a> App<'a> {
    fn new(config: Config) -> Result<Self, String> {
        // Load system prompt
        let system_prompt_path = find_system_prompt()?;
        let claude_system_prompt = std::fs::read_to_string(&system_prompt_path)
            .map_err(|e| format!("Failed to read system prompt: {e}"))?;

        let python_path = config.python_path();
        let build_timeout = config.defaults.build_timeout;
        let session = SessionManager::new(build_timeout, python_path.clone());

        // Ensure ~/MiModel/ exists and scan for projects
        let _ = storage::project::ensure_root();
        seed_references();
        let projects = storage::project::list_projects().unwrap_or_default();

        let mut project_tree = ProjectTreePane::new();
        project_tree.refresh(&projects);

        let mut viewer = Viewer::new(&config.viewer.command);
        viewer.set_working_dir(session.temp_dir());

        Ok(App {
            focus: Focus::ProjectTree,
            layout_config: LayoutConfig::default(),
            phase: Phase::Spec,
            project_tree,
            conversation: ConversationPane::new(),
            model_panel: ModelPanel::new(),
            input_bar: InputBar::new(),
            spec_panel: SpecPanel::new(),
            component_tree_panel: ComponentTreePanel::new(),
            component_list: ComponentListPanel::new(),
            right_panel: RightPanel::new(),
            session,
            claude_system_prompt,
            claude: claude_bridge::ClaudeBridge::new(config.claude.model),
            viewer,
            pending_images: Vec::new(),
            python_path,
            projects,
            should_quit: false,
            dirty: true,
            spinner_frame: 0,
            panel_rects: PanelRects::default(),
            last_ctrl_c: None,
            new_session_pending: false,
            new_project_pending: false,
            export_pending: false,
            rename_pending: None,
            delete_pending: None,
            save_part_pending: false,
            active_refs: Vec::new(),
            ref_confirm_pending: None,
            build_timeout,
            usage_monitor: {
                let m = usage::UsageMonitor::new();
                m.maybe_refresh(); // fetch once at startup
                m
            },
        })
    }

    fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();

        // Show message if terminal is too narrow
        if area.width < 40 {
            let msg = Paragraph::new("Terminal too narrow.\nPlease resize to at least 40 columns.")
                .block(Block::default().borders(Borders::ALL));
            frame.render_widget(msg, area);
            return;
        }

        // Keep layout phase in sync with app phase
        self.layout_config.phase = self.phase;

        // Dynamic input bar height: grows with line count (1 line per row + 2 for border).
        let line_count = self.input_bar.textarea.lines().len();
        self.layout_config.input_height = (line_count as u16 + 2).clamp(5, 9);

        // Phase-aware placeholder text
        let placeholder = match self.phase {
            Phase::Spec => "Describe what you want to build...",
            Phase::Decompose => "Describe changes to the component tree...",
            Phase::Component => "Feedback, 'approve', or 'undo'...",
            Phase::Assembly => "Assembly instructions or feedback...",
            Phase::Refinement => "Parameter changes or feedback...",
        };
        self.input_bar.set_placeholder(placeholder);

        let panes = compute_layout(area, &self.layout_config);

        // Cache panel Rects for mouse hit-testing
        self.panel_rects.project_tree = panes.left_panel.unwrap_or_default();
        self.panel_rects.conversation = panes.conversation;
        self.panel_rects.right_panel = panes.right_panel.unwrap_or_default();
        self.panel_rects.input = panes.input_bar;

        // Render left panel — always show project tree (components visible inside session)
        if let Some(left_area) = panes.left_panel {
            self.project_tree.render(frame, left_area, self.focus == Focus::ProjectTree);
        }

        // Render conversation with spinner if busy
        let conv_area = panes.conversation;
        let mut conv = ConversationPane {
            entries: self.conversation.entries.clone(),
            // When auto-scrolling, use MAX so render clamps to actual bottom
            scroll_offset: if self.conversation.auto_scroll { u16::MAX } else { self.conversation.scroll_offset },
            auto_scroll: self.conversation.auto_scroll,
        };
        // Show streaming text or spinner when busy
        if self.claude.busy != BusyState::Idle {
            let spinner_char = SPINNER[self.spinner_frame % SPINNER.len()];
            let msg = match self.claude.busy {
                BusyState::Thinking => {
                    if self.claude.streaming_text.is_empty() {
                        format!("{spinner_char} Thinking...")
                    } else {
                        format!("{spinner_char} {}", self.claude.streaming_text)
                    }
                }
                BusyState::Building => format!("{spinner_char} Building..."),
                BusyState::Idle => unreachable!(),
            };
            conv.entries.push(crate::tui::conversation::ConversationEntry {
                role: if self.claude.streaming_text.is_empty() { "system" } else { "assistant" }.to_string(),
                content: msg,
            });
        }
        let max_scroll = conv.render(frame, conv_area, self.focus == Focus::Conversation);
        // Write the clamped scroll back so scroll_up() works from a real position
        self.conversation.scroll_offset = self.conversation.scroll_offset.min(max_scroll);

        // Render right panel (unified tabbed panel)
        if let Some(right_area) = panes.right_panel {
            self.right_panel.render(frame, right_area, self.focus == Focus::RightPanel);
        }

        // Render input bar with status indicators
        let bar_area = panes.input_bar;
        let input_focused = self.focus == Focus::Input;
        let border_color = if input_focused {
            Color::Rgb(137, 180, 250)
        } else {
            Color::Rgb(49, 50, 68)
        };

        // Build input bar title with status indicators
        let mut title_spans: Vec<Span> = vec![Span::raw(" Input ")];

        // Attachment indicators — separate images from PDFs
        if !self.pending_images.is_empty() {
            let img_count = self.pending_images.iter()
                .filter(|p| image::is_image(p))
                .count();
            let pdf_count = self.pending_images.iter()
                .filter(|p| image::is_pdf(p))
                .count();
            if img_count > 0 {
                title_spans.push(Span::styled(
                    format!(" {img_count} img "),
                    Style::default().fg(Color::Rgb(30, 30, 46)).bg(Color::Rgb(148, 226, 213)),
                ));
                title_spans.push(Span::raw(" "));
            }
            if pdf_count > 0 {
                title_spans.push(Span::styled(
                    format!(" {pdf_count} pdf "),
                    Style::default().fg(Color::Rgb(30, 30, 46)).bg(Color::Rgb(249, 226, 175)),
                ));
                title_spans.push(Span::raw(" "));
            }
        }

        // Busy indicator
        if self.claude.busy != BusyState::Idle {
            let spinner_char = SPINNER[self.spinner_frame % SPINNER.len()];
            let (label, fg, bg) = match self.claude.busy {
                BusyState::Thinking => ("Thinking", Color::Rgb(30, 30, 46), Color::Rgb(203, 166, 247)),
                BusyState::Building => ("Building", Color::Rgb(30, 30, 46), Color::Rgb(249, 226, 175)),
                BusyState::Idle => unreachable!(),
            };
            title_spans.push(Span::styled(
                format!(" {spinner_char} {label} "),
                Style::default().fg(fg).bg(bg),
            ));
        }

        self.input_bar.textarea.set_block(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(Style::default().fg(border_color))
                .title(Line::from(title_spans))
                .title_style(Style::default().fg(Color::Rgb(147, 153, 178)))
        );
        frame.render_widget(&self.input_bar.textarea, bar_area);

        // Render legend bar
        let legend_area = panes.legend;
        let phase_spans = render::phase_indicator_spans(
            self.phase,
            if self.component_list.len() > 0 { Some(self.component_list.selected()) } else { None },
            self.component_list.len(),
            self.component_list.selected_id(),
        );
        render::render_legend_bar(frame, legend_area, self.focus, phase_spans);

        // Render usage stats (right-aligned overlay on legend bar)
        let usage_stats = self.usage_monitor.stats();
        tui::status_bar::render_usage_bar(frame, legend_area, &usage_stats);
    }


    fn submit_prompt(&mut self, text: String) {
        if self.claude.busy != BusyState::Idle {
            self.conversation.add("system", "Please wait for the current operation to finish.");
            return;
        }

        // Handle save part
        if self.save_part_pending {
            self.save_part_pending = false;
            let part_name: String = text.chars().take(50).collect();
            let part_name = part_name.trim().to_string();
            if part_name.is_empty() {
                self.conversation.add("system", "Save cancelled (empty name).");
                return;
            }
            if let Some(ref stl_src) = self.session.latest_stl_path() {
                // Save to project dir as <name>.stl
                let dest_dir = self.session.active_dir
                    .as_ref()
                    .and_then(|d| d.parent().map(|p| p.to_path_buf()))
                    .unwrap_or_else(|| storage::project::root_dir().join("Untitled"));
                let dest = dest_dir.join(format!("{part_name}.stl"));
                let _ = std::fs::create_dir_all(&dest_dir);
                match std::fs::copy(stl_src, &dest) {
                    Ok(_) => {
                        self.conversation.add("system", &format!("Saved part '{part_name}.stl' to {}", dest_dir.display()));
                        // Also save the code alongside
                        if let Some(ref code) = self.session.current_code {
                            let code_dest = dest_dir.join(format!("{part_name}.py"));
                            let _ = std::fs::write(&code_dest, code);
                        }
                    }
                    Err(e) => self.conversation.add("system", &format!("Save failed: {e}")),
                }
            } else {
                self.conversation.add("system", "No model to save.");
            }
            return;
        }

        // Handle delete confirmation
        if let Some(target) = self.delete_pending.take() {
            if text.trim().eq_ignore_ascii_case("yes") {
                match target {
                    DeleteTarget::Project { name, .. } => {
                        match storage::project::delete_project(&name) {
                            Ok(()) => self.conversation.add("system", &format!("Deleted project '{name}'.")),
                            Err(e) => self.conversation.add("system", &format!("Delete failed: {e}")),
                        }
                    }
                    DeleteTarget::Session { project_idx, name } => {
                        if let Some(project) = self.projects.get(project_idx) {
                            let session_path = project.path.join(&name);
                            match storage::session::delete_session(&session_path) {
                                Ok(()) => self.conversation.add("system", &format!("Deleted session '{name}'.")),
                                Err(e) => self.conversation.add("system", &format!("Delete failed: {e}")),
                            }
                        }
                    }
                }
                self.refresh_projects();
            } else {
                self.conversation.add("system", "Delete cancelled.");
            }
            return;
        }

        // Handle rename pending
        if let Some(target) = self.rename_pending.take() {
            let new_name: String = text.chars().take(50).collect();
            let new_name = new_name.trim().to_string();
            if new_name.is_empty() {
                self.conversation.add("system", "Rename cancelled (empty name).");
                return;
            }
            match target {
                RenameTarget::Project { old_name, .. } => {
                    match storage::project::rename_project(&old_name, &new_name) {
                        Ok(()) => self.conversation.add("system", &format!("Renamed project '{old_name}' to '{new_name}'.")),
                        Err(e) => self.conversation.add("system", &format!("Rename failed: {e}")),
                    }
                }
                RenameTarget::Session { project_idx, old_name } => {
                    if let Some(project) = self.projects.get(project_idx) {
                        let session_path = project.path.join(&old_name);
                        match storage::session::rename_session(&session_path, &new_name) {
                            Ok(_) => self.conversation.add("system", &format!("Renamed session '{old_name}' to '{new_name}'.")),
                            Err(e) => self.conversation.add("system", &format!("Rename failed: {e}")),
                        }
                    }
                }
            }
            self.refresh_projects();
            return;
        }

        // Handle new project pending
        if self.new_project_pending {
            self.new_project_pending = false;
            let project_name: String = text.chars().take(30).collect();
            let project_name = project_name.trim().to_string();
            if !project_name.is_empty() {
                match storage::project::create_project(&project_name, "") {
                    Ok(_) => {
                        self.conversation.add("system", &format!("Created project '{project_name}'."));
                        self.refresh_projects();
                    }
                    Err(e) => {
                        self.conversation.add("system", &format!("Failed to create project: {e}"));
                    }
                }
            }
            return;
        }

        // Handle new session pending
        if self.new_session_pending {
            self.new_session_pending = false;
            // Reset session state
            self.session.reset();
            self.conversation.clear();
            self.model_panel.clear();
            self.claude.session_id = None;
            self.phase = Phase::Spec;
            self.layout_config.phase = Phase::Spec;
            self.active_refs.clear();
            self.ref_confirm_pending = None;
            self.conversation.add("system", "New session started.");
        }

        // Handle reference save confirmation
        if let Some(pending) = self.ref_confirm_pending.take() {
            if text.trim().eq_ignore_ascii_case("yes") {
                self.save_pending_reference(pending);
            } else {
                self.conversation.add("system", "Reference not saved.");
            }
            return;
        }

        // Handle /ref commands — extract attached images first since we return early
        if text.starts_with("/ref") {
            let (_clean, mut images) = image::extract_attachment_paths(&text);
            images.extend(self.pending_images.drain(..));

            // Check for multiple /ref commands (e.g. "/ref nema23, /ref Arduino Nano, /ref DM556-S")
            let parts: Vec<&str> = text.split("/ref")
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.trim().trim_matches(',').trim())
                .collect();

            if parts.len() > 1 {
                self.handle_multi_ref(parts, images);
            } else {
                self.handle_ref_command(&text, images);
            }
            return;
        }

        // Handle /attach command — explicit file attachment (works in tmux where drag-drop doesn't)
        if text.starts_with("/attach") {
            let paths_str = text.strip_prefix("/attach").unwrap_or("").trim();
            if paths_str.is_empty() {
                self.conversation.add("system", "Usage: /attach <path> [path2 ...]\nPaste or type file paths to attach images/PDFs.");
                return;
            }
            let (_, files) = image::extract_attachment_paths(paths_str);
            if files.is_empty() {
                self.conversation.add("system", "No valid image/PDF files found in the provided paths.");
            } else {
                for path in &files {
                    let kind = if image::is_pdf(path) { "PDF" } else { "image" };
                    let size_kb = std::fs::metadata(path).map(|m| m.len() / 1024).unwrap_or(0);
                    self.conversation.add("system", &format!("Attached {kind} ({size_kb}KB): {}", path.display()));
                }
                self.pending_images.extend(files);
            }
            return;
        }

        // Auto-create session name from first prompt if none active
        if self.session.active_name.is_none() {
            let session_name: String = text.chars()
                .filter(|c| c.is_alphanumeric() || *c == ' ')
                .take(30)
                .collect();
            let session_name = session_name.trim().replace(' ', "_");
            if !session_name.is_empty() {
                // Auto-create session dir under active project or "Untitled"
                let project_path = self.session.project_idx
                    .and_then(|idx| self.projects.get(idx))
                    .map(|p| p.path.clone())
                    .unwrap_or_else(|| storage::project::root_dir().join("Untitled"));
                let session_dir = project_path.join(&session_name);
                self.viewer.set_working_dir(&session_dir);
                self.session.active_name = Some(session_name);
                self.session.active_dir = Some(session_dir);
            }
        }

        // Create PhaseSession if we have a session dir but no phase session yet
        if self.session.phase_session.is_none() {
            if let Some(dir) = self.session.active_dir.clone() {
                self.session.create(dir, self.build_timeout, self.python_path.clone());
            }
        }

        // Extract attachment paths (images + PDFs) from text
        let (clean_text, mut extracted_images) = image::extract_attachment_paths(&text);
        // Show confirmation for files detected from typed/pasted paths
        for path in &extracted_images {
            let kind = if image::is_pdf(path) { "PDF" } else { "image" };
            let size_kb = std::fs::metadata(path).map(|m| m.len() / 1024).unwrap_or(0);
            self.conversation.add("system", &format!("Attached {kind} ({size_kb}KB): {}", path.display()));
        }
        extracted_images.extend(self.pending_images.drain(..));
        self.model_panel.pending_files.clear();
        let all_images = extracted_images;

        // Add user message to conversation
        self.conversation.add("user", &clean_text);
        self.session.add_message(self.phase, "user", &clean_text);
        self.session.save(self.phase);

        // Handle 'advance' command to move between phases
        if clean_text.trim().eq_ignore_ascii_case("advance") {
            match self.phase {
                Phase::Spec => {
                    self.phase = Phase::Decompose;
                    self.layout_config.phase = Phase::Decompose;
                    self.claude.session_id = None;
                    self.conversation.add("system", "Advanced to Decompose phase.");
                    self.session.save(self.phase);
                    self.dirty = true;
                }
                Phase::Decompose => {
                    self.conversation.add("system", "Use 'approve' to accept the component tree first.");
                }
                Phase::Assembly => {
                    self.phase = Phase::Refinement;
                    self.layout_config.phase = Phase::Refinement;
                    self.claude.session_id = None;
                    self.conversation.add("system", "Advanced to Refinement phase.");
                    self.session.save(self.phase);
                    self.dirty = true;
                }
                _ => {
                    self.conversation.add("system", "Cannot advance from this phase.");
                }
            }
            return;
        }

        // Phase-specific dispatch
        match self.phase {
            Phase::Spec => {
                self.send_spec_prompt(&clean_text, all_images);
                return;
            }
            Phase::Decompose => {
                if clean_text.trim().eq_ignore_ascii_case("approve") {
                    self.approve_decomposition();
                    return;
                }
                // Otherwise, send as feedback to Claude
                self.send_decompose_prompt(&clean_text);
                return;
            }
            Phase::Component => {
                let trimmed = clean_text.trim().to_lowercase();
                if trimmed == "approve" || trimmed == "ok" || trimmed == "next" {
                    self.approve_current_component();
                } else if trimmed == "undo" {
                    self.undo_component();
                } else {
                    // Text feedback — refine current component
                    self.send_component_feedback(&clean_text, all_images);
                }
                return;
            }
            Phase::Assembly => {
                self.handle_assembly_input(&clean_text);
                return;
            }
            Phase::Refinement => {
                self.handle_refinement_input(&clean_text);
                return;
            }
            // All phases are explicitly handled above — no fall-through to legacy path.
        }
    }

    fn handle_ref_command(&mut self, text: &str, attached_images: Vec<PathBuf>) {
        let args = text.strip_prefix("/ref").unwrap_or("").trim();

        if args.is_empty() || args == "list" {
            match reference::load_library() {
                Ok(library) if library.is_empty() => {
                    self.conversation.add("system", "Reference library is empty.");
                }
                Ok(library) => {
                    let list: Vec<String> = library.iter()
                        .map(|(c, s)| format!("  {} — {} [{}]", s, c.identity.name, c.identity.category))
                        .collect();
                    self.conversation.add("system", &format!("References:\n{}", list.join("\n")));
                }
                Err(e) => self.conversation.add("system", &format!("Error: {e}")),
            }
            return;
        }

        if let Some(name) = args.strip_prefix("remove ") {
            let name = name.trim();
            let slug = reference::slug_from_name(name);
            let path = reference::references_dir().join(format!("{slug}.toml"));
            if path.exists() {
                if let Err(e) = std::fs::remove_file(&path) {
                    self.conversation.add("system", &format!("Failed to remove: {e}"));
                } else {
                    self.active_refs.retain(|s| s != &slug);
                    self.conversation.add("system", &format!("Removed reference '{slug}'."));
                }
            } else {
                self.conversation.add("system", &format!("Reference '{slug}' not found."));
            }
            return;
        }

        let is_refresh = args.starts_with("refresh ");
        let query = if is_refresh {
            args.strip_prefix("refresh ").unwrap().trim()
        } else {
            args
        };

        // Try to load existing (unless refresh)
        if !is_refresh {
            match reference::load_one(query) {
                Ok((comp, slug)) => {
                    if !self.active_refs.contains(&slug) {
                        self.active_refs.push(slug.clone());
                    }
                    let summary = reference::summarize_for_prompt(&[&comp]);
                    self.conversation.add("system", &format!("Loaded reference:\n{summary}"));
                    self.refresh_refs_panel();
                    return;
                }
                Err(e) if e.contains("Multiple matches") => {
                    self.conversation.add("system", &e);
                    return;
                }
                Err(_) => {} // Not found — fall through to research
            }
        }

        // Research new component via Claude
        self.conversation.add("system", &format!("Researching '{query}'..."));

        let name = query.to_string();
        let research_prompt = format!(
            "Research the component: {name}\n\
             Find official datasheet or technical drawing.\n\
             Extract ALL mechanical dimensions in millimeters and key constraints.\n\
             Return the data as a TOML block in this exact format:\n\
             ```toml\n\
             [identity]\n\
             name = \"full component name\"\n\
             manufacturer = \"...\"\n\
             part_number = \"...\"\n\
             category = \"motor|fastener|bearing|connector|other\"\n\
             created = \"\"\n\
             updated = \"\"\n\n\
             [dimensions]\n\
             units = \"mm\"\n\
             key_name = value\n\n\
             [constraints]\n\
             key_with_unit_suffix = value\n\n\
             [sources]\n\
             urls = [\"...\"]\n\
             notes = \"...\"\n\
             ```\n\
             Return ONLY the TOML block, nothing else."
        );

        self.claude.send_raw_prompt(
            "You are a technical reference researcher. Search for component datasheets and extract precise mechanical specifications.",
            &research_prompt,
            &attached_images,
            &name,
        );
    }

    fn handle_multi_ref(&mut self, names: Vec<&str>, images: Vec<PathBuf>) {
        let mut loaded = Vec::new();
        let mut to_research = Vec::new();

        for name in &names {
            match reference::load_one(name) {
                Ok((comp, slug)) => {
                    if !self.active_refs.contains(&slug) {
                        self.active_refs.push(slug.clone());
                    }
                    loaded.push(comp.identity.name.clone());
                }
                Err(_) => {
                    to_research.push(name.to_string());
                }
            }
        }

        if !loaded.is_empty() {
            self.conversation.add("system",
                &format!("Loaded {} references: {}", loaded.len(), loaded.join(", ")));
            self.refresh_refs_panel();
        }

        if to_research.is_empty() {
            return;
        }

        // Research all unknown components in a single Claude call
        self.conversation.add("system",
            &format!("Researching {} components: {}...", to_research.len(), to_research.join(", ")));

        let research_prompt = format!(
            "Research these components and return a TOML block for EACH one:\n- {}\n\n\
             For each component, output a separate ```toml fenced block with [identity], [dimensions], [constraints], [sources] sections.\n\
             Use the exact format: name, manufacturer, part_number, category, created=\"\", updated=\"\" in [identity].\n\
             All dimensions in mm. Constraints with unit suffixes (_g, _a, _nm, _c, _kn).\n\
             Separate each component's TOML block clearly.",
            to_research.join("\n- ")
        );

        // Store the names for batch save handling (comma-separated signals batch mode)
        let result_name = to_research.join(",");

        self.claude.send_raw_prompt(
            "You are a technical reference researcher. Search for component datasheets and extract precise mechanical specifications.",
            &research_prompt,
            &images,
            &result_name,
        );
    }

    fn save_pending_reference(&mut self, pending: PendingReference) {
        let is_batch = pending.name.contains(',');

        if is_batch {
            // Extract multiple TOML blocks from the response
            let mut saved = Vec::new();
            let mut failed = Vec::new();
            let now = chrono::Utc::now().to_rfc3339();

            for block in pending.raw_response.split("```toml") {
                if let Some(end) = block.find("```") {
                    let toml_str = block[..end].trim();
                    if toml_str.is_empty() {
                        continue;
                    }
                    match toml::from_str::<reference::ReferenceComponent>(toml_str) {
                        Ok(mut comp) => {
                            if comp.identity.created.is_empty() {
                                comp.identity.created = now.clone();
                            }
                            if comp.identity.updated.is_empty() {
                                comp.identity.updated = now.clone();
                            }
                            let name = comp.identity.name.clone();
                            match reference::save(&comp) {
                                Ok(slug) => {
                                    if !self.active_refs.contains(&slug) {
                                        self.active_refs.push(slug);
                                    }
                                    saved.push(name);
                                }
                                Err(e) => failed.push(format!("{}: {}", name, e)),
                            }
                        }
                        Err(e) => failed.push(format!("parse error: {}", e)),
                    }
                }
            }

            if !saved.is_empty() {
                self.conversation.add("system",
                    &format!("Saved {} references: {}", saved.len(), saved.join(", ")));
                self.refresh_refs_panel();
            }
            if !failed.is_empty() {
                self.conversation.add("system",
                    &format!("Failed: {}", failed.join("; ")));
            }
        } else {
            // Single reference — existing logic
            let toml_str = if let Ok(extracted) = parser::parse_toml_response(&pending.raw_response) {
                extracted
            } else {
                pending.raw_response.clone()
            };

            let now = chrono::Utc::now().to_rfc3339();

            match toml::from_str::<reference::ReferenceComponent>(&toml_str) {
                Ok(mut comp) => {
                    if comp.identity.created.is_empty() {
                        comp.identity.created = now.clone();
                    }
                    if comp.identity.updated.is_empty() {
                        comp.identity.updated = now;
                    }
                    match reference::save(&comp) {
                        Ok(saved_slug) => {
                            if !self.active_refs.contains(&saved_slug) {
                                self.active_refs.push(saved_slug.clone());
                            }
                            self.conversation.add("system",
                                &format!("Saved reference '{}' as {saved_slug}.toml", comp.identity.name));
                            self.refresh_refs_panel();
                        }
                        Err(e) => self.conversation.add("system", &format!("Failed to save: {e}")),
                    }
                }
                Err(e) => {
                    self.conversation.add("system",
                        &format!("Failed to parse reference TOML: {e}\nTry `/ref refresh {}` to retry.", pending.name));
                }
            }
        }
    }

    fn build_ref_context(&self) -> Option<String> {
        let library = reference::load_library().unwrap_or_default();
        if library.is_empty() && self.active_refs.is_empty() {
            return None;
        }

        let mut parts = Vec::new();

        // Active references — full specs
        if !self.active_refs.is_empty() {
            let active: Vec<&reference::ReferenceComponent> = library.iter()
                .filter(|(_, slug)| self.active_refs.contains(slug))
                .map(|(comp, _)| comp)
                .collect();
            if !active.is_empty() {
                parts.push(format!(
                    "## Active Reference Components (use these dimensions)\n{}",
                    reference::summarize_for_prompt(&active)
                ));
            }
        }

        // All references — names only
        let all_refs: Vec<&reference::ReferenceComponent> = library.iter()
            .map(|(comp, _)| comp)
            .collect();
        if !all_refs.is_empty() {
            parts.push(format!(
                "## Available in Reference Library\n{}",
                reference::list_names(&all_refs)
            ));
        }

        if parts.is_empty() { None } else { Some(parts.join("\n\n")) }
    }

    /// Build full context for non-Spec phases: spec data + reference context.
    /// This ensures Claude knows what was specified when working in later phases.
    fn build_phase_context(&self) -> Option<String> {
        let mut parts = Vec::new();

        // Include spec data from the right panel
        let spec = self.right_panel.spec_content.clone();
        if !spec.is_empty() {
            parts.push(format!("## Design Specification\n{spec}"));
        }

        // Include previous conversation for context
        let spec_conversation = self.session.conversations(Phase::Spec);
        if !spec_conversation.is_empty() {
            let summary: Vec<String> = spec_conversation.iter()
                .filter(|e| e.role == "user" || e.role == "assistant")
                .take(20) // Limit to last 20 messages
                .map(|e| format!("{}: {}", e.role, e.content))
                .collect();
            if !summary.is_empty() {
                parts.push(format!("## Spec Conversation Summary\n{}", summary.join("\n")));
            }
        }

        // Include reference context
        if let Some(ref_ctx) = self.build_ref_context() {
            parts.push(ref_ctx);
        }

        if parts.is_empty() { None } else { Some(parts.join("\n\n")) }
    }

    fn handle_bg_result(&mut self, result: BackgroundResult) {
        // Refresh usage stats after each API interaction (cached, won't spam)
        self.usage_monitor.maybe_refresh();
        match result {
            BackgroundResult::ClaudeResponse { result, session_id } => {
                // Update session_id
                if let Some(sid) = session_id {
                    self.claude.session_id = Some(sid);
                }

                match result {
                    Ok(response) => {
                        // Add assistant message to conversation
                        self.conversation.add("assistant", &response);
                        self.session.add_message(self.phase, "assistant", &response);
                        self.session.save(self.phase);

                        match self.phase {
                            Phase::Spec => {
                                self.handle_spec_response(&response);
                                self.claude.busy = BusyState::Idle;
                            }
                            Phase::Decompose => {
                                self.handle_decompose_response(&response);
                                self.claude.busy = BusyState::Idle;
                            }
                            Phase::Component => {
                                // Parse response for cadquery code block
                                let parsed = parser::parse_response(&response);
                                if let Some(code_block) = parsed.code {
                                    // Build the component
                                    self.claude.busy = BusyState::Building;
                                    let build_result = self.session.build(&code_block.code, code_block.engine);
                                    self.handle_component_build_result(build_result, code_block.code);
                                } else {
                                    // No code in response — just a conversation message
                                    self.claude.busy = BusyState::Idle;
                                }
                            }
                            Phase::Assembly => {
                                // Assembly responses may contain code to rebuild, or just conversation
                                let parsed = parser::parse_response(&response);
                                if let Some(code_block) = parsed.code {
                                    self.claude.busy = BusyState::Building;
                                    let result = self.session.build(&code_block.code, code_block.engine);
                                    self.handle_build_result(result);
                                } else {
                                    self.claude.busy = BusyState::Idle;
                                }
                            }
                            Phase::Refinement => {
                                // Refinement responses may contain updated code
                                let parsed = parser::parse_response(&response);
                                if let Some(code_block) = parsed.code {
                                    self.claude.busy = BusyState::Building;
                                    let result = self.session.build(&code_block.code, code_block.engine);
                                    self.handle_build_result(result);
                                } else {
                                    self.claude.busy = BusyState::Idle;
                                }
                            }
                            // All phases are explicitly handled above — no catch-all build path.
                        }
                    }
                    Err(e) => {
                        self.conversation.add("system", &format!("Claude error: {e}"));
                        self.claude.busy = BusyState::Idle;
                    }
                }
            }
            BackgroundResult::BuildComplete(build_result) => {
                self.handle_build_result(build_result);
            }
            BackgroundResult::ReferenceResearch { name, result } => {
                match result {
                    Ok(response) => {
                        self.conversation.add("assistant", &response);
                        if name.contains(',') {
                            // Batch result — multiple components
                            self.conversation.add("system", "Save all references? (yes/no)");
                        } else {
                            self.conversation.add("system", "Save as reference? (yes/no)");
                        }
                        self.ref_confirm_pending = Some(PendingReference {
                            name,
                            raw_response: response,
                        });
                    }
                    Err(e) => {
                        self.conversation.add("system", &format!("Research failed: {e}"));
                    }
                }
                self.claude.busy = BusyState::Idle;
            }
        }
    }

    fn handle_build_result(&mut self, build_result: python::BuildResult) {
        match build_result {
            python::BuildResult::Success(ref meta) => {
                let dims_msg = format!(
                    "Built successfully\n{:.1} x {:.1} x {:.1} mm",
                    meta.dimensions.x, meta.dimensions.y, meta.dimensions.z
                );
                let features_str = if meta.features.is_empty() {
                    String::new()
                } else {
                    format!("\n{}", meta.features.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n"))
                };
                self.conversation.add("system", &format!("{dims_msg}{features_str}"));

                // Update model panel with STL path for braille preview
                let stl_path = self.session.latest_stl_path();
                let iteration = self.session.iteration();
                self.model_panel.update(meta, stl_path.as_deref(), iteration);
                let model_summary = format!(
                    "{:.1} x {:.1} x {:.1} mm\nIterations: {}\nEngine: {}\nWatertight: {}{}",
                    meta.dimensions.x, meta.dimensions.y, meta.dimensions.z,
                    iteration,
                    meta.engine.as_str(),
                    if meta.watertight { "yes" } else { "no" },
                    if meta.features.is_empty() { String::new() } else {
                        format!("\n\nFeatures:\n{}", meta.features.iter().map(|f| format!("  - {f}")).collect::<Vec<_>>().join("\n"))
                    }
                );
                self.right_panel.set_model(&model_summary);

                // Update working.stl so f3d auto-reloads
                if let Some(ref src) = stl_path {
                    if let Err(e) = self.viewer.update_working_stl(src) {
                        self.conversation.add("system", &format!("Warning: {e}"));
                    }
                    // Auto-open f3d on first build if not already running
                    if !self.viewer.is_running() {
                        let _ = self.viewer.show();
                    }
                }

                // Auto-save phase session
                self.session.save(self.phase);
                self.refresh_projects();
            }
            python::BuildResult::BuildError(e) | python::BuildResult::SyntaxError(e) => {
                self.conversation.add("system", &format!("Build error: {}", e.error));
            }
            python::BuildResult::Timeout => {
                self.conversation.add("system", "Build timed out.");
            }
        }
        self.claude.busy = BusyState::Idle;
    }

    fn load_session(&mut self, project_idx: usize, session_name: String) {
        if let Some(project) = self.projects.get(project_idx) {
            let session_dir = project.path.join(&session_name);

            match self.session.load(&session_dir, self.build_timeout, self.python_path.clone()) {
                Ok(()) => {
                    // Clear any pending operations from before the load
                    self.new_project_pending = false;
                    self.new_session_pending = false;
                    self.save_part_pending = false;
                    self.rename_pending = None;
                    self.delete_pending = None;
                    self.ref_confirm_pending = None;

                    let phase = self.session.phase_session.as_ref()
                        .map(|ps| ps.phase)
                        .unwrap_or(Phase::Spec);

                    // Restore phase
                    self.phase = phase;
                    self.layout_config.phase = phase;

                    // Restore conversation from saved data
                    self.conversation.clear();
                    let entries = self.session.conversations(phase);
                    for entry in entries {
                        self.conversation.add(&entry.role, &entry.content);
                    }
                    self.conversation.add("system", &format!(
                        "Resumed session '{}' in {} phase.", session_name, phase.label()
                    ));

                    // Point viewer at session directory so f3d watches the right file
                    self.viewer.set_working_dir(&session_dir);

                    // Launch viewer if working.stl exists
                    let working_stl = session_dir.join("working.stl");
                    if working_stl.exists() && !self.viewer.is_running() {
                        let _ = self.viewer.show();
                    }

                    // Crash recovery hint
                    if phase == Phase::Component {
                        self.conversation.add("system",
                            "Tip: If the last build was interrupted, type 'undo' to restore the previous state.");
                    }

                    // Store session state
                    self.session.project_idx = Some(project_idx);
                    self.session.active_name = Some(session_name.clone());

                    // Update project tree selection
                    self.project_tree.active_project = Some(project_idx);
                    self.project_tree.active_session = Some(session_name.to_string());
                    self.refresh_projects();
                    // Focus stays in ProjectTree — don't switch to Input
                }
                Err(e) => {
                    self.conversation.add("system", &format!("Failed to load session: {e}"));
                }
            }
        }
    }

    fn open_project(&mut self, project_idx: usize) {
        if let Some(project) = self.projects.get(project_idx) {
            // Clear any pending operations
            self.new_project_pending = false;
            self.new_session_pending = false;
            self.save_part_pending = false;
            self.rename_pending = None;
            self.delete_pending = None;
            self.ref_confirm_pending = None;

            // Set active project so new prompts land here
            self.session.project_idx = Some(project_idx);
            self.session.active_name = None;
            self.session.active_dir = None;
            self.session.phase_session = None;
            self.claude.session_id = None;

            // Reset build state (but don't clear project_idx/active_name which we just set)
            self.conversation.clear();
            self.model_panel.clear();

            // Show project info
            let name = &project.meta.name;
            let desc = if project.meta.description.is_empty() {
                String::new()
            } else {
                format!("\n{}", project.meta.description)
            };
            self.conversation.add("system", &format!("Project: {name}{desc}"));

            // List sessions with status
            if project.sessions.is_empty() {
                self.conversation.add("system", "No sessions yet. Type a prompt to start building.");
            } else {
                let mut session_info = String::from("Sessions:");
                for si in &project.sessions {
                    let sname = &si.name;
                    let session_path = project.path.join(sname);
                    let status = storage::session::session_status(&session_path);
                    let detail = match status {
                        storage::session::SessionStatus::Ok { phase, created } => {
                            let date = created.split('T').next().unwrap_or(&created);
                            format!("  {sname}  ({phase}, {date})")
                        }
                        storage::session::SessionStatus::Empty => {
                            format!("  {sname}  (empty)")
                        }
                        storage::session::SessionStatus::Corrupted => {
                            format!("  {sname}  (corrupted)")
                        }
                    };
                    session_info.push_str(&format!("\n{detail}"));
                }
                self.conversation.add("system", &session_info);
            }

            // Check for saved parts (.stl files in project root)
            let mut parts: Vec<String> = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&project.path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("stl") {
                        if let Some(name) = path.file_stem().and_then(|n| n.to_str()) {
                            parts.push(name.to_string());
                        }
                    }
                }
            }
            if !parts.is_empty() {
                parts.sort();
                let parts_list = parts.iter().map(|p| format!("  {p}.stl")).collect::<Vec<_>>().join("\n");
                self.conversation.add("system", &format!("Saved parts:\n{parts_list}"));
            }

            // Check for documentation files
            let doc_names = ["README.md", "readme.md", "NOTES.md", "notes.md", "notes.txt", "docs.md"];
            for doc_name in &doc_names {
                let doc_path = project.path.join(doc_name);
                if doc_path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&doc_path) {
                        let preview: String = content.lines().take(20).collect::<Vec<_>>().join("\n");
                        self.conversation.add("system", &format!("{doc_name}:\n{preview}"));
                    }
                }
            }

            // Open latest STL in f3d if any session has one
            if let Some(project) = self.projects.get(project_idx) {
                let mut latest_stl: Option<PathBuf> = None;
                // Check sessions in reverse (last = most recent)
                for si in project.sessions.iter().rev() {
                    let sname = &si.name;
                    let session_path = project.path.join(sname);
                    // Find highest iteration STL
                    if let Ok(entries) = std::fs::read_dir(&session_path) {
                        let mut stls: Vec<PathBuf> = entries.flatten()
                            .map(|e| e.path())
                            .filter(|p| {
                                p.file_name()
                                    .and_then(|n| n.to_str())
                                    .map(|n| n.starts_with("iter_") && n.ends_with(".stl"))
                                    .unwrap_or(false)
                            })
                            .collect();
                        stls.sort();
                        if let Some(stl) = stls.last() {
                            latest_stl = Some(stl.clone());
                            break;
                        }
                    }
                }
                if let Some(ref stl) = latest_stl {
                    if let Err(e) = self.viewer.update_working_stl(stl) {
                        self.conversation.add("system", &format!("Warning: {e}"));
                    }
                    if !self.viewer.is_running() {
                        let _ = self.viewer.show();
                    }
                }
            }

            // Focus stays in ProjectTree — don't switch to Input
        }
    }

    fn refresh_projects(&mut self) {
        self.projects = storage::project::list_projects().unwrap_or_default();
        let projects = self.projects.clone();
        self.project_tree.refresh(&projects);
    }

    /// Rebuild the Refs tab content from the current active_refs list.
    fn refresh_refs_panel(&mut self) {
        let library = reference::load_library().unwrap_or_default();
        if self.active_refs.is_empty() {
            self.right_panel.set_refs("No references loaded. Use /ref <name> to load.");
            return;
        }
        let mut lines = Vec::new();
        lines.push(format!("Active references ({}):", self.active_refs.len()));
        for slug in &self.active_refs {
            if let Some((comp, _)) = library.iter().find(|(_, s)| s == slug) {
                lines.push(format!(
                    "  {} — {} [{}]",
                    slug, comp.identity.name, comp.identity.category
                ));
            } else {
                lines.push(format!("  {slug} (not in library)"));
            }
        }
        self.right_panel.set_refs(&lines.join("\n"));
    }

    /// Dispatch an MCP tool call from Claude's stream to the appropriate handler.
    fn handle_tool_call(&mut self, tool: claude_bridge::ToolCall) {
        // Strip mcp__mimodel__ prefix
        let name = tool.name.strip_prefix("mcp__mimodel__").unwrap_or(&tool.name);

        match name {
            "ask_question" | "ask_clarification" => {
                if let Some(q) = tool.input.get("question").and_then(|v| v.as_str()) {
                    self.session.add_message(self.phase, "assistant", q);
                    self.conversation.add("assistant", q);
                }
            }
            "record_spec_field" => {
                let cat = tool.input.get("category").and_then(|v| v.as_str()).unwrap_or("");
                let key = tool.input.get("key").and_then(|v| v.as_str()).unwrap_or("");
                let val = tool.input.get("value").and_then(|v| v.as_str()).unwrap_or("");
                let unit = tool.input.get("unit").and_then(|v| v.as_str()).unwrap_or("");
                let entry = format!("[{}] {} = {} {}", cat, key, val, unit);
                let mut content = self.right_panel.spec_content.clone();
                if !content.is_empty() { content.push('\n'); }
                content.push_str(&entry);
                self.right_panel.set_spec(&content);
            }
            "mark_spec_complete" => {
                self.conversation.add("system", "Spec complete. Type 'advance' to move to Decompose phase.");
                self.session.add_message(self.phase, "system", "Spec complete.");
            }
            "propose_component_tree" => {
                if let Some(components) = tool.input.get("components").and_then(|v| v.as_array()) {
                    let mut lines = Vec::new();
                    for c in components {
                        let id = c.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                        let cname = c.get("name").and_then(|v| v.as_str()).unwrap_or(id);
                        let op = c.get("assembly_op").and_then(|v| v.as_str()).unwrap_or("union");
                        lines.push(format!("  {} -- {} [{}]", id, cname, op));
                    }
                    self.conversation.add("system",
                        &format!("Component tree proposed:\n{}\nType 'approve' to accept, or describe changes.",
                            lines.join("\n")));
                }
            }
            "submit_cadquery_code" | "submit_assembly_code" | "submit_code_patch" => {
                // Build happened in MCP server — working.stl already written to session dir.
                // Viewer's working_dir points there, so just launch if not running.
                if !self.viewer.is_running() {
                    let _ = self.viewer.show();
                }
                self.right_panel.set_model("Build complete -- check 3D viewer");
            }
            "request_approval" => {
                if let Some(summary) = tool.input.get("summary").and_then(|v| v.as_str()) {
                    self.conversation.add("system",
                        &format!("Review model in viewer. {}\nType 'approve' or describe changes.", summary));
                }
            }
            "update_parameter" => {
                let pname = tool.input.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let new_val = tool.input.get("new_value").and_then(|v| v.as_str()).unwrap_or("");
                let mut content = self.right_panel.spec_content.clone();
                content.push_str(&format!("\nUpdated: {} = {}", pname, new_val));
                self.right_panel.set_spec(&content);
            }
            "open_viewer" => {
                // Signal file is handled in poll loop; this ensures
                // the viewer opens if the tool call arrives via streaming too
                if !self.viewer.is_running() {
                    let _ = self.viewer.show();
                }
            }
            _ => {} // Unknown tool -- ignore
        }
    }

    /// Kill any running Claude subprocess on app exit.
    fn cleanup(&self) {
        self.claude.cancel();
    }

}

/// Decode percent-encoded URI path (e.g. %20 -> space).
fn percent_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            } else {
                result.push('%');
                result.push_str(&hex);
            }
        } else {
            result.push(ch);
        }
    }
    result
}

fn find_system_prompt() -> Result<std::path::PathBuf, String> {
    let starts: Vec<std::path::PathBuf> = [
        std::env::current_dir().ok(),
        std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())),
    ]
    .into_iter()
    .flatten()
    .collect();

    for start in &starts {
        let mut dir = start.as_path();
        loop {
            let candidate = dir.join("prompts/system.md");
            if candidate.exists() {
                return Ok(candidate);
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => break,
            }
        }
    }
    // Return a dummy path — missing prompt is handled gracefully by not loading
    Err("prompts/system.md not found. Run from within the MiModel project.".to_string())
}

fn startup_checks(config: &Config) -> Result<(), String> {
    claude::check_claude()?;
    python::check_python(&config.python_path())?;
    if !which_exists(&config.viewer.command) {
        eprintln!("Warning: {} not found. Install for 3D preview.", config.viewer.command);
    }
    Ok(())
}

fn which_exists(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn make_fallback_app<'a>(config: Config, warn: &str) -> App<'a> {
    eprintln!("Warning: {warn}");
    let python_path = config.python_path();
    let projects = storage::project::list_projects().unwrap_or_default();
    let mut pt = ProjectTreePane::new();
    pt.refresh(&projects);
    App {
        focus: Focus::ProjectTree,
        layout_config: LayoutConfig::default(),
        phase: Phase::Spec,
        project_tree: pt,
        conversation: ConversationPane::new(),
        model_panel: ModelPanel::new(),
        input_bar: InputBar::new(),
        spec_panel: SpecPanel::new(),
        component_tree_panel: ComponentTreePanel::new(),
        component_list: ComponentListPanel::new(),
        right_panel: RightPanel::new(),
        session: SessionManager::new(60, python_path.clone()),
        claude_system_prompt: String::new(),
        claude: claude_bridge::ClaudeBridge::new(config.claude.model.clone()),
        viewer: Viewer::new(&config.viewer.command),
        pending_images: Vec::new(),
        python_path,
        projects,
        should_quit: false,
        dirty: true,
        spinner_frame: 0,
        panel_rects: PanelRects::default(),
        last_ctrl_c: None,
        new_session_pending: false,
        new_project_pending: false,
        export_pending: false,
        rename_pending: None,
        delete_pending: None,
        save_part_pending: false,
        active_refs: Vec::new(),
        ref_confirm_pending: None,
        build_timeout: 60,
        usage_monitor: usage::UsageMonitor::new(),
    }
}

/// Seed ~/MiModel/references/ with common components on first run.
fn seed_references() {
    let dir = reference::references_dir();
    if dir.exists() && std::fs::read_dir(&dir).map(|mut d| d.next().is_some()).unwrap_or(false) {
        return; // Already has references
    }
    let _ = reference::ensure_references_dir();

    let seeds: &[(&str, &str)] = &[
        ("m3_shcs.toml", include_str!("../references/m3_shcs.toml")),
        ("m3x5x4_threaded_insert.toml", include_str!("../references/m3x5x4_threaded_insert.toml")),
    ];
    for (name, content) in seeds {
        let path = dir.join(name);
        if !path.exists() {
            let _ = std::fs::write(&path, content);
        }
    }
}

fn main() {
    let config = Config::load();

    // Non-fatal startup checks — warn but continue
    if let Err(e) = startup_checks(&config) {
        eprintln!("Startup warning: {e}");
    }

    let mut app = match App::new(config.clone()) {
        Ok(app) => app,
        Err(e) => make_fallback_app(config, &e),
    };

    // Initialize ratatui terminal
    let mut terminal = ratatui::init();

    // Enable bracketed paste for drag-and-drop file detection and mouse capture
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::event::EnableBracketedPaste,
        crossterm::event::EnableMouseCapture,
    );

    // Run event loop
    let result = run_event_loop(&mut terminal, &mut app);

    // Kill any running Claude subprocess before exiting
    app.cleanup();

    // Disable bracketed paste and mouse capture before restoring terminal
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::event::DisableBracketedPaste,
        crossterm::event::DisableMouseCapture,
    );

    // Restore terminal
    ratatui::restore();

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run_event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
) -> std::io::Result<()> {
    let mut tick_count: u64 = 0;

    loop {
        // Drain streaming text chunks from Claude
        if app.claude.drain_streaming() {
            app.conversation.scroll_to_bottom();
            app.dirty = true;
        }

        // Check background channel (final result)
        if let Some(result) = app.claude.try_recv_result() {
            app.claude.streaming_text.clear();
            app.handle_bg_result(result);
            app.dirty = true;
        }

        // Drain MCP tool calls
        let tool_calls = app.claude.drain_tool_calls();
        for tc in tool_calls {
            app.handle_tool_call(tc);
            app.dirty = true;
        }

        // Poll .building file for BusyState transitions
        if app.claude.busy == BusyState::Thinking {
            if let Some(ref dir) = app.session.active_dir {
                let building = dir.join(".building");
                if building.exists() {
                    app.claude.busy = BusyState::Building;
                    app.dirty = true;
                }
            }
        } else if app.claude.busy == BusyState::Building {
            if let Some(ref dir) = app.session.active_dir {
                let building = dir.join(".building");
                if !building.exists() {
                    app.claude.busy = BusyState::Thinking;
                    app.dirty = true;
                }
            }
        }

        // Poll .open_viewer signal from MCP server
        if let Some(ref dir) = app.session.active_dir {
            let signal = dir.join(".open_viewer");
            if signal.exists() {
                let _ = std::fs::remove_file(&signal);
                let working_stl = dir.join("working.stl");
                if working_stl.exists() {
                    let _ = app.viewer.update_working_stl(&working_stl);
                    if !app.viewer.is_running() {
                        let _ = app.viewer.show();
                    }
                }
                app.dirty = true;
            }
        }

        // Render only when dirty
        if app.dirty {
            terminal.draw(|f| app.render(f))?;
            app.dirty = false;
        }

        // Poll for events with 50ms timeout
        if crossterm::event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    app.handle_key(key);
                    app.dirty = true;
                }
                Event::Paste(text) => {
                    app.handle_paste(text);
                    app.dirty = true;
                }
                Event::Mouse(mouse) => {
                    use crossterm::event::{MouseEventKind, MouseButton};
                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            let pos = ratatui::prelude::Position::new(mouse.column, mouse.row);
                            if app.panel_rects.project_tree.contains(pos) {
                                app.focus = Focus::ProjectTree;
                            } else if app.panel_rects.conversation.contains(pos) {
                                app.focus = Focus::Conversation;
                            } else if app.panel_rects.right_panel.contains(pos) {
                                app.focus = Focus::RightPanel;
                            } else if app.panel_rects.input.contains(pos) {
                                app.focus = Focus::Input;
                            }
                            app.dirty = true;
                        }
                        MouseEventKind::ScrollUp => {
                            let pos = ratatui::prelude::Position::new(mouse.column, mouse.row);
                            if app.panel_rects.conversation.contains(pos) {
                                app.conversation.scroll_up(3);
                            } else if app.panel_rects.right_panel.contains(pos) {
                                app.right_panel.scroll_up(3);
                            }
                            app.dirty = true;
                        }
                        MouseEventKind::ScrollDown => {
                            let pos = ratatui::prelude::Position::new(mouse.column, mouse.row);
                            if app.panel_rects.conversation.contains(pos) {
                                app.conversation.scroll_down(3);
                            } else if app.panel_rects.right_panel.contains(pos) {
                                app.right_panel.scroll_down(3);
                            }
                            app.dirty = true;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        // Advance spinner at ~10fps (every 5th loop at 50ms = 250ms period)
        if app.claude.busy != BusyState::Idle && tick_count % 5 == 0 {
            app.spinner_frame = app.spinner_frame.wrapping_add(1);
            app.dirty = true;
        }

        tick_count = tick_count.wrapping_add(1);

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
