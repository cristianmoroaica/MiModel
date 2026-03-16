mod assembly;
mod claude;
mod component;
mod config;
mod image;
mod model_session;
mod parser;
mod phase;
mod preview;
mod prompt_builder;
mod python;
mod spec;
mod stl;
mod storage;
mod tui;
mod viewer;

use crate::config::Config;
use crate::model_session::{PhaseSession, Session};
use crate::phase::Phase;
use crate::storage::Project;
use crate::tui::{BackgroundResult, BusyState, Focus};
use crate::tui::layout::{LayoutConfig, compute_layout};
use crate::tui::input_bar::InputBar;
use crate::tui::conversation::ConversationPane;
use crate::tui::project_tree::ProjectTreePane;
use crate::tui::model_panel::ModelPanel;
use crate::tui::spec_panel::SpecPanel;
use crate::tui::component_tree::ComponentTreePanel;
use crate::tui::component_list::ComponentListPanel;
use crate::viewer::Viewer;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
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
    busy: BusyState,
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

    // Backend
    session: Session,
    phase_session: Option<PhaseSession>,
    claude_model: Option<String>,
    claude_system_prompt: String,
    claude_session_id: Option<String>,
    viewer: Viewer,
    pending_images: Vec<PathBuf>,
    python_path: String,

    // Background channels
    bg_tx: mpsc::Sender<BackgroundResult>,
    bg_rx: mpsc::Receiver<BackgroundResult>,
    /// Channel for streaming text chunks from Claude
    stream_rx: mpsc::Receiver<String>,
    stream_tx: mpsc::Sender<String>,
    bg_pid: Arc<AtomicU32>,

    // Storage
    projects: Vec<Project>,
    active_project_idx: Option<usize>,
    active_session_name: Option<String>,
    active_session_dir: Option<PathBuf>,

    // App state
    should_quit: bool,
    spinner_frame: usize,
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
    /// Accumulates streaming text from Claude during Thinking state
    streaming_text: String,
    build_timeout: u64,
}

#[derive(Debug, Clone)]
enum DeleteTarget {
    Project { project_idx: usize, name: String },
    Session { project_idx: usize, name: String },
}

impl<'a> App<'a> {
    fn new(config: Config) -> Result<Self, String> {
        // Load system prompt
        let system_prompt_path = find_system_prompt()?;
        let claude_system_prompt = std::fs::read_to_string(&system_prompt_path)
            .map_err(|e| format!("Failed to read system prompt: {e}"))?;

        let python_path = config.python_path();
        let build_timeout = config.defaults.build_timeout;
        let session = Session::new(build_timeout, python_path.clone());

        // Ensure ~/MiModel/ exists and scan for projects
        let _ = storage::project::ensure_root();
        let projects = storage::project::list_projects().unwrap_or_default();

        // Setup background channel
        let (bg_tx, bg_rx) = mpsc::channel::<BackgroundResult>();
        let (stream_tx, stream_rx) = mpsc::channel::<String>();
        let bg_pid = Arc::new(AtomicU32::new(0));

        let mut project_tree = ProjectTreePane::new();
        project_tree.refresh(&projects);

        let mut viewer = Viewer::new(&config.viewer.command);
        viewer.set_working_dir(session.temp_dir());

        Ok(App {
            focus: Focus::ProjectTree,
            busy: BusyState::Idle,
            layout_config: LayoutConfig::default(),
            phase: Phase::Spec,
            project_tree,
            conversation: ConversationPane::new(),
            model_panel: ModelPanel::new(),
            input_bar: InputBar::new(),
            spec_panel: SpecPanel::new(),
            component_tree_panel: ComponentTreePanel::new(),
            component_list: ComponentListPanel::new(),
            session,
            phase_session: None,
            claude_model: config.claude.model,
            claude_system_prompt,
            claude_session_id: None,
            viewer,
            pending_images: Vec::new(),
            python_path,
            bg_tx,
            bg_rx,
            stream_tx,
            stream_rx,
            bg_pid,
            projects,
            active_project_idx: None,
            active_session_name: None,
            active_session_dir: None,
            should_quit: false,
            spinner_frame: 0,
            last_ctrl_c: None,
            new_session_pending: false,
            new_project_pending: false,
            export_pending: false,
            rename_pending: None,
            delete_pending: None,
            save_part_pending: false,
            streaming_text: String::new(),
            build_timeout,
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
        let panes = compute_layout(area, &self.layout_config);

        // Render left panel (phase-aware)
        if let Some(left_area) = panes.left_panel {
            match self.phase {
                Phase::Spec | Phase::Decompose => {
                    self.project_tree.render(frame, left_area, self.focus == Focus::ProjectTree);
                }
                Phase::Component | Phase::Assembly | Phase::Refinement => {
                    self.component_list.render(frame, left_area, self.focus == Focus::ProjectTree);
                }
            }
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
        if self.busy != BusyState::Idle {
            let spinner_char = SPINNER[self.spinner_frame % SPINNER.len()];
            let msg = match self.busy {
                BusyState::Thinking => {
                    if self.streaming_text.is_empty() {
                        format!("{spinner_char} Thinking...")
                    } else {
                        format!("{spinner_char} {}", self.streaming_text)
                    }
                }
                BusyState::Building => format!("{spinner_char} Building..."),
                BusyState::Idle => unreachable!(),
            };
            conv.entries.push(crate::tui::conversation::ConversationEntry {
                role: if self.streaming_text.is_empty() { "system" } else { "assistant" }.to_string(),
                content: msg,
            });
        }
        let max_scroll = conv.render(frame, conv_area, self.focus == Focus::Conversation);
        // Write the clamped scroll back so scroll_up() works from a real position
        self.conversation.scroll_offset = self.conversation.scroll_offset.min(max_scroll);

        // Render right panel (phase-aware)
        if let Some(right_area) = panes.right_panel {
            match self.phase {
                Phase::Spec => {
                    self.spec_panel.render(frame, right_area, false);
                }
                Phase::Decompose => {
                    self.component_tree_panel.render(frame, right_area, false);
                }
                Phase::Component | Phase::Assembly | Phase::Refinement => {
                    self.model_panel.render(frame, right_area, false);
                }
            }
        }

        // Render input bar with status indicators
        let bar_area = panes.input_bar;
        let input_focused = self.focus == Focus::Input;
        let border_color = if input_focused { Color::Cyan } else { Color::DarkGray };

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
                    Style::default().fg(Color::Black).bg(Color::Cyan),
                ));
                title_spans.push(Span::raw(" "));
            }
            if pdf_count > 0 {
                title_spans.push(Span::styled(
                    format!(" {pdf_count} pdf "),
                    Style::default().fg(Color::Black).bg(Color::Yellow),
                ));
                title_spans.push(Span::raw(" "));
            }
        }

