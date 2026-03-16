//! Model panel pane — dimensions, features, metadata, pending files.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use crate::python::ModelMetadata;
use std::path::PathBuf;

pub struct ModelPanel {
    pub metadata: Option<ModelMetadata>,
    pub iteration: u32,
    pub pending_files: Vec<PathBuf>,
}

impl ModelPanel {
    pub fn new() -> Self {
        Self { metadata: None, iteration: 0, pending_files: Vec::new() }
    }

    pub fn update(&mut self, metadata: &ModelMetadata, _stl_path: Option<&std::path::Path>, iteration: u32) {
        self.metadata = Some(metadata.clone());
        self.iteration = iteration;
    }

    pub fn clear(&mut self) {
        self.metadata = None;
        self.iteration = 0;
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, _focused: bool) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Model ");

        let mut lines: Vec<Line> = Vec::new();

        if let Some(ref meta) = self.metadata {
            lines.push(Line::from(Span::styled(
                format!("{:.1} x {:.1} x {:.1} mm", meta.dimensions.x, meta.dimensions.y, meta.dimensions.z),
                Style::default().fg(Color::Yellow).bold(),
            )));
            lines.push(Line::raw(""));

            if !meta.features.is_empty() {
                lines.push(Line::from(Span::styled("Features:", Style::default().fg(Color::DarkGray))));
                for f in &meta.features {
                    lines.push(Line::from(format!("  {f}")));
                }
                lines.push(Line::raw(""));
            }

            lines.push(Line::from(vec![
                Span::styled("Iterations: ", Style::default().fg(Color::DarkGray)),
                Span::raw(self.iteration.to_string()),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Engine: ", Style::default().fg(Color::DarkGray)),
                Span::raw(meta.engine.as_str()),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Watertight: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    if meta.watertight { "yes" } else { "no" },
                    Style::default().fg(if meta.watertight { Color::Green } else { Color::Red }),
                ),
            ]));
        } else {
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled("No model yet", Style::default().fg(Color::DarkGray))));
        }

        // Show pending attachments
        if !self.pending_files.is_empty() {
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled("Attached:", Style::default().fg(Color::Cyan))));
            for (i, path) in self.pending_files.iter().enumerate() {
                let name = path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string());
                // Truncate long names
                let display = if name.len() > 20 {
                    format!("{}...", &name[..17])
                } else {
                    name
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("  {}. ", i + 1), Style::default().fg(Color::DarkGray)),
                    Span::raw(display),
                ]));
            }
            lines.push(Line::from(Span::styled(
                "  Ctrl+X clear all",
                Style::default().fg(Color::DarkGray),
            )));
        }

        let paragraph = Paragraph::new(Text::from(lines)).block(block);
        frame.render_widget(paragraph, area);
    }
}
