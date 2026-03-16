use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use crate::component::ComponentStatus;

pub fn status_badge(status: ComponentStatus) -> &'static str {
    match status {
        ComponentStatus::Pending => "○",
        ComponentStatus::Building => "⋯",
        ComponentStatus::Reviewing => "◎",
        ComponentStatus::Approved => "✓",
        ComponentStatus::Error => "✗",
    }
}

fn status_color(status: ComponentStatus) -> Color {
    match status {
        ComponentStatus::Pending => Color::DarkGray,
        ComponentStatus::Building => Color::Yellow,
        ComponentStatus::Reviewing => Color::Cyan,
        ComponentStatus::Approved => Color::Green,
        ComponentStatus::Error => Color::Red,
    }
}

struct ComponentItem {
    id: String,
    name: String,
    status: ComponentStatus,
}

pub struct ComponentListPanel {
    items: Vec<ComponentItem>,
    state: ListState,
}

impl ComponentListPanel {
    pub fn new() -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self { items: Vec::new(), state }
    }

    pub fn set_items(&mut self, items: &[(String, String, ComponentStatus)]) {
        self.items = items.iter().map(|(id, name, status)| {
            ComponentItem { id: id.clone(), name: name.clone(), status: *status }
        }).collect();
        if !self.items.is_empty() {
            self.state.select(Some(0));
        }
    }

    pub fn len(&self) -> usize { self.items.len() }

    pub fn selected(&self) -> usize {
        self.state.selected().unwrap_or(0)
    }

    pub fn selected_id(&self) -> Option<&str> {
        let idx = self.selected();
        self.items.get(idx).map(|item| item.id.as_str())
    }

    pub fn select_next(&mut self) {
        if self.items.is_empty() { return; }
        let i = self.selected();
        if i < self.items.len() - 1 {
            self.state.select(Some(i + 1));
        }
    }

    pub fn select_prev(&mut self) {
        let i = self.selected();
        if i > 0 {
            self.state.select(Some(i - 1));
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let border_style = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let block = Block::default()
            .title(" Components ")
            .borders(Borders::ALL)
            .border_style(border_style);

        let list_items: Vec<ListItem> = self.items.iter().map(|item| {
            let badge = status_badge(item.status);
            let color = status_color(item.status);
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", badge), Style::default().fg(color)),
                Span::raw(&item.name),
            ]))
        }).collect();

        let list = List::new(list_items)
            .block(block)
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        frame.render_stateful_widget(list, area, &mut self.state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_badge() {
        assert_eq!(status_badge(ComponentStatus::Pending), "○");
        assert_eq!(status_badge(ComponentStatus::Building), "⋯");
        assert_eq!(status_badge(ComponentStatus::Reviewing), "◎");
        assert_eq!(status_badge(ComponentStatus::Approved), "✓");
        assert_eq!(status_badge(ComponentStatus::Error), "✗");
    }

    #[test]
    fn test_new_empty() {
        let list = ComponentListPanel::new();
        assert_eq!(list.len(), 0);
        assert_eq!(list.selected(), 0);
    }

    #[test]
    fn test_set_items() {
        let mut list = ComponentListPanel::new();
        list.set_items(&[
            ("body".into(), "Case Body".into(), ComponentStatus::Approved),
            ("cavity".into(), "Cavity".into(), ComponentStatus::Pending),
        ]);
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_navigation() {
        let mut list = ComponentListPanel::new();
        list.set_items(&[
            ("a".into(), "A".into(), ComponentStatus::Pending),
            ("b".into(), "B".into(), ComponentStatus::Pending),
            ("c".into(), "C".into(), ComponentStatus::Pending),
        ]);
        assert_eq!(list.selected(), 0);
        list.select_next();
        assert_eq!(list.selected(), 1);
        list.select_next();
        assert_eq!(list.selected(), 2);
        list.select_next(); // Should not exceed max
        assert_eq!(list.selected(), 2);
        list.select_prev();
        assert_eq!(list.selected(), 1);
    }

    #[test]
    fn test_selected_id() {
        let mut list = ComponentListPanel::new();
        list.set_items(&[
            ("body".into(), "Case Body".into(), ComponentStatus::Pending),
            ("cavity".into(), "Cavity".into(), ComponentStatus::Pending),
        ]);
        assert_eq!(list.selected_id(), Some("body"));
        list.select_next();
        assert_eq!(list.selected_id(), Some("cavity"));
    }
}
