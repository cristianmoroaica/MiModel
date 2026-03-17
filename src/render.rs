//! Render helpers extracted from App::render().
//!
//! These are pure functions that take only the data they need,
//! keeping the heavy rendering logic out of main.rs.

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::phase::Phase;
use crate::tui::Focus;

/// Build phase indicator spans for the legend bar.
///
/// Shows: " Spec ● ○ ○ ○ ○ " with the current phase filled.
/// During Component phase, also shows progress like "Component 2/5: Case Body".
pub fn phase_indicator_spans(
    phase: Phase,
    current_component_idx: Option<usize>,
    components_len: usize,
    current_component_name: Option<&str>,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let current_idx = phase.index();

    // Phase label — with component progress when applicable
    let label = match phase {
        Phase::Component if components_len > 0 => {
            let current = current_component_idx.unwrap_or(0) + 1;
            let name = current_component_name.unwrap_or("?");
            format!(" {} {}/{}: {} ", phase.label(), current, components_len, name)
        }
        _ => format!(" {} ", phase.label()),
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

/// Render the legend/status bar at the bottom of the screen.
///
/// Combines phase indicators, key shortcuts, and the focus-dependent legend.
pub fn render_legend_bar(
    frame: &mut Frame,
    area: Rect,
    focus: Focus,
    phase_spans: Vec<Span<'static>>,
) {
    let mut legend_spans = phase_spans;
    legend_spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));
    legend_spans.push(Span::styled(" Alt+1-5 ", Style::default().fg(Color::Black).bg(Color::DarkGray)));
    legend_spans.push(Span::raw(" Phase "));
    legend_spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));
    let focus_spans: Vec<Span> = match focus {
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
        Focus::RightPanel => vec![
            Span::styled(" h/l ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
            Span::raw(" Tabs "),
            Span::styled(" j/k ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
            Span::raw(" Scroll "),
            Span::styled(" Tab ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
            Span::raw(" Panes "),
            Span::styled(" Ctrl+C ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
            Span::raw(" Quit "),
        ],
    };
    legend_spans.extend(focus_spans);
    let legend_text = Line::from(legend_spans);
    frame.render_widget(Paragraph::new(legend_text), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_indicator_spec_phase() {
        let spans = phase_indicator_spans(Phase::Spec, None, 0, None);
        // First span is the label
        let label_content = &spans[0].content;
        assert!(label_content.contains("Spec"), "Expected 'Spec' in label, got: {label_content}");
        // 5 dot spans + 1 trailing space = 6 more spans
        assert_eq!(spans.len(), 7);
    }

    #[test]
    fn phase_indicator_component_with_progress() {
        let spans = phase_indicator_spans(
            Phase::Component,
            Some(1),
            5,
            Some("Case Body"),
        );
        let label_content = &spans[0].content;
        assert!(label_content.contains("2/5"), "Expected '2/5' in label, got: {label_content}");
        assert!(label_content.contains("Case Body"), "Expected 'Case Body' in label, got: {label_content}");
    }

    #[test]
    fn phase_indicator_component_empty_list() {
        let spans = phase_indicator_spans(Phase::Component, None, 0, None);
        let label_content = &spans[0].content;
        assert!(label_content.contains("Component"), "Expected 'Component' in label");
        assert!(!label_content.contains('/'), "Should not show progress with 0 components");
    }
}
