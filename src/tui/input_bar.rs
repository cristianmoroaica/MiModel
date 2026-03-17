//! Input bar — wraps tui-textarea with submit (Ctrl+Enter) and history.

use tui_textarea::{Input, Key, TextArea};
use ratatui::style::{Color, Style};

pub struct InputBar<'a> {
    pub textarea: TextArea<'a>,
    history: Vec<String>,
    history_pos: Option<usize>,
}

impl<'a> InputBar<'a> {
    pub fn new() -> Self {
        let mut textarea = TextArea::default();
        textarea.set_cursor_line_style(Style::default());
        textarea.set_placeholder_text("Type what you want to build...");
        textarea.set_block(Self::make_block());
        Self {
            textarea,
            history: Vec::new(),
            history_pos: None,
        }
    }

    fn make_block() -> ratatui::widgets::Block<'a> {
        ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(49, 50, 68)))
            .title(" Input ")
            .title_style(Style::default().fg(Color::Rgb(147, 153, 178)))
    }

    fn reset_textarea(&mut self) {
        self.textarea = TextArea::default();
        self.textarea.set_cursor_line_style(Style::default());
        self.textarea.set_placeholder_text("Type what you want to build...");
        self.textarea.set_block(Self::make_block());
    }

    fn set_textarea_content(&mut self, text: String) {
        // Split on actual newlines so multi-line text is stored correctly.
        let lines: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
        self.textarea = TextArea::new(lines);
        self.textarea.set_cursor_line_style(Style::default());
        self.textarea.set_block(Self::make_block());
    }

    /// Set the placeholder text shown when the textarea is empty.
    pub fn set_placeholder(&mut self, placeholder: &str) {
        self.textarea.set_placeholder_text(placeholder);
    }

    /// Handle input event. Returns Some(text) if user submitted (Enter).
    /// Use backslash + Enter for newline continuation.
    pub fn handle_input(&mut self, input: Input) -> Option<String> {
        match input {
            // Enter = submit (unless line ends with \ for continuation)
            Input { key: Key::Enter, ctrl: false, alt: false, .. } => {
                let current = self.textarea.lines().join("\n");
                if current.ends_with('\\') {
                    // Continuation: strip trailing backslash, add an actual newline.
                    // trim_end_matches strips ALL trailing backslashes; we only want one.
                    let trimmed = &current[..current.len() - 1];
                    self.set_textarea_content(format!("{trimmed}\n"));
                    // Move cursor to the end of the last line.
                    self.textarea.move_cursor(tui_textarea::CursorMove::Bottom);
                    self.textarea.move_cursor(tui_textarea::CursorMove::End);
                    None
                } else {
                    let text = current.trim().to_string();
                    if !text.is_empty() {
                        self.history.push(text.clone());
                        self.history_pos = None;
                    }
                    self.reset_textarea();
                    if text.is_empty() { None } else { Some(text) }
                }
            }
            // Up arrow with empty input = history back
            Input { key: Key::Up, .. } if self.textarea.lines() == [""] => {
                if !self.history.is_empty() {
                    let pos = match self.history_pos {
                        Some(p) if p > 0 => p - 1,
                        None => self.history.len() - 1,
                        Some(p) => p,
                    };
                    self.history_pos = Some(pos);
                    let text = self.history[pos].clone();
                    self.set_textarea_content(text);
                }
                None
            }
            // Down arrow with history active = history forward
            Input { key: Key::Down, .. } if self.history_pos.is_some() => {
                let pos = self.history_pos.unwrap() + 1;
                if pos < self.history.len() {
                    self.history_pos = Some(pos);
                    let text = self.history[pos].clone();
                    self.set_textarea_content(text);
                } else {
                    self.history_pos = None;
                    self.reset_textarea();
                }
                None
            }
            // Everything else: pass to tui-textarea
            input => {
                self.textarea.input(input);
                None
            }
        }
    }

    /// Get the current input text (for checking if empty, etc.).
    pub fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Set a prefix badge (e.g. "[2 images]").
    pub fn set_badge(&mut self, badge: &str) {
        let title = if badge.is_empty() {
            " Input ".to_string()
        } else {
            format!(" Input · {badge} ")
        };
        self.textarea.set_block(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(Style::default().fg(Color::Rgb(49, 50, 68)))
                .title(title)
                .title_style(Style::default().fg(Color::Rgb(147, 153, 178)))
        );
    }

    /// Set border highlight when focused.
    pub fn set_focused(&mut self, focused: bool) {
        let border_color = if focused {
            Color::Rgb(137, 180, 250)
        } else {
            Color::Rgb(49, 50, 68)
        };
        self.textarea.set_block(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(Style::default().fg(border_color))
                .title(" Input ")
                .title_style(Style::default().fg(Color::Rgb(147, 153, 178)))
        );
    }
}
