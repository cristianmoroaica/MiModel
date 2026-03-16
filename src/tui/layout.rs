//! Layout constraint calculation for the three-column + input bar TUI.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub struct PaneAreas {
    pub project_tree: Option<Rect>,
    pub conversation: Rect,
    pub model_panel: Option<Rect>,
    pub input_bar: Rect,
    pub legend: Rect,
}

pub struct LayoutConfig {
    pub show_sidebar: bool,
    pub show_model_panel: bool,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self { show_sidebar: true, show_model_panel: true }
    }
}

/// Compute pane areas based on terminal size and toggle state.
pub fn compute_layout(area: Rect, config: &LayoutConfig) -> PaneAreas {
    let width = area.width;

    // Auto-hide panels for narrow terminals
    let show_sidebar = config.show_sidebar && width >= 100;
    let show_model = config.show_model_panel && width >= 60;

    // Split vertically: main area + input bar (5 lines) + legend (1 line)
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .split(area);

    let main_area = vertical[0];
    let input_bar = vertical[1];
    let legend = vertical[2];

    // Split main area horizontally based on visible panels
    let (project_tree, conversation, model_panel) = match (show_sidebar, show_model) {
        (true, true) => {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(20),
                    Constraint::Percentage(55),
                    Constraint::Percentage(25),
                ])
                .split(main_area);
            (Some(cols[0]), cols[1], Some(cols[2]))
        }
        (true, false) => {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(25),
                    Constraint::Percentage(75),
                ])
                .split(main_area);
            (Some(cols[0]), cols[1], None)
        }
        (false, true) => {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(70),
                    Constraint::Percentage(30),
                ])
                .split(main_area);
            (None, cols[0], Some(cols[1]))
        }
        (false, false) => {
            (None, main_area, None)
        }
    };

    PaneAreas { project_tree, conversation, model_panel, input_bar, legend }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_all_panels() {
        let area = Rect::new(0, 0, 120, 40);
        let config = LayoutConfig::default();
        let panes = compute_layout(area, &config);
        assert!(panes.project_tree.is_some());
        assert!(panes.model_panel.is_some());
        assert_eq!(panes.input_bar.height, 5);
    }

    #[test]
    fn test_layout_narrow_hides_sidebar() {
        let area = Rect::new(0, 0, 80, 40);
        let config = LayoutConfig::default();
        let panes = compute_layout(area, &config);
        assert!(panes.project_tree.is_none()); // auto-hidden below 100
        assert!(panes.model_panel.is_some());
    }

    #[test]
    fn test_layout_very_narrow() {
        let area = Rect::new(0, 0, 50, 40);
        let config = LayoutConfig::default();
        let panes = compute_layout(area, &config);
        assert!(panes.project_tree.is_none());
        assert!(panes.model_panel.is_none());
    }
}
