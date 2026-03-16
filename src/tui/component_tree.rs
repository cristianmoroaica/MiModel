use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

/// Lightweight component info for tree display
#[derive(Debug, Clone)]
pub struct TreeComponent {
    pub id: String,
    pub name: String,
    pub depends_on: Vec<String>,
    pub assembly_op: String,
}

pub struct ComponentTreePanel {
    components: Vec<TreeComponent>,
    scroll: u16,
}

impl ComponentTreePanel {
    pub fn new() -> Self {
        Self { components: Vec::new(), scroll: 0 }
    }

    pub fn from_components(components: &[TreeComponent]) -> Self {
        Self { components: components.to_vec(), scroll: 0 }
    }

    pub fn set_components(&mut self, components: &[TreeComponent]) {
        self.components = components.to_vec();
    }

    pub fn len(&self) -> usize { self.components.len() }

    pub fn scroll_up(&mut self) { self.scroll = self.scroll.saturating_sub(1); }
    pub fn scroll_down(&mut self) { self.scroll += 1; }

    /// Generate tree text representation
    pub fn as_text(&self) -> String {
        let mut lines = Vec::new();

        // Find root components (no dependencies)
        let roots: Vec<&TreeComponent> = self.components.iter()
            .filter(|c| c.depends_on.is_empty())
            .collect();

        for root in &roots {
            let op_label = if root.assembly_op == "none" { "base" } else { &root.assembly_op };
            lines.push(format!("  {} — {} ({})", root.id, root.name, op_label));

            // Find children that depend on this root
            let children: Vec<&TreeComponent> = self.components.iter()
                .filter(|c| c.depends_on.contains(&root.id))
                .collect();

            for (i, child) in children.iter().enumerate() {
                let connector = if i == children.len() - 1 { "└──" } else { "├──" };
                lines.push(format!("  {} {} — {} ({})", connector, child.id, child.name, child.assembly_op));
            }
        }

        lines.join("\n")
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let border_style = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let block = Block::default()
            .title(" Components ")
            .borders(Borders::ALL)
            .border_style(border_style);

        let text = self.as_text();
        let paragraph = Paragraph::new(text)
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
    fn test_empty_tree() {
        let panel = ComponentTreePanel::new();
        assert_eq!(panel.len(), 0);
    }

    #[test]
    fn test_from_components() {
        let components = vec![
            TreeComponent { id: "body".into(), name: "Case Body".into(), depends_on: vec![], assembly_op: "none".into() },
            TreeComponent { id: "cavity".into(), name: "Cavity".into(), depends_on: vec!["body".into()], assembly_op: "subtract".into() },
            TreeComponent { id: "lugs".into(), name: "Lugs".into(), depends_on: vec!["body".into()], assembly_op: "fuse".into() },
        ];
        let panel = ComponentTreePanel::from_components(&components);
        assert_eq!(panel.len(), 3);
    }

    #[test]
    fn test_tree_text_output() {
        let components = vec![
            TreeComponent { id: "body".into(), name: "Case Body".into(), depends_on: vec![], assembly_op: "none".into() },
            TreeComponent { id: "cavity".into(), name: "Cavity".into(), depends_on: vec!["body".into()], assembly_op: "subtract".into() },
        ];
        let panel = ComponentTreePanel::from_components(&components);
        let text = panel.as_text();
        assert!(text.contains("Case Body"));
        assert!(text.contains("Cavity"));
        assert!(text.contains("subtract"));
    }
}
