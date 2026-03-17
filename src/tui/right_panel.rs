//! Right panel — tabbed container for Spec, Refs, and Model views.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Tabs, Wrap};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RightTab {
    Spec,
    Refs,
    Model,
}

impl RightTab {
    fn index(self) -> usize {
        match self {
            RightTab::Spec => 0,
            RightTab::Refs => 1,
            RightTab::Model => 2,
        }
    }

    fn from_index(idx: usize) -> Self {
        match idx {
            0 => RightTab::Spec,
            1 => RightTab::Refs,
            _ => RightTab::Model,
        }
    }

    const COUNT: usize = 3;
}

pub struct RightPanel {
    pub active_tab: RightTab,
    pub scroll_offset: u16,
    pub spec_content: String,
    pub refs_content: String,
    pub model_content: String,
}

impl RightPanel {
    pub fn new() -> Self {
        Self {
            active_tab: RightTab::Spec,
            scroll_offset: 0,
            spec_content: String::new(),
            refs_content: String::new(),
            model_content: String::new(),
        }
    }

    pub fn set_spec(&mut self, content: &str) {
        self.spec_content = content.to_string();
    }

    pub fn set_refs(&mut self, content: &str) {
        self.refs_content = content.to_string();
    }

    pub fn set_model(&mut self, content: &str) {
        self.model_content = content.to_string();
    }

    pub fn next_tab(&mut self) {
        let next = (self.active_tab.index() + 1) % RightTab::COUNT;
        self.active_tab = RightTab::from_index(next);
        self.scroll_offset = 0;
    }

    pub fn prev_tab(&mut self) {
        let idx = self.active_tab.index();
        let prev = if idx == 0 { RightTab::COUNT - 1 } else { idx - 1 };
        self.active_tab = RightTab::from_index(prev);
        self.scroll_offset = 0;
    }

    pub fn scroll_up(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    pub fn scroll_down(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_add(amount);
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, focused: bool) {
        if area.height < 2 {
            return;
        }

        let border_style = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        // Split: 1 line for tab bar (inside block border means we need the outer block first),
        // then content. We render the whole thing inside a bordered block.
        // Layout: top border + 1 line tabs + content + bottom border.
        // Use a block for the outer frame and split the inner area.
        let outer_block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner = outer_block.inner(area);
        frame.render_widget(outer_block, area);

        if inner.height < 2 {
            return;
        }

        // Split inner area: first line = tab headers, rest = content
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(inner);

        let tab_area = chunks[0];
        let content_area = chunks[1];

        // Tab titles
        let titles: Vec<Line> = vec![
            Line::from(" Spec "),
            Line::from(" Refs "),
            Line::from(" Model "),
        ];

        let highlight_style = if focused {
            Style::default().fg(Color::Black).bg(Color::Cyan).bold()
        } else {
            Style::default().fg(Color::Black).bg(Color::DarkGray)
        };

        let tabs = Tabs::new(titles)
            .select(self.active_tab.index())
            .highlight_style(highlight_style)
            .divider("|");

        frame.render_widget(tabs, tab_area);

        // Active tab content
        let content = match self.active_tab {
            RightTab::Spec => {
                if self.spec_content.is_empty() {
                    "No spec content yet."
                } else {
                    &self.spec_content
                }
            }
            RightTab::Refs => {
                if self.refs_content.is_empty() {
                    "No references loaded. Use /ref <name> to load."
                } else {
                    &self.refs_content
                }
            }
            RightTab::Model => {
                if self.model_content.is_empty() {
                    "No model built yet."
                } else {
                    &self.model_content
                }
            }
        };

        let paragraph = Paragraph::new(content)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));

        frame.render_widget(paragraph, content_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_defaults() {
        let panel = RightPanel::new();
        assert_eq!(panel.active_tab, RightTab::Spec);
        assert_eq!(panel.scroll_offset, 0);
        assert!(panel.spec_content.is_empty());
        assert!(panel.refs_content.is_empty());
        assert!(panel.model_content.is_empty());
    }

    #[test]
    fn test_set_content() {
        let mut panel = RightPanel::new();
        panel.set_spec("spec text");
        panel.set_refs("refs text");
        panel.set_model("model text");
        assert_eq!(panel.spec_content, "spec text");
        assert_eq!(panel.refs_content, "refs text");
        assert_eq!(panel.model_content, "model text");
    }

    #[test]
    fn test_next_tab_cycles() {
        let mut panel = RightPanel::new();
        assert_eq!(panel.active_tab, RightTab::Spec);
        panel.next_tab();
        assert_eq!(panel.active_tab, RightTab::Refs);
        panel.next_tab();
        assert_eq!(panel.active_tab, RightTab::Model);
        panel.next_tab();
        assert_eq!(panel.active_tab, RightTab::Spec);
    }

    #[test]
    fn test_prev_tab_cycles() {
        let mut panel = RightPanel::new();
        panel.prev_tab();
        assert_eq!(panel.active_tab, RightTab::Model);
        panel.prev_tab();
        assert_eq!(panel.active_tab, RightTab::Refs);
        panel.prev_tab();
        assert_eq!(panel.active_tab, RightTab::Spec);
    }

    #[test]
    fn test_tab_switch_resets_scroll() {
        let mut panel = RightPanel::new();
        panel.scroll_down(5);
        assert_eq!(panel.scroll_offset, 5);
        panel.next_tab();
        assert_eq!(panel.scroll_offset, 0);
    }

    #[test]
    fn test_scroll_up_clamps_at_zero() {
        let mut panel = RightPanel::new();
        panel.scroll_up(10); // Should not underflow
        assert_eq!(panel.scroll_offset, 0);
        panel.scroll_down(3);
        panel.scroll_up(10);
        assert_eq!(panel.scroll_offset, 0);
    }

    #[test]
    fn test_scroll_down() {
        let mut panel = RightPanel::new();
        panel.scroll_down(3);
        assert_eq!(panel.scroll_offset, 3);
        panel.scroll_down(2);
        assert_eq!(panel.scroll_offset, 5);
    }
}
