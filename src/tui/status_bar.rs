//! Status bar widget showing Claude API usage limits.

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::usage::{UsageStats, format_reset_time};

fn usage_color(pct: f64) -> Color {
    if pct >= 80.0 {
        Color::Rgb(243, 139, 168) // Catppuccin red
    } else if pct >= 50.0 {
        Color::Rgb(249, 226, 175) // Catppuccin yellow
    } else {
        Color::Rgb(166, 227, 161) // Catppuccin green
    }
}

pub fn render_usage_bar(frame: &mut Frame, area: Rect, stats: &UsageStats) {
    let mut spans: Vec<Span> = Vec::new();
    let dim = Style::default().fg(Color::Rgb(88, 91, 112));

    if let Some(pct5) = stats.five_hour_pct {
        spans.push(Span::styled(format!("5h {:.0}%", pct5), Style::default().fg(usage_color(pct5))));
        if let Some(ref reset) = stats.five_hour_reset {
            if let Some(fmt) = format_reset_time(reset) {
                spans.push(Span::styled(format!(" ({fmt})"), dim));
            }
        }
    }

    if let Some(pct7) = stats.seven_day_pct {
        if !spans.is_empty() {
            spans.push(Span::styled(" · ", dim));
        }
        spans.push(Span::styled(format!("7d {:.0}%", pct7), Style::default().fg(usage_color(pct7))));
        if let Some(ref reset) = stats.seven_day_reset {
            if let Some(fmt) = format_reset_time(reset) {
                spans.push(Span::styled(format!(" ({fmt})"), dim));
            }
        }
    }

    if spans.is_empty() {
        return;
    }

    // Right-align with padding
    spans.insert(0, Span::raw(" "));
    spans.push(Span::raw(" "));
    let line = Line::from(spans);
    let paragraph = Paragraph::new(line)
        .alignment(Alignment::Right)
        .style(Style::default().bg(Color::Rgb(24, 24, 37)));
    frame.render_widget(paragraph, area);
}
