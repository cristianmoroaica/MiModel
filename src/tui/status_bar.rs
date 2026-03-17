//! Status bar widget showing Claude API usage limits.

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::usage::{UsageStats, format_reset_time};

/// Color a percentage: green <50, yellow 50-80, red >=80.
fn usage_color(pct: f64) -> Color {
    if pct >= 80.0 {
        Color::Red
    } else if pct >= 50.0 {
        Color::Yellow
    } else {
        Color::Green
    }
}

/// Build styled spans for the usage stats, right-aligned.
pub fn render_usage_bar(frame: &mut Frame, area: Rect, stats: &UsageStats) {
    let mut spans: Vec<Span> = Vec::new();

    if let Some(pct5) = stats.five_hour_pct {
        let color = usage_color(pct5);
        spans.push(Span::styled(
            format!("5h {:.0}%", pct5),
            Style::default().fg(color),
        ));
        if let Some(ref reset) = stats.five_hour_reset {
            if let Some(fmt) = format_reset_time(reset) {
                spans.push(Span::styled(
                    format!(" ({fmt})"),
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }
    }

    if let Some(pct7) = stats.seven_day_pct {
        if !spans.is_empty() {
            spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        }
        let color = usage_color(pct7);
        spans.push(Span::styled(
            format!("7d {:.0}%", pct7),
            Style::default().fg(color),
        ));
        if let Some(ref reset) = stats.seven_day_reset {
            if let Some(fmt) = format_reset_time(reset) {
                spans.push(Span::styled(
                    format!(" ({fmt})"),
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }
    }

    if spans.is_empty() {
        return;
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).alignment(Alignment::Right);
    frame.render_widget(paragraph, area);
}
