//! Conversation pane — scrollable styled message list.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

#[derive(Debug, Clone)]
pub struct ConversationEntry {
    pub role: String,    // "user", "assistant", "system"
    pub content: String,
}

pub struct ConversationPane {
    pub entries: Vec<ConversationEntry>,
    pub scroll_offset: u16,
    pub auto_scroll: bool,
}

impl ConversationPane {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            scroll_offset: 0,
            auto_scroll: true,
        }
    }

    pub fn add(&mut self, role: &str, content: &str) {
        self.entries.push(ConversationEntry {
            role: role.to_string(),
            content: content.to_string(),
        });
        if self.auto_scroll {
            self.scroll_to_bottom();
        }
    }

    pub fn scroll_up(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
        self.auto_scroll = false;
    }

    pub fn scroll_down(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_add(amount);
        // max_scroll is clamped in render(), so offset > content re-enables auto-scroll
    }

    /// Scroll by page (visible height).
    pub fn page_up(&mut self, visible_height: u16) {
        self.scroll_up(visible_height.saturating_sub(2));
    }

    pub fn page_down(&mut self, visible_height: u16) {
        self.scroll_down(visible_height.saturating_sub(2));
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = u16::MAX;
        self.auto_scroll = true;
    }

    /// Clamp scroll offset to actual content height. Call after render to keep
    /// the offset in range so scroll_up() works correctly from a real position.
    pub fn clamp_scroll(&mut self, max_scroll: u16) {
        self.scroll_offset = self.scroll_offset.min(max_scroll);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.scroll_offset = 0;
    }

    /// Render the conversation into the given area. Returns max_scroll for clamping.
    pub fn render(&self, frame: &mut Frame, area: Rect, focused: bool) -> u16 {
        let border_color = if focused { Color::Cyan } else { Color::DarkGray };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(" Conversation ");

        // Build styled text
        let mut lines: Vec<Line> = Vec::new();
        for entry in &self.entries {
            let (prefix, color) = match entry.role.as_str() {
                "user" => ("you: ", Color::Green),
                "assistant" => ("claude: ", Color::Magenta),
                _ => ("", Color::DarkGray),
            };

            lines.push(Line::from(vec![
                Span::styled(prefix, Style::default().fg(color).bold()),
            ]));
            for line in entry.content.lines() {
                lines.push(Line::from(vec![
                    Span::raw(format!("  {line}")),
                ]));
            }
            lines.push(Line::raw("")); // blank separator
        }

        let text = Text::from(lines);

        // Compute wrapped line count to get accurate scroll range.
        // Inner width = area minus borders (1 each side).
        let inner_width = area.width.saturating_sub(2).max(1) as usize;
        let mut wrapped_lines: u16 = 0;
        for line in &text.lines {
            let line_width: usize = line.width();
            if line_width == 0 {
                wrapped_lines += 1;
            } else {
                wrapped_lines += ((line_width as f64) / (inner_width as f64)).ceil() as u16;
            }
        }

        let visible = area.height.saturating_sub(2); // minus borders
        let max_scroll = wrapped_lines.saturating_sub(visible);
        let scroll = self.scroll_offset.min(max_scroll);

        let paragraph = Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0));

        frame.render_widget(paragraph, area);
        max_scroll
    }
}
