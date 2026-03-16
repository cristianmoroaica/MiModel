//! Model panel pane — dimensions, features, braille preview, metadata.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use crate::python::ModelMetadata;
use crate::preview::{render_braille, ViewAngle};
use crate::stl::StlMesh;

pub struct ModelPanel {
    pub metadata: Option<ModelMetadata>,
    pub preview_text: Option<String>,
    pub iteration: u32,
}

impl ModelPanel {
    pub fn new() -> Self {
        Self { metadata: None, preview_text: None, iteration: 0 }
    }

    /// Update with new build results.
    pub fn update(&mut self, metadata: &ModelMetadata, stl_path: Option<&std::path::Path>, iteration: u32) {
        self.metadata = Some(metadata.clone());
        self.iteration = iteration;

        // Generate braille preview if STL is available
        if let Some(path) = stl_path {
            if let Ok(mesh) = StlMesh::from_file(path) {
                self.preview_text = Some(render_braille(&mesh, ViewAngle::Front, 20));
            }
        }
    }

    pub fn clear(&mut self) {
        self.metadata = None;
        self.preview_text = None;
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

            if let Some(ref preview) = self.preview_text {
                lines.push(Line::from(Span::styled("Preview:", Style::default().fg(Color::DarkGray))));
                for line in preview.lines() {
                    lines.push(Line::raw(format!(" {line}")));
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

        let paragraph = Paragraph::new(Text::from(lines)).block(block);
        frame.render_widget(paragraph, area);
    }
}
