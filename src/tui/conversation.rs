//! Conversation pane — scrollable styled message list.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

#[derive(Debug, Clone)]
pub struct ConversationEntry {
    pub role: String,    // "user", "assistant", "system"
    pub content: String,
}

pub struct ConversationPane {
    pub entries: Vec<ConversationEntry>,
    pub scroll_offset: u16,
    pub auto_scroll: bool,
}

impl ConversationPane {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            scroll_offset: 0,
            auto_scroll: true,
        }
    }

    pub fn add(&mut self, role: &str, content: &str) {
        self.entries.push(ConversationEntry {
            role: role.to_string(),
            content: content.to_string(),
        });
        if self.auto_scroll {
            self.scroll_to_bottom();
        }
    }

    pub fn scroll_up(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
        self.auto_scroll = false;
    }

    pub fn scroll_down(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_add(amount);
        // max_scroll is clamped in render(), so offset > content re-enables auto-scroll
    }

    /// Scroll by page (visible height).
    pub fn page_up(&mut self, visible_height: u16) {
        self.scroll_up(visible_height.saturating_sub(2));
    }

    pub fn page_down(&mut self, visible_height: u16) {
        self.scroll_down(visible_height.saturating_sub(2));
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = u16::MAX;
        self.auto_scroll = true;
    }

    /// Clamp scroll offset to actual content height. Call after render to keep
    /// the offset in range so scroll_up() works correctly from a real position.
    pub fn clamp_scroll(&mut self, max_scroll: u16) {
        self.scroll_offset = self.scroll_offset.min(max_scroll);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.scroll_offset = 0;
        self.auto_scroll = true;
    }

    /// Render the conversation into the given area. Returns max_scroll for clamping.
    pub fn render(&self, frame: &mut Frame, area: Rect, focused: bool) -> u16 {
        let border_color = if focused {
            Color::Rgb(137, 180, 250) // Catppuccin blue
        } else {
            Color::Rgb(49, 50, 68) // Subtle border
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title_style(Style::default().fg(Color::Rgb(147, 153, 178)))
            .border_type(ratatui::widgets::BorderType::Rounded);

        // Build styled text
        let mut lines: Vec<Line> = Vec::new();
        for entry in &self.entries {
            match entry.role.as_str() {
                "user" => {
                    lines.push(Line::from(vec![
                        Span::styled("you ", Style::default().fg(Color::Rgb(166, 227, 161)).bold()),
                    ]));
                    for line in entry.content.lines() {
                        lines.push(Line::from(vec![
                            Span::styled(format!("  {line}"), Style::default().fg(Color::Rgb(205, 214, 244))),
                        ]));
                    }
                    lines.push(Line::raw(""));
                }
                "assistant" => {
                    lines.push(Line::from(vec![
                        Span::styled("claude ", Style::default().fg(Color::Rgb(203, 166, 247)).bold()),
                    ]));
                    let md_lines = render_markdown_lines(&entry.content);
                    for ml in md_lines {
                        // Indent each line
                        let mut spans = vec![Span::raw("  ")];
                        spans.extend(ml.spans);
                        lines.push(Line::from(spans));
                    }
                    lines.push(Line::raw("")); // blank separator
                }
                "system" | _ => {
                    // Compact banner style — dim, subtle
                    let content = entry.content.replace('\n', " ");
                    let truncated = if content.len() > 120 {
                        format!("{}…", &content[..120])
                    } else {
                        content
                    };
                    lines.push(Line::from(vec![
                        Span::styled("  › ", Style::default().fg(Color::Rgb(88, 91, 112))),
                        Span::styled(
                            truncated,
                            Style::default().fg(Color::Rgb(108, 112, 134)).italic(),
                        ),
                    ]));
                }
            }
        }

        // Bottom padding so the last message isn't flush against the border
        if !lines.is_empty() {
            lines.push(Line::raw(""));
        }

        let text = Text::from(lines);

        let paragraph = Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: false });

        // Use ratatui's own word-wrap line count for accurate scroll range.
        let wrapped_lines = paragraph.line_count(area.width) as u16;
        let visible = area.height.saturating_sub(2); // minus borders
        let max_scroll = wrapped_lines.saturating_sub(visible);
        let scroll = self.scroll_offset.min(max_scroll);

        let paragraph = paragraph.scroll((scroll, 0));
        frame.render_widget(paragraph, area);
        max_scroll
    }
}

