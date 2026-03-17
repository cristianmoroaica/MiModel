//! Layout constraint calculation for the three-column + input bar TUI.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use crate::phase::Phase;

/// Cached Rect positions for mouse hit-testing.
#[derive(Default, Clone)]
pub struct PanelRects {
    pub project_tree: ratatui::prelude::Rect,
    pub conversation: ratatui::prelude::Rect,
    pub right_panel: ratatui::prelude::Rect,
    pub input: ratatui::prelude::Rect,
}

pub struct PaneAreas {
    pub left_panel: Option<Rect>,   // was project_tree
    pub conversation: Rect,
    pub right_panel: Option<Rect>,  // was model_panel
    pub input_bar: Rect,
    pub legend: Rect,
}

pub struct LayoutConfig {
    pub show_sidebar: bool,
    pub show_model_panel: bool,
    pub phase: Phase,
    /// Height of the input bar in rows (default 3, grows with line count, capped at 7).
    pub input_height: u16,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self { show_sidebar: true, show_model_panel: true, phase: Phase::Spec, input_height: 5 }
    }
}

/// Compute pane areas based on terminal size and toggle state.
pub fn compute_layout(area: Rect, config: &LayoutConfig) -> PaneAreas {
    let width = area.width;

    // Auto-hide panels for narrow terminals
    let show_sidebar = config.show_sidebar && width >= 100;
    let show_model = config.show_model_panel && width >= 60;

    // Split vertically: main area + input bar (dynamic height) + legend (1 line)
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),
            Constraint::Length(config.input_height),
            Constraint::Length(1),
        ])
        .split(area);

    let main_area = vertical[0];
    let input_bar = vertical[1];
    let legend = vertical[2];

    // Split main area horizontally based on visible panels
    let (left_panel, conversation, right_panel) = match (show_sidebar, show_model) {
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

    PaneAreas { left_panel, conversation, right_panel, input_bar, legend }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_all_panels() {
        let area = Rect::new(0, 0, 120, 40);
        let config = LayoutConfig::default();
        let panes = compute_layout(area, &config);
        assert!(panes.left_panel.is_some());
        assert!(panes.right_panel.is_some());
        assert_eq!(panes.input_bar.height, 5); // default input_height
    }

    #[test]
    fn test_layout_narrow_hides_sidebar() {
        let area = Rect::new(0, 0, 80, 40);
        let config = LayoutConfig::default();
        let panes = compute_layout(area, &config);
        assert!(panes.left_panel.is_none()); // auto-hidden below 100
        assert!(panes.right_panel.is_some());
    }

    #[test]
    fn test_layout_very_narrow() {
        let area = Rect::new(0, 0, 50, 40);
        let config = LayoutConfig::default();
        let panes = compute_layout(area, &config);
        assert!(panes.left_panel.is_none());
        assert!(panes.right_panel.is_none());
    }

    #[test]
    fn test_layout_with_phase() {
        let area = Rect::new(0, 0, 120, 40);
        let config = LayoutConfig { show_sidebar: true, show_model_panel: true, phase: Phase::Component, input_height: 3 };
        let panes = compute_layout(area, &config);
        // Layout dimensions are the same regardless of phase
        assert!(panes.left_panel.is_some());
        assert!(panes.right_panel.is_some());
    }
}
