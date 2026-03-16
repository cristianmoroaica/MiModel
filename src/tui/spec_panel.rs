//! Spec preview panel — displays the evolving spec.toml content during the Spec phase.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

pub struct SpecPanel {
    content: String,
    scroll: u16,
}

impl SpecPanel {
    pub fn new() -> Self {
        Self { content: String::new(), scroll: 0 }
    }

    pub fn set_content(&mut self, content: &str) {
        self.content = content.to_string();
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn scroll_offset(&self) -> u16 {
        self.scroll
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        self.scroll += 1;
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let border_style = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let block = Block::default()
            .title(" Spec ")
            .borders(Borders::ALL)
            .border_style(border_style);

        let paragraph = Paragraph::new(self.content.as_str())
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0));
        frame.render_widget(paragraph, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_is_empty() {
        let panel = SpecPanel::new();
        assert!(panel.content().is_empty());
    }

    #[test]
    fn test_set_and_get_content() {
        let mut panel = SpecPanel::new();
        panel.set_content("[model]\nname = \"Test\"");
        assert_eq!(panel.content(), "[model]\nname = \"Test\"");
    }

    #[test]
    fn test_scroll() {
        let mut panel = SpecPanel::new();
        panel.scroll_down();
        panel.scroll_down();
        assert_eq!(panel.scroll_offset(), 2);
        panel.scroll_up();
        assert_eq!(panel.scroll_offset(), 1);
        panel.scroll_up();
        panel.scroll_up(); // Should not go below 0
        assert_eq!(panel.scroll_offset(), 0);
    }
}