/// Convert markdown-lite text into styled ratatui `Line`s.
/// Handles bullet points and inline bold/code formatting.
fn render_markdown_lines(text: &str) -> Vec<Line<'static>> {
    text.lines()
        .map(|line| {
            if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
                // Bullet point line
                let mut spans = vec![Span::styled(
                    "\u{2022} ",
                    Style::default().fg(Color::Cyan),
                )];
                spans.extend(parse_inline_spans(rest));
                Line::from(spans)
            } else {
                Line::from(parse_inline_spans(line))
            }
        })
        .collect()
}

/// Parse inline markdown spans: **bold** and `code`, everything else plain.
pub fn parse_inline_spans(text: &str) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut current = String::new();

    while i < len {
        // Check for **bold**
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            // Look for closing **
            if let Some(close) = find_close(&chars, i + 2, "**") {
                if !current.is_empty() {
                    spans.push(Span::raw(current.clone()));
                    current.clear();
                }
                let bold_text: String = chars[i + 2..close].iter().collect();
                spans.push(Span::styled(bold_text, Style::default().bold()));
                i = close + 2;
                continue;
            }
        }

        // Check for `code`
        if chars[i] == '`' {
            // Look for closing backtick
            if let Some(close) = find_close_char(&chars, i + 1, '`') {
                if !current.is_empty() {
                    spans.push(Span::raw(current.clone()));
                    current.clear();
                }
                let code_text: String = chars[i + 1..close].iter().collect();
                spans.push(Span::styled(code_text, Style::default().fg(Color::Rgb(250, 179, 135))));
                i = close + 1;
                continue;
            }
        }

        current.push(chars[i]);
        i += 1;
    }

    if !current.is_empty() {
        spans.push(Span::raw(current));
    }

    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }

    spans
}

/// Find the position of the next occurrence of a two-char delimiter starting from `from`.
fn find_close(chars: &[char], from: usize, delim: &str) -> Option<usize> {
    let d: Vec<char> = delim.chars().collect();
    let len = chars.len();
    let mut i = from;
    while i + d.len() <= len {
        if chars[i..i + d.len()] == d[..] {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Find the position of the next occurrence of a single-char delimiter starting from `from`.
fn find_close_char(chars: &[char], from: usize, delim: char) -> Option<usize> {
    for i in from..chars.len() {
        if chars[i] == delim {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    #[test]
    fn test_parse_inline_bold() {
        let spans = parse_inline_spans("hello **world**");
        assert_eq!(spans.len(), 2, "expected 2 spans, got {}", spans.len());
        assert_eq!(spans[0].content, "hello ");
        assert_eq!(spans[1].content, "world");
        assert!(
            spans[1].style.add_modifier.contains(Modifier::BOLD),
            "second span should be bold"
        );
    }

    #[test]
    fn test_parse_inline_code() {
        let spans = parse_inline_spans("use `foo` here");
        assert_eq!(spans.len(), 3, "expected 3 spans, got {}", spans.len());
        assert_eq!(spans[0].content, "use ");
        assert_eq!(spans[1].content, "foo");
        assert_eq!(
            spans[1].style.fg,
            Some(Color::Rgb(250, 179, 135)),
            "middle span should be yellow"
        );
        assert_eq!(spans[2].content, " here");
    }

    #[test]
    fn test_render_markdown_bullet() {
        let lines = render_markdown_lines("- item one");
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        // First span is the bullet symbol
        assert!(
            line.spans[0].content.contains('\u{2022}'),
            "first span should contain bullet character"
        );
        assert_eq!(
            line.spans[0].style.fg,
            Some(Color::Cyan),
            "bullet span should be cyan"
        );
        // Remaining spans contain the item text
        let text: String = line.spans[1..].iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("item one"), "line should contain item text");
    }
}
