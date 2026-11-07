//! Highlights `{{variable}}` spans in the URL editor (mockup `.url .v`).
//! The URL is a single line, so there is no state to carry between lines.

use std::ops::Range;

use iced_core::text::highlighter::{self, Highlighter};

use crate::ui::components::Tone;

/// The one kind of span this highlighter emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Variable;

pub struct UrlHighlighter {
    line: usize,
}

impl Highlighter for UrlHighlighter {
    type Settings = ();
    type Highlight = Variable;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, Variable)>;

    fn new(_settings: &()) -> Self {
        Self { line: 0 }
    }

    fn update(&mut self, _new_settings: &()) {
        self.line = 0;
    }

    fn change_line(&mut self, line: usize) {
        self.line = self.line.min(line);
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        self.line += 1;
        variable_spans(line)
            .into_iter()
            .map(|r| (r, Variable))
            .collect::<Vec<_>>()
            .into_iter()
    }

    fn current_line(&self) -> usize {
        self.line
    }
}

/// Byte ranges of every closed `{{…}}`, braces included.
pub fn variable_spans(line: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(open) = line[from..].find("{{").map(|i| i + from) {
        let Some(close) = line[open + 2..].find("}}").map(|i| i + open + 2) else {
            break;
        };
        out.push(open..close + 2);
        from = close + 2;
    }
    out
}

/// The table cells' and Auth fields' highlighter: like the URL's, plus red
/// for a variable that does not resolve (settings: the names that do).
pub struct FieldHighlighter {
    known: Vec<String>,
    line: usize,
}

impl Highlighter for FieldHighlighter {
    type Settings = Vec<String>;
    type Highlight = bool;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, bool)>;

    fn new(known: &Vec<String>) -> Self {
        Self {
            known: known.clone(),
            line: 0,
        }
    }

    fn update(&mut self, known: &Vec<String>) {
        self.known = known.clone();
        self.line = 0;
    }

    fn change_line(&mut self, line: usize) {
        self.line = self.line.min(line);
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        self.line += 1;
        variable_spans(line)
            .into_iter()
            .map(|r| {
                let name = line[r.start + 2..r.end - 2].trim();
                let known = self.known.iter().any(|k| k == name);
                (r, known)
            })
            .collect::<Vec<_>>()
            .into_iter()
    }

    fn current_line(&self) -> usize {
        self.line
    }
}

pub fn field_format(known: &bool, _theme: &iced::Theme) -> highlighter::Format<iced::Font> {
    highlighter::Format {
        color: Some(if *known {
            Tone::Info.color()
        } else {
            crate::ui::theme::Palette::ERROR
        }),
        font: None,
    }
}

pub fn format(_v: &Variable, _theme: &iced::Theme) -> highlighter::Format<iced::Font> {
    highlighter::Format {
        color: Some(Tone::Info.color()),
        font: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_closed_variables_only() {
        let s = "{{base_url}}/v1/{{id}}?q={{open";
        assert_eq!(variable_spans(s), vec![0..12, 16..22]);
        assert!(variable_spans("https://x.test/a").is_empty());
    }

    #[test]
    fn a_field_marks_which_variables_resolve() {
        let mut h = FieldHighlighter::new(&vec!["id".to_owned()]);
        let spans: Vec<_> = h.highlight_line("a{{id}}b{{ nope }}").collect();
        assert_eq!(spans, vec![(1..7, true), (8..18, false)]);
    }
}
