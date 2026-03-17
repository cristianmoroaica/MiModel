//! Render helpers extracted from App::render().
//!
//! These are pure functions that take only the data they need,
//! keeping the heavy rendering logic out of main.rs.

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::phase::Phase;
use crate::tui::Focus;

/// Build phase indicator spans for the legend bar.
pub fn phase_indicator_spans(
    phase: Phase,
    current_component_idx: Option<usize>,
    components_len: usize,
    current_component_name: Option<&str>,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let current_idx = phase.index();

    let label = match phase {
        Phase::Component if components_len > 0 => {
            let current = current_component_idx.unwrap_or(0) + 1;
            let name = current_component_name.unwrap_or("?");
            format!("{} {}/{}: {}", phase.label(), current, components_len, name)
        }
        _ => phase.label().to_string(),
    };
    spans.push(Span::styled(
        format!(" {label} "),
        Style::default().fg(Color::Rgb(249, 226, 175)).bold(),
    ));

    // Phase dots
    for i in 0..5 {
        let (dot, color) = if i == current_idx {
            ("●", Color::Rgb(249, 226, 175))
        } else if i < current_idx {
            ("●", Color::Rgb(88, 91, 112))
        } else {
            ("○", Color::Rgb(59, 60, 75))
        };
        spans.push(Span::styled(format!(" {dot}"), Style::default().fg(color)));
    }
    spans.push(Span::raw("  "));

    spans
}

/// Render the legend/status bar at the bottom of the screen.
pub fn render_legend_bar(
    frame: &mut Frame,
    area: Rect,
    focus: Focus,
    phase_spans: Vec<Span<'static>>,
) {
    let key_style = Style::default().fg(Color::Rgb(147, 153, 178));
    let label_style = Style::default().fg(Color::Rgb(88, 91, 112));
    let sep = Span::styled(" · ", Style::default().fg(Color::Rgb(59, 60, 75)));

    let mut spans = phase_spans;
    spans.push(sep.clone());

    let keys: Vec<(&str, &str)> = match focus {
        Focus::Input => vec![
            ("Enter", "Send"), ("PgUp/Dn", "Scroll"), ("Tab", "Panes"),
            ("^W", "Save"), ("^V", "Img"), ("^C", "Quit"),
        ],
        Focus::ProjectTree => vec![
            ("j/k", "Nav"), ("Enter", "Open"), ("l/h", "Expand"),
            ("e", "Rename"), ("d", "Del"), ("Tab", "Panes"),
        ],
        Focus::Conversation => vec![
            ("j/k", "Scroll"), ("u/d", "Page"), ("Tab", "Panes"), ("^C", "Quit"),
        ],
        Focus::RightPanel => vec![
            ("h/l", "Tabs"), ("j/k", "Scroll"), ("Tab", "Panes"), ("^C", "Quit"),
        ],
    };

    for (i, (key, label)) in keys.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" ", label_style));
        }
        spans.push(Span::styled((*key).to_string(), key_style));
        spans.push(Span::styled(format!(" {label}"), label_style));
    }

    let legend = Line::from(spans);
    frame.render_widget(
        Paragraph::new(legend).style(Style::default().bg(Color::Rgb(24, 24, 37))),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_indicator_spec_phase() {
        let spans = phase_indicator_spans(Phase::Spec, None, 0, None);
        let label_content = &spans[0].content;
        assert!(label_content.contains("Spec"), "Expected 'Spec' in label, got: {label_content}");
    }

    #[test]
    fn phase_indicator_component_with_progress() {
        let spans = phase_indicator_spans(Phase::Component, Some(1), 5, Some("Case Body"));
        let label_content = &spans[0].content;
        assert!(label_content.contains("2/5"));
        assert!(label_content.contains("Case Body"));
    }

    #[test]
    fn phase_indicator_component_empty_list() {
        let spans = phase_indicator_spans(Phase::Component, None, 0, None);
        let label_content = &spans[0].content;
        assert!(label_content.contains("Component"));
        assert!(!label_content.contains('/'));
    }
}