        // Busy indicator
        if self.busy != BusyState::Idle {
            let spinner_char = SPINNER[self.spinner_frame % SPINNER.len()];
            let (label, color) = match self.busy {
                BusyState::Thinking => ("Thinking", Color::Magenta),
                BusyState::Building => ("Building", Color::Yellow),
                BusyState::Idle => unreachable!(),
            };
            title_spans.push(Span::styled(
                format!(" {spinner_char} {label} "),
                Style::default().fg(Color::Black).bg(color),
            ));
        }

        self.input_bar.textarea.set_block(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(Line::from(title_spans))
        );
        frame.render_widget(&self.input_bar.textarea, bar_area);

        // Render legend bar
        let legend_area = panes.legend;
        let mut legend_spans = self.phase_indicator_spans();
        legend_spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));
        legend_spans.push(Span::styled(" Alt+1-5 ", Style::default().fg(Color::Black).bg(Color::DarkGray)));
        legend_spans.push(Span::raw(" Phase "));
        legend_spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));
        let focus_spans: Vec<Span> = match self.focus {
            Focus::Input => vec![
                Span::styled(" Enter ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::raw(" Send "),
                Span::styled(" PgUp/Dn ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::raw(" Scroll "),
                Span::styled(" Tab ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::raw(" Panes "),
                Span::styled(" Ctrl+W ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::raw(" Save "),
                Span::styled(" Ctrl+V ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::raw(" Img "),
                Span::styled(" Ctrl+C ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::raw(" Quit "),
            ],
            Focus::ProjectTree => vec![
                Span::styled(" j/k ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::raw(" Navigate "),
                Span::styled(" Enter ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::raw(" Open/Expand "),
                Span::styled(" e ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::raw(" Rename "),
                Span::styled(" d ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::raw(" Delete "),
                Span::styled(" Tab ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::raw(" Panes "),
                Span::styled(" Ctrl+C ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::raw(" Quit "),
            ],
            Focus::Conversation => vec![
                Span::styled(" j/k ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::raw(" Scroll "),
                Span::styled(" u/d ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::raw(" Page "),
                Span::styled(" Tab ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::raw(" Panes "),
                Span::styled(" Ctrl+C ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::raw(" Quit "),
            ],
        };
        legend_spans.extend(focus_spans);
        let legend_text = Line::from(legend_spans);
        frame.render_widget(Paragraph::new(legend_text), legend_area);
    }

    /// Build phase indicator spans for the legend bar.
    /// Shows: " Spec ● ○ ○ ○ ○ " with the current phase filled.
    /// During Component phase, also shows progress like "Component 2/5: Case Body".
    fn phase_indicator_spans(&self) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        let current_idx = self.phase.index();

        // Phase label — with component progress when applicable
        let label = match self.phase {
            Phase::Component => {
                let total = self.component_list.len();
                if total > 0 {
                    let current = self.component_list.selected() + 1;
                    let name = self.component_list.selected_id()
                        .unwrap_or("?")
                        .to_string();
                    format!(" {} {}/{}: {} ", self.phase.label(), current, total, name)
                } else {
                    format!(" {} ", self.phase.label())
                }
            }
            _ => format!(" {} ", self.phase.label()),
        };
        spans.push(Span::styled(label, Style::default().fg(Color::White).bold()));

        // Phase dots
        for i in 0..5 {
            let dot = if i == current_idx { "\u{25cf}" } else { "\u{25cb}" };
            let style = if i == current_idx {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            spans.push(Span::styled(format!(" {dot}"), style));
        }
        spans.push(Span::raw(" "));

        spans
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
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
                if self.busy == BusyState::Idle {
                    if self.session.undo() {
                        self.conversation.add("system", "Undone last iteration.");
                        self.model_panel.clear();
                        if let Some(meta) = &self.session.current_metadata {
                            self.model_panel.update(meta, None, 0);
                        }
                    } else {
                        self.conversation.add("system", "Nothing to undo.");
                    }
                }
                return;
            }
            (Char('c'), KeyModifiers::CONTROL) => {
                if self.busy != BusyState::Idle {
                    // Kill background process
                    let pid = self.bg_pid.load(Ordering::SeqCst);
                    if pid != 0 {
                        unsafe {
                            #[cfg(unix)]
                            libc::kill(pid as i32, libc::SIGTERM);
                        }
                        self.conversation.add("system", "(cancelled)");
                        self.busy = BusyState::Idle;
                    }
                    self.last_ctrl_c = None;
                } else {
                    // Double Ctrl+C to quit
                    let now = std::time::Instant::now();
                    if let Some(last) = self.last_ctrl_c {
                        if now.duration_since(last).as_millis() < 500 {
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
                let img_dir = self.active_session_dir
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
                if let Some(_stl_path) = self.session.latest_stl_path() {
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
            // Phase navigation: Alt+1 through Alt+5
            (Char('1'), KeyModifiers::ALT) => {
                self.try_switch_phase(Phase::Spec);
                return;
            }
            (Char('2'), KeyModifiers::ALT) => {
                self.try_switch_phase(Phase::Decompose);
                return;
            }
            (Char('3'), KeyModifiers::ALT) => {
                self.try_switch_phase(Phase::Component);
                return;
            }
            (Char('4'), KeyModifiers::ALT) => {
                self.try_switch_phase(Phase::Assembly);
                return;
            }
            (Char('5'), KeyModifiers::ALT) => {
                self.try_switch_phase(Phase::Refinement);
                return;
            }
            // Component navigation: Ctrl+Left/Right (only in Component phase)
            (Left, KeyModifiers::CONTROL) => {
                if self.phase == Phase::Component {
                    self.component_list.select_prev();
                }
                return;
            }
            (Right, KeyModifiers::CONTROL) => {
                if self.phase == Phase::Component {
                    self.component_list.select_next();
                }
                return;
            }
            (Tab, _) => {
                self.focus = match self.focus {
                    Focus::Input => Focus::ProjectTree,
                    Focus::ProjectTree => Focus::Conversation,
                    Focus::Conversation => Focus::Input,
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
        }
    }

    fn handle_input_key(&mut self, key: crossterm::event::KeyEvent) {
        use KeyCode::*;

        // Convert key event to tui_textarea Input and handle
        let input = tui_textarea::Input::from(key);
        if let Some(text) = self.input_bar.handle_input(input) {
            self.submit_prompt(text);
        }
    }

    fn handle_tree_key(&mut self, key: crossterm::event::KeyEvent) {
        use KeyCode::*;
        match key.code {
            Up | Char('k') => self.project_tree.select_prev(),
            Down | Char('j') => self.project_tree.select_next(),
            Char('e') => {
                // Rename selected item
                if let Some(entry) = self.project_tree.selected_entry() {
                    if entry.project_idx == usize::MAX {
                        return; // Can't rename "+ New Project"
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
                // Delete selected item — prompt for confirmation
                if let Some(entry) = self.project_tree.selected_entry() {
                    if entry.project_idx == usize::MAX {
                        return; // Can't delete "+ New Project"
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
                // Clone the entry to avoid borrow issues
                if let Some(entry) = self.project_tree.selected_entry() {
                    let is_project = entry.is_project;
                    let project_idx = entry.project_idx;
                    let session_name = entry.session_name.clone();

                    if is_project {
                        if project_idx == usize::MAX {
                            // "New Project" entry
                            self.new_project_pending = true;
                            self.conversation.add("system", "Next prompt will create a new project.");
                        } else {
                            // Toggle project expansion
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
                    } else if let Some(ref name) = session_name {
                        // Load session
                        self.load_session(project_idx, name.clone());
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_conversation_key(&mut self, key: crossterm::event::KeyEvent) {
        use KeyCode::*;
        match key.code {
            Up | Char('k') => self.conversation.scroll_up(1),
            Down | Char('j') => self.conversation.scroll_down(1),
            Char('u') => self.conversation.scroll_up(10),
            Char('d') => self.conversation.scroll_down(10),
            _ => {}
        }
    }

    /// Handle pasted text (from bracketed paste / drag-and-drop).
    /// Detects file paths and attaches them; inserts remaining text into input.
    fn handle_paste(&mut self, pasted: String) {
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

    fn submit_prompt(&mut self, text: String) {
        if self.busy != BusyState::Idle {
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
                let dest_dir = self.active_session_dir
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
            self.claude_session_id = None;
            self.active_session_name = None;
            self.active_session_dir = None;
            self.phase_session = None;
            self.phase = Phase::Spec;
            self.layout_config.phase = Phase::Spec;
            self.conversation.add("system", "New session started.");
        }

        // Auto-create session name from first prompt if none active
        if self.active_session_name.is_none() {
            let session_name: String = text.chars()
                .filter(|c| c.is_alphanumeric() || *c == ' ')
                .take(30)
                .collect();
            let session_name = session_name.trim().replace(' ', "_");
            if !session_name.is_empty() {
                // Auto-create session dir under active project or "Untitled"
                let project_path = self.active_project_idx
                    .and_then(|idx| self.projects.get(idx))
                    .map(|p| p.path.clone())
                    .unwrap_or_else(|| storage::project::root_dir().join("Untitled"));
                let session_dir = project_path.join(&session_name);
                self.active_session_name = Some(session_name);
                self.active_session_dir = Some(session_dir);
            }
        }

        // Create PhaseSession if we have a session dir but no phase session yet
        if self.phase_session.is_none() {
            if let Some(ref dir) = self.active_session_dir {
                self.phase_session = Some(PhaseSession::new(
                    dir.clone(),
                    self.build_timeout,
                    self.python_path.clone(),
                ));
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
        self.session.add_user_message(&clean_text);

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
            // For other phases, fall through to existing monolithic path (for now)
            _ => {}
        }

        // Set busy state
        self.busy = BusyState::Thinking;
        self.streaming_text.clear();

        // Clone what we need for the background thread
        let model = self.claude_model.clone();
        let system_prompt = self.claude_system_prompt.clone();
        let session_id = self.claude_session_id.clone();
        let tx = self.bg_tx.clone();
        let stream_tx = self.stream_tx.clone();
        let bg_pid = Arc::clone(&self.bg_pid);

        std::thread::spawn(move || {
            let result = claude::send_prompt(
                &model,
                &system_prompt,
                session_id.as_deref(),
                &clean_text,
                &all_images,
                Some(&stream_tx),
                Some(&bg_pid),
            );
            bg_pid.store(0, Ordering::SeqCst);
            match result {
                Ok((response, new_sid)) => {
                    let _ = tx.send(BackgroundResult::ClaudeResponse {
                        result: Ok(response),
                        session_id: new_sid.or(session_id),
                    });
                }
                Err(e) => {
                    let _ = tx.send(BackgroundResult::ClaudeResponse {
                        result: Err(e),
                        session_id: None,
                    });
                }
            }
        });
    }

    fn send_spec_prompt(&mut self, text: &str, images: Vec<PathBuf>) {
        self.busy = BusyState::Thinking;
        self.streaming_text.clear();

        let model = self.claude_model.clone();
        let session_id = self.claude_session_id.clone();
        let tx = self.bg_tx.clone();
        let stream_tx = self.stream_tx.clone();
        let bg_pid = Arc::clone(&self.bg_pid);
        let prompt = text.to_string();

        std::thread::spawn(move || {
            let result = claude::send_with_phase_prompt(
                &model,
                "spec",
                session_id.as_deref(),
                &prompt,
                &images,
                Some(&stream_tx),
                Some(&bg_pid),
            );
            bg_pid.store(0, Ordering::SeqCst);
            match result {
                Ok((response, new_sid)) => {
                    let _ = tx.send(BackgroundResult::ClaudeResponse {
                        result: Ok(response),
                        session_id: new_sid.or(session_id),
                    });
                }
                Err(e) => {
                    let _ = tx.send(BackgroundResult::ClaudeResponse {
                        result: Err(e),
                        session_id: None,
                    });
                }
            }
        });
    }

    fn send_decompose_prompt(&mut self, text: &str) {
        self.busy = BusyState::Thinking;
        self.streaming_text.clear();

        let model = self.claude_model.clone();
        let session_id = self.claude_session_id.clone();
        let tx = self.bg_tx.clone();
        let stream_tx = self.stream_tx.clone();
        let bg_pid = Arc::clone(&self.bg_pid);
        let prompt = text.to_string();

        std::thread::spawn(move || {
            let result = claude::send_with_phase_prompt(
                &model,
                "decompose",
                session_id.as_deref(),
                &prompt,
                &[],
                Some(&stream_tx),
                Some(&bg_pid),
            );
            bg_pid.store(0, Ordering::SeqCst);
            match result {
                Ok((response, new_sid)) => {
                    let _ = tx.send(BackgroundResult::ClaudeResponse {
                        result: Ok(response),
                        session_id: new_sid.or(session_id),
                    });
                }
                Err(e) => {
                    let _ = tx.send(BackgroundResult::ClaudeResponse {
                        result: Err(e),
                        session_id: None,
                    });
                }
            }
        });
    }

    fn handle_spec_response(&mut self, response: &str) {
        // Update spec panel with the running conversation
        let mut spec_content = self.spec_panel.content().to_string();

        // Check for SPEC_COMPLETE signal
        if response.contains("SPEC_COMPLETE") {
            self.conversation.add("system", "Specification complete! Building spec.toml...");
            self.conversation.add("system", "Transitioning to Decompose phase. You can review the spec in the right panel.");

            // Transition to Decompose
            self.phase = Phase::Decompose;
            self.layout_config.phase = Phase::Decompose;
            self.claude_session_id = None; // Fresh session for Decompose
            self.save_phase_session();
        } else {
            // Append Claude's response to the spec panel for visibility
            if !spec_content.is_empty() {
                spec_content.push_str("\n\n");
            }
            spec_content.push_str(response);
            self.spec_panel.set_content(&spec_content);
        }
    }

    fn handle_decompose_response(&mut self, response: &str) {
        match parser::parse_toml_response(response) {
            Ok(toml_str) => {
                // Parse the TOML and display components in the tree panel
                self.parse_and_display_components(&toml_str);

                self.conversation.add("system",
                    "Component tree proposed. Type 'approve' to accept, or describe changes.");
            }
            Err(e) => {
                self.conversation.add("system",
                    &format!("Failed to parse component structure: {e}. Please try again."));
            }
        }
    }

    fn parse_and_display_components(&mut self, toml_str: &str) {
        use crate::tui::component_tree::TreeComponent;

        match toml::from_str::<toml::Value>(toml_str) {
            Ok(value) => {
                let mut tree_components = Vec::new();

                if let Some(components) = value.get("components").and_then(|c| c.as_array()) {
                    for comp in components {
                        let id = comp.get("id").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                        let name = comp.get("name").and_then(|v| v.as_str()).unwrap_or(&id).to_string();
                        let assembly_op = comp.get("assembly_op").and_then(|v| v.as_str()).unwrap_or("none").to_string();
                        let depends_on: Vec<String> = comp.get("depends_on")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                            .unwrap_or_default();

                        tree_components.push(TreeComponent { id, name, depends_on, assembly_op });
                    }
                }

                self.component_tree_panel.set_components(&tree_components);

                // Store the raw TOML for later merging into spec
                self.spec_panel.set_content(toml_str);
            }
            Err(e) => {
                self.conversation.add("system", &format!("TOML parse error: {e}"));
            }
        }
    }

    fn approve_decomposition(&mut self) {
        let toml_str = self.spec_panel.content().to_string();
        if toml_str.is_empty() {
            self.conversation.add("system", "No component structure to approve. Ask Claude to decompose first.");
            return;
        }

        // TODO: Merge the component TOML into the spec.toml file
        // TODO: Create component directories using the parsed components
        // For now, just transition to Component phase

        self.conversation.add("system", "Component structure approved! Transitioning to Component phase.");
        self.phase = Phase::Component;
        self.layout_config.phase = Phase::Component;
        self.claude_session_id = None; // Fresh session for Component phase
        self.save_phase_session();
    }

    fn handle_bg_result(&mut self, result: BackgroundResult) {
        match result {
            BackgroundResult::ClaudeResponse { result, session_id } => {
                // Update session_id
                if let Some(sid) = session_id {
                    self.claude_session_id = Some(sid);
                }

                match result {
                    Ok(response) => {
                        // Add assistant message to conversation
                        self.conversation.add("assistant", &response);
                        self.session.add_assistant_message(&response);

                        match self.phase {
                            Phase::Spec => {
                                self.handle_spec_response(&response);
                                self.busy = BusyState::Idle;
                            }
                            Phase::Decompose => {
                                self.handle_decompose_response(&response);
                                self.busy = BusyState::Idle;
                            }
                            Phase::Component => {
                                // Parse response for cadquery code block
                                let parsed = parser::parse_response(&response);
                                if let Some(code_block) = parsed.code {
                                    // Build the component
                                    self.busy = BusyState::Building;
                                    let build_result = self.session.build(&code_block.code, code_block.engine);
                                    self.handle_component_build_result(build_result, code_block.code);
                                } else {
                                    // No code in response — just a conversation message
                                    self.busy = BusyState::Idle;
                                }
                            }
                            Phase::Assembly => {
                                // Assembly responses may contain code to rebuild, or just conversation
                                let parsed = parser::parse_response(&response);
                                if let Some(code_block) = parsed.code {
                                    self.busy = BusyState::Building;
                                    let result = self.session.build(&code_block.code, code_block.engine);
                                    self.handle_build_result(result);
                                } else {
                                    self.busy = BusyState::Idle;
                                }
                            }
                            Phase::Refinement => {
                                // Refinement responses may contain updated code
                                let parsed = parser::parse_response(&response);
                                if let Some(code_block) = parsed.code {
                                    self.busy = BusyState::Building;
                                    let result = self.session.build(&code_block.code, code_block.engine);
                                    self.handle_build_result(result);
                                } else {
                                    self.busy = BusyState::Idle;
                                }
                            }
                            _ => {
                                // Existing monolithic path: parse for code, build
                                let parsed = parser::parse_response(&response);
                                if let Some(code_block) = parsed.code {
                                    self.busy = BusyState::Building;
                                    let result = self.session.build(&code_block.code, code_block.engine);
                                    self.handle_build_result(result);
                                } else {
                                    self.busy = BusyState::Idle;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        self.conversation.add("system", &format!("Claude error: {e}"));
                        self.busy = BusyState::Idle;
                    }
                }
            }
            BackgroundResult::BuildComplete(build_result) => {
                self.handle_build_result(build_result);
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
                self.model_panel.update(meta, stl_path.as_deref(), self.session.iteration());

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

                // Auto-save (legacy session)
                if let Some(ref session_dir) = self.active_session_dir.clone() {
                    let session_name = self.active_session_name.clone().unwrap_or_else(|| "session".to_string());
                    let claude_sid = self.claude_session_id.clone();
                    if let Err(e) = self.session.save_to(session_dir, &session_name, claude_sid.as_deref()) {
                        self.conversation.add("system", &format!("Warning: auto-save failed: {e}"));
                    } else {
                        self.refresh_projects();
                    }
                }
                // Auto-save phase session
                self.save_phase_session();
            }
            python::BuildResult::BuildError(e) | python::BuildResult::SyntaxError(e) => {
                self.conversation.add("system", &format!("Build error: {}", e.error));
            }
            python::BuildResult::Timeout => {
                self.conversation.add("system", "Build timed out.");
            }
        }
        self.busy = BusyState::Idle;
    }

    fn load_session(&mut self, project_idx: usize, session_name: String) {
        if let Some(project) = self.projects.get(project_idx) {
            let session_dir = project.path.join(&session_name);

            // Check if this is a new-format phase session
            let session_json_path = session_dir.join("session.json");
            if session_json_path.exists() {
                if let Ok(json_str) = std::fs::read_to_string(&session_json_path) {
                    if !crate::storage::session::is_legacy_session_json(&json_str) {
                        // New format — load as PhaseSession
                        self.load_phase_session(&session_dir, &session_name, project_idx);
                        return;
                    }
                }
            }

            // Fall through to legacy session loading
            let claude_session_id = if session_json_path.exists() {
                std::fs::read_to_string(&session_json_path)
                    .ok()
                    .and_then(|json| serde_json::from_str::<model_session::SessionData>(&json).ok())
                    .and_then(|data| data.claude_session_id)
            } else {
                None
            };

            match Session::load_from(&session_dir, self.build_timeout, self.python_path.clone()) {
                Ok(loaded_session) => {
                    // Restore conversation pane
                    self.conversation.clear();
                    for msg in &loaded_session.messages {
                        self.conversation.add(&msg.role, &msg.content);
                    }

                    // Update model panel and viewer
                    let stl_path = loaded_session.latest_stl_path();
                    if let Some(ref meta) = loaded_session.current_metadata {
                        self.model_panel.update(meta, stl_path.as_deref(), loaded_session.iteration());
                    } else {
                        self.model_panel.clear();
                    }
                    if let Some(ref src) = stl_path {
                        if let Err(e) = self.viewer.update_working_stl(src) {
                            self.conversation.add("system", &format!("Warning: {e}"));
                        }
                        if !self.viewer.is_running() {
                            let _ = self.viewer.show();
                        }
                    }

                    self.session = loaded_session;
                    self.claude_session_id = claude_session_id;
                    self.active_project_idx = Some(project_idx);
                    self.active_session_name = Some(session_name.clone());
                    self.active_session_dir = Some(session_dir);
                    self.phase_session = None; // Legacy session, no phase session

                    // Update project tree selection
                    self.project_tree.active_project = Some(project_idx);
                    self.project_tree.active_session = Some(session_name.clone());
                    let projects = self.projects.clone();
                    self.project_tree.refresh(&projects);

                    self.conversation.add("system", &format!("Loaded session '{session_name}'."));
                    self.focus = Focus::Input;
                }
                Err(e) => {
                    self.conversation.add("system", &format!("Failed to load session: {e}"));
                }
            }
        }
    }

    fn load_phase_session(&mut self, session_dir: &Path, session_name: &str, project_idx: usize) {
        match PhaseSession::load(session_dir, self.build_timeout, self.python_path.clone()) {
            Ok(ps) => {
                // Restore phase
                self.phase = ps.phase;
                self.layout_config.phase = ps.phase;

                // Restore conversation (show a summary)
                self.conversation.clear();
                self.conversation.add("system", &format!(
                    "Resumed session '{}' in {} phase.", session_name, ps.phase.label()
                ));

                // Restore viewer with working.stl if it exists
                let working_stl = session_dir.join("working.stl");
                if working_stl.exists() {
                    if let Err(e) = self.viewer.update_working_stl(&working_stl) {
                        self.conversation.add("system", &format!("Warning: {e}"));
                    }
                    if !self.viewer.is_running() {
                        let _ = self.viewer.show();
                    }
                }

                // Crash recovery hint (Task 26)
                if ps.phase == Phase::Component {
                    self.conversation.add("system",
                        "Tip: If the last build was interrupted, type 'undo' to restore the previous state.");
                }

                // Store session state
                self.active_project_idx = Some(project_idx);
                self.active_session_name = Some(session_name.to_string());
                self.active_session_dir = Some(session_dir.to_path_buf());
                self.phase_session = Some(ps);

                // Update project tree selection
                self.project_tree.active_project = Some(project_idx);
                self.project_tree.active_session = Some(session_name.to_string());
                self.refresh_projects();

                self.focus = Focus::Input;
            }
            Err(e) => {
                self.conversation.add("system", &format!("Failed to load session: {e}"));
            }
        }
    }

    fn open_project(&mut self, project_idx: usize) {
        if let Some(project) = self.projects.get(project_idx) {
            // Set active project so new prompts land here
            self.active_project_idx = Some(project_idx);
            self.active_session_name = None;
            self.active_session_dir = None;
            self.claude_session_id = None;

            // Reset session state
            self.session.reset();
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
                        storage::session::SessionStatus::Ok { iteration_count, modified } => {
                            let date = modified.split('T').next().unwrap_or(&modified);
                            format!("  {sname}  ({iteration_count} iterations, {date})")
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

            self.focus = Focus::Input;
        }
    }

    fn refresh_projects(&mut self) {
        self.projects = storage::project::list_projects().unwrap_or_default();
        let projects = self.projects.clone();
        self.project_tree.refresh(&projects);
    }

    /// Kill any running Claude subprocess on app exit.
    fn cleanup(&self) {
        let pid = self.bg_pid.load(Ordering::SeqCst);
        if pid != 0 {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
    }

    /// Save the PhaseSession to disk (auto-save helper).
    /// Syncs the current phase into the PhaseSession and writes session.json.
    fn save_phase_session(&mut self) {
        if let Some(ref mut ps) = self.phase_session {
            ps.phase = self.phase;
            if let Err(e) = ps.save() {
                self.conversation.add("system", &format!("Warning: auto-save failed: {e}"));
            }
        }
    }

    /// Attempt to switch to a different phase.
    /// For now, allows free navigation between phases.
    /// Prerequisite validation will be added when phase flows are implemented.
    fn try_switch_phase(&mut self, target: Phase) {
        if target == self.phase {
            return; // Already here
        }
        self.phase = target;
        self.layout_config.phase = target;
        // Add system message about phase change
        self.conversation.add("system", &format!("Switched to {} phase", target.label()));
        self.save_phase_session();
    }

    // -- Phase-specific input handlers --

    #[allow(dead_code)]
    fn handle_spec_input(&mut self, _text: &str) {
        // Will be implemented in Chunk 6: send spec prompt, parse spec.toml response
    }

    #[allow(dead_code)]
    fn handle_decompose_input(&mut self, _text: &str) {
        // Will be implemented in Chunk 6: send decompose prompt, parse component tree
    }

    fn handle_assembly_input(&mut self, text: &str) {
        let trimmed = text.trim().to_lowercase();
        if trimmed == "approve" || trimmed == "ok" || trimmed == "done" {
            // Approve assembly, move to Refinement
            self.conversation.add("system", "Assembly approved! Transitioning to Refinement phase.");
            self.phase = Phase::Refinement;
            self.layout_config.phase = Phase::Refinement;
            self.claude_session_id = None;
            self.save_phase_session();
        } else {
            // Send feedback about assembly to Claude
            self.send_assembly_feedback(text);
        }
    }

    fn send_assembly_feedback(&mut self, text: &str) {
        self.busy = BusyState::Thinking;
        self.streaming_text.clear();

        let model = self.claude_model.clone();
        let session_id = self.claude_session_id.clone();
        let tx = self.bg_tx.clone();
        let stream_tx = self.stream_tx.clone();
        let bg_pid = Arc::clone(&self.bg_pid);
        let prompt = text.to_string();

        std::thread::spawn(move || {
            let result = claude::send_with_phase_prompt(
                &model,
                "assembly",
                session_id.as_deref(),
                &prompt,
                &[],
                Some(&stream_tx),
                Some(&bg_pid),
            );
            bg_pid.store(0, Ordering::SeqCst);
            match result {
                Ok((response, new_sid)) => {
                    let _ = tx.send(BackgroundResult::ClaudeResponse {
                        result: Ok(response),
                        session_id: new_sid.or(session_id),
                    });
                }
                Err(e) => {
                    let _ = tx.send(BackgroundResult::ClaudeResponse {
                        result: Err(e),
                        session_id: None,
                    });
                }
            }
        });
    }

    fn handle_refinement_input(&mut self, text: &str) {
        let trimmed = text.trim().to_lowercase();

        if trimmed.starts_with("set ") {
            // Parameter edit mode: "set PARAM_NAME value"
            // e.g., "set OUTER_DIAMETER 42.0"
            self.handle_param_edit(text);
        } else if trimmed == "export" {
            self.handle_export();
        } else {
            // Text feedback — scoped Claude call for one component
            self.send_refinement_feedback(text);
        }
    }

    fn handle_param_edit(&mut self, text: &str) {
        // Parse "set PARAM_NAME value" format
        let parts: Vec<&str> = text.trim().splitn(3, ' ').collect();
        if parts.len() < 3 {
            self.conversation.add("system", "Usage: set PARAM_NAME value (e.g., 'set OUTER_DIAMETER 42.0')");
            return;
        }

        let param_name = parts[1].to_uppercase();
        let value: f64 = match parts[2].parse() {
            Ok(v) => v,
            Err(_) => {
                self.conversation.add("system", &format!("Invalid number: {}", parts[2]));
                return;
            }
        };

        self.conversation.add("system", &format!(
            "Parameter edit: {} = {} (zero-Claude rebuild)", param_name, value
        ));

        // In the future, this will:
        // 1. Write params JSON
        // 2. Call python::paramset()
        // 3. Rebuild assembly
        // 4. Update viewer
        // For now, just acknowledge the change
        self.conversation.add("system", "Parameter edit acknowledged. Full paramset integration pending PhaseSession wiring.");
    }

    fn send_refinement_feedback(&mut self, text: &str) {
        self.busy = BusyState::Thinking;
        self.streaming_text.clear();

        let model = self.claude_model.clone();
        let session_id = self.claude_session_id.clone();
        let tx = self.bg_tx.clone();
        let stream_tx = self.stream_tx.clone();
        let bg_pid = Arc::clone(&self.bg_pid);
        let prompt = text.to_string();

        std::thread::spawn(move || {
            let result = claude::send_with_phase_prompt(
                &model,
                "refinement",
                session_id.as_deref(),
                &prompt,
                &[],
                Some(&stream_tx),
                Some(&bg_pid),
            );
            bg_pid.store(0, Ordering::SeqCst);
            match result {
                Ok((response, new_sid)) => {
                    let _ = tx.send(BackgroundResult::ClaudeResponse {
                        result: Ok(response),
                        session_id: new_sid.or(session_id),
                    });
                }
                Err(e) => {
                    let _ = tx.send(BackgroundResult::ClaudeResponse {
                        result: Err(e),
                        session_id: None,
                    });
                }
            }
        });
    }

    fn handle_export(&mut self) {
        if let Some(ref session_dir) = self.active_session_dir {
            if let Some(stl_path) = self.session.latest_stl_path() {
                let export_stl = session_dir.join("export.stl");
                match std::fs::copy(&stl_path, &export_stl) {
                    Ok(_) => {
                        self.conversation.add("system", &format!("Exported to {}", export_stl.display()));
                    }
                    Err(e) => {
                        self.conversation.add("system", &format!("Export failed: {e}"));
                    }
                }
            } else {
                self.conversation.add("system", "No model to export.");
            }
        } else {
            self.conversation.add("system", "No active session directory for export.");
        }
    }

    // -- Component phase methods --

    /// Start building the current component by sending an initial prompt to Claude.
    fn start_component_build(&mut self) {
        let idx = self.component_list.selected();
        let component_name = self.component_list.selected_id()
            .unwrap_or("unknown")
            .to_string();
        let total = self.component_list.len();
        let component_info = format!(
            "Generate CadQuery code for component {}/{}: '{}'.",
            idx + 1, total, component_name
        );
        self.send_component_prompt(&component_info, vec![]);
    }

    fn send_component_prompt(&mut self, text: &str, images: Vec<PathBuf>) {
        self.busy = BusyState::Thinking;
        self.streaming_text.clear();

        let model = self.claude_model.clone();
        let session_id = self.claude_session_id.clone();
        let tx = self.bg_tx.clone();
        let stream_tx = self.stream_tx.clone();
        let bg_pid = Arc::clone(&self.bg_pid);
        let prompt = text.to_string();

        std::thread::spawn(move || {
            let result = claude::send_with_phase_prompt(
                &model,
                "component",
                session_id.as_deref(),
                &prompt,
                &images,
                Some(&stream_tx),
                Some(&bg_pid),
            );
            bg_pid.store(0, Ordering::SeqCst);
            match result {
                Ok((response, new_sid)) => {
                    let _ = tx.send(BackgroundResult::ClaudeResponse {
                        result: Ok(response),
                        session_id: new_sid.or(session_id),
                    });
                }
                Err(e) => {
                    let _ = tx.send(BackgroundResult::ClaudeResponse {
                        result: Err(e),
                        session_id: None,
                    });
                }
            }
        });
    }

    fn send_component_feedback(&mut self, text: &str, images: Vec<PathBuf>) {
        self.busy = BusyState::Thinking;
        self.streaming_text.clear();

        let model = self.claude_model.clone();
        let session_id = self.claude_session_id.clone();
        let tx = self.bg_tx.clone();
        let stream_tx = self.stream_tx.clone();
        let bg_pid = Arc::clone(&self.bg_pid);
        let prompt = text.to_string();

        // Use "component" prompt for initial generation, "refinement" for feedback
        // If we already have code for this component, use refinement
        let phase_prompt = if self.session.current_code.is_some() {
            "refinement"
        } else {
            "component"
        };
        let phase_name = phase_prompt.to_string();

        std::thread::spawn(move || {
            let result = claude::send_with_phase_prompt(
                &model,
                &phase_name,
                session_id.as_deref(),
                &prompt,
                &images,
                Some(&stream_tx),
                Some(&bg_pid),
            );
            bg_pid.store(0, Ordering::SeqCst);
            match result {
                Ok((response, new_sid)) => {
                    let _ = tx.send(BackgroundResult::ClaudeResponse {
                        result: Ok(response),
                        session_id: new_sid.or(session_id),
                    });
                }
                Err(e) => {
                    let _ = tx.send(BackgroundResult::ClaudeResponse {
                        result: Err(e),
                        session_id: None,
                    });
                }
            }
        });
    }

    fn handle_component_build_result(&mut self, build_result: python::BuildResult, _code: String) {
        match build_result {
            python::BuildResult::Success(ref meta) => {
                self.conversation.add("system", &format!(
                    "Component built: {:.1} x {:.1} x {:.1} mm",
                    meta.dimensions.x, meta.dimensions.y, meta.dimensions.z
                ));

                // Update model panel
                let stl_path = self.session.latest_stl_path();
                self.model_panel.update(meta, stl_path.as_deref(), self.session.iteration());

                // Update viewer
                if let Some(ref src) = stl_path {
                    let _ = self.viewer.update_working_stl(src);
                    if !self.viewer.is_running() {
                        let _ = self.viewer.show();
                    }
                }

                self.conversation.add("system", "Type 'approve' to accept, or describe changes.");
                self.save_phase_session();
            }
            python::BuildResult::BuildError(ref e) | python::BuildResult::SyntaxError(ref e) => {
                self.conversation.add("system", &format!("Build error: {}", e.error));
            }
            python::BuildResult::Timeout => {
                self.conversation.add("system", "Build timed out.");
            }
        }
        self.busy = BusyState::Idle;
    }

    fn approve_current_component(&mut self) {
        let total = self.component_list.len();
        let current = self.component_list.selected();

        if total == 0 {
            self.conversation.add("system", "No components to approve.");
            return;
        }

        // Trigger progressive assembly note if we have 2+ approved components
        // (current is 0-indexed, so current >= 1 means at least 2 approved)
        if current >= 1 {
            self.conversation.add("system", "Progressive assembly updated.");
        }

        if current + 1 < total {
            // Move to next component
            self.component_list.select_next();
            self.claude_session_id = None; // Fresh session for next component
            self.conversation.add("system", &format!(
                "Component approved! Moving to component {}/{}.",
                current + 2, total
            ));
            // Auto-start build for next component
            // self.start_component_build();
            self.save_phase_session();
        } else {
            // Last component — transition to Assembly
            self.conversation.add("system", "All components approved! Transitioning to Assembly phase.");
            self.phase = Phase::Assembly;
            self.layout_config.phase = Phase::Assembly;
            self.claude_session_id = None;
            self.save_phase_session();
        }
    }

    fn undo_component(&mut self) {
        if self.session.undo() {
            self.conversation.add("system", "Undid last component iteration.");
            // Update model panel and viewer
            if let Some(ref meta) = self.session.current_metadata {
                let stl_path = self.session.latest_stl_path();
                self.model_panel.update(meta, stl_path.as_deref(), self.session.iteration());
                if let Some(ref src) = stl_path {
                    let _ = self.viewer.update_working_stl(src);
                }
            }
        } else {
            self.conversation.add("system", "Nothing to undo.");
        }
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
    let (bg_tx, bg_rx) = mpsc::channel::<BackgroundResult>();
    let (stream_tx, stream_rx) = mpsc::channel::<String>();
    let mut pt = ProjectTreePane::new();
    pt.refresh(&projects);
    App {
        focus: Focus::Input,
        busy: BusyState::Idle,
        layout_config: LayoutConfig::default(),
        phase: Phase::Spec,
        project_tree: pt,
        conversation: ConversationPane::new(),
        model_panel: ModelPanel::new(),
        input_bar: InputBar::new(),
        spec_panel: SpecPanel::new(),
        component_tree_panel: ComponentTreePanel::new(),
        component_list: ComponentListPanel::new(),
        session: Session::new(60, python_path.clone()),
        phase_session: None,
        claude_model: config.claude.model.clone(),
        claude_system_prompt: String::new(),
        claude_session_id: None,
        viewer: Viewer::new(&config.viewer.command),
        pending_images: Vec::new(),
        python_path,
        bg_tx,
        bg_rx,
        stream_tx,
        stream_rx,
        bg_pid: Arc::new(AtomicU32::new(0)),
        projects,
        active_project_idx: None,
        active_session_name: None,
        active_session_dir: None,
        should_quit: false,
        spinner_frame: 0,
        last_ctrl_c: None,
        new_session_pending: false,
        new_project_pending: false,
        export_pending: false,
        rename_pending: None,
        delete_pending: None,
        save_part_pending: false,
        streaming_text: String::new(),
        build_timeout: 60,
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

    // Enable bracketed paste for drag-and-drop file detection
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::EnableBracketedPaste);

    // Run event loop
    let result = run_event_loop(&mut terminal, &mut app);

    // Kill any running Claude subprocess before exiting
    app.cleanup();

    // Disable bracketed paste before restoring terminal
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableBracketedPaste);

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
    loop {
        terminal.draw(|f| app.render(f))?;

        // Drain streaming text chunks from Claude
        let mut got_stream = false;
        while let Ok(chunk) = app.stream_rx.try_recv() {
            app.streaming_text.push_str(&chunk);
            got_stream = true;
        }
        if got_stream {
            app.conversation.scroll_to_bottom();
        }

        // Check background channel (final result)
        if let Ok(result) = app.bg_rx.try_recv() {
            app.streaming_text.clear();
            app.handle_bg_result(result);
        }

        // Poll for events with 50ms timeout
        if crossterm::event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    app.handle_key(key);
                }
                Event::Paste(text) => {
                    app.handle_paste(text);
                }
                _ => {}
            }
        }

        // Advance spinner
        if app.busy != BusyState::Idle {
            app.spinner_frame = app.spinner_frame.wrapping_add(1);
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
