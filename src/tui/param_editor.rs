use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Row, Table, Cell};

struct ParamEntry {
    name: String,
    original_value: f64,
    current_value: f64,
    unit: String,
}

pub struct ParamEditor {
    params: Vec<ParamEntry>,
    selected: usize,
    editing: bool,
    edit_buffer: String,
}

impl ParamEditor {
    pub fn new(params: &[(String, f64, String)]) -> Self {
        let entries = params.iter().map(|(name, value, unit)| ParamEntry {
            name: name.clone(),
            original_value: *value,
            current_value: *value,
            unit: unit.clone(),
        }).collect();
        Self { params: entries, selected: 0, editing: false, edit_buffer: String::new() }
    }

    pub fn reset(&mut self, params: &[(String, f64, String)]) {
        self.params = params.iter().map(|(name, value, unit)| ParamEntry {
            name: name.clone(),
            original_value: *value,
            current_value: *value,
            unit: unit.clone(),
        }).collect();
        self.selected = 0;
        self.editing = false;
    }

    pub fn len(&self) -> usize { self.params.len() }
    pub fn selected(&self) -> usize { self.selected }

    pub fn value(&self, idx: usize) -> Option<f64> {
        self.params.get(idx).map(|p| p.current_value)
    }

    pub fn set_value(&mut self, idx: usize, value: f64) {
        if let Some(p) = self.params.get_mut(idx) {
            p.current_value = value;
        }
    }

    pub fn select_next(&mut self) {
        if !self.params.is_empty() && self.selected < self.params.len() - 1 {
            self.selected += 1;
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Returns only parameters whose values differ from the original
    pub fn changed_params(&self) -> Vec<(String, f64)> {
        self.params.iter()
            .filter(|p| (p.current_value - p.original_value).abs() > f64::EPSILON)
            .map(|p| (p.name.clone(), p.current_value))
            .collect()
    }

    // Edit mode methods for future TUI integration
    pub fn start_editing(&mut self) {
        if let Some(p) = self.params.get(self.selected) {
            self.edit_buffer = format!("{}", p.current_value);
            self.editing = true;
        }
    }

    pub fn cancel_editing(&mut self) {
        self.editing = false;
        self.edit_buffer.clear();
    }

    pub fn confirm_editing(&mut self) {
        if self.editing {
            if let Ok(val) = self.edit_buffer.parse::<f64>() {
                self.set_value(self.selected, val);
            }
            self.editing = false;
            self.edit_buffer.clear();
        }
    }

    pub fn edit_input(&mut self, ch: char) {
        if self.editing {
            self.edit_buffer.push(ch);
        }
    }

    pub fn edit_backspace(&mut self) {
        if self.editing {
            self.edit_buffer.pop();
        }
    }

    pub fn is_editing(&self) -> bool { self.editing }

    pub fn render(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let border_style = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let block = Block::default()
            .title(" Parameters ")
            .borders(Borders::ALL)
            .border_style(border_style);

        let rows: Vec<Row> = self.params.iter().enumerate().map(|(i, p)| {
            let changed = (p.current_value - p.original_value).abs() > f64::EPSILON;
            let value_style = if changed {
                Style::default().fg(Color::Yellow).bold()
            } else {
                Style::default()
            };

            let value_text = if self.editing && i == self.selected {
                format!("{}▎", self.edit_buffer)
            } else {
                format!("{}", p.current_value)
            };

            let row_style = if i == self.selected && focused {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(Span::styled(&p.name, Style::default().fg(Color::DarkGray))),
                Cell::from(Span::styled(value_text, value_style)),
                Cell::from(Span::styled(&p.unit, Style::default().fg(Color::DarkGray))),
            ]).style(row_style)
        }).collect();

        let widths = [
            Constraint::Percentage(45),
            Constraint::Percentage(35),
            Constraint::Percentage(20),
        ];

        let table = Table::new(rows, widths)
            .block(block)
            .header(Row::new(["Name", "Value", "Unit"])
                .style(Style::default().fg(Color::DarkGray).bold()));

        frame.render_widget(table, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_from_params() {
        let editor = ParamEditor::new(&[
            ("OUTER_DIAMETER".into(), 40.0, "mm".into()),
            ("HEIGHT".into(), 11.5, "mm".into()),
        ]);
        assert_eq!(editor.len(), 2);
    }

    #[test]
    fn test_get_value() {
        let editor = ParamEditor::new(&[
            ("WIDTH".into(), 10.0, "mm".into()),
        ]);
        assert_eq!(editor.value(0), Some(10.0));
        assert_eq!(editor.value(99), None);
    }

    #[test]
    fn test_set_value() {
        let mut editor = ParamEditor::new(&[
            ("WIDTH".into(), 10.0, "mm".into()),
        ]);
        editor.set_value(0, 20.0);
        assert_eq!(editor.value(0), Some(20.0));
    }

    #[test]
    fn test_changed_params_only_modified() {
        let mut editor = ParamEditor::new(&[
            ("WIDTH".into(), 10.0, "mm".into()),
            ("HEIGHT".into(), 5.0, "mm".into()),
            ("DEPTH".into(), 3.0, "mm".into()),
        ]);
        editor.set_value(0, 15.0);
        // HEIGHT unchanged
        editor.set_value(2, 4.0);

        let changed = editor.changed_params();
        assert_eq!(changed.len(), 2);
        assert!(changed.iter().any(|(name, val)| name == "WIDTH" && *val == 15.0));
        assert!(changed.iter().any(|(name, val)| name == "DEPTH" && *val == 4.0));
        // HEIGHT should NOT be in changed
        assert!(!changed.iter().any(|(name, _)| name == "HEIGHT"));
    }

    #[test]
    fn test_navigation() {
        let mut editor = ParamEditor::new(&[
            ("A".into(), 1.0, "mm".into()),
            ("B".into(), 2.0, "mm".into()),
            ("C".into(), 3.0, "mm".into()),
        ]);
        assert_eq!(editor.selected(), 0);
        editor.select_next();
        assert_eq!(editor.selected(), 1);
        editor.select_prev();
        assert_eq!(editor.selected(), 0);
    }

    #[test]
    fn test_reset_clears_changes() {
        let mut editor = ParamEditor::new(&[
            ("WIDTH".into(), 10.0, "mm".into()),
        ]);
        editor.set_value(0, 20.0);
        assert_eq!(editor.changed_params().len(), 1);

        editor.reset(&[("WIDTH".into(), 10.0, "mm".into())]);
        assert_eq!(editor.changed_params().len(), 0);
        assert_eq!(editor.value(0), Some(10.0));
    }
}
