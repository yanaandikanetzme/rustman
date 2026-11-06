//! Structured preview of an HTML response body.
//!
//! There is no embedded web engine, so a page cannot be rendered as a
//! browser would. Debug pages are the case that matters most when calling
//! an API: Laravel `dd()` / Symfony VarDumper emit `<pre class=sf-dump>`
//! blocks whose colours come from CSS and whose layout is the text itself.
//! Those blocks are kept as coloured spans (VarDumper's dark theme), the
//! rest of the page becomes readable text.

use super::format::{html_to_text, push_entity};

/// Colour role of a dump span, from its `sf-dump-*` class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DumpKind {
    /// Brackets, `=>` and other un-classed text.
    Default,
    Num,
    Const,
    Str,
    Note,
    Ref,
    Member,
    Meta,
    Key,
    Index,
    Ellipsis,
}

impl DumpKind {
    fn from_class(class: &str) -> Option<Self> {
        let kind = match class.strip_prefix("sf-dump-")? {
            "num" => Self::Num,
            "const" => Self::Const,
            "str" => Self::Str,
            "note" => Self::Note,
            "ref" => Self::Ref,
            "public" | "protected" | "private" | "label" => Self::Member,
            "meta" => Self::Meta,
            "key" => Self::Key,
            "index" => Self::Index,
            "ellipsis" => Self::Ellipsis,
            _ => return None,
        };
        Some(kind)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// A VarDumper `<pre>`: whitespace kept, text split into coloured runs.
    Dump(Vec<(String, DumpKind)>),
    /// Any other content, as readable text.
    Text(String),
}

/// Input cap: past this the page is cut, like the readable-text preview.
const INPUT_CAP: usize = 1_000_000;

/// Splits a page into dump blocks and readable text between them.
pub fn blocks(html: &str) -> Vec<Block> {
    let html = &html[..super::format::head_capped(html, INPUT_CAP).len()];
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(start) = find_dump_pre(rest) {
        push_text(&mut out, &rest[..start]);
        let after = &rest[start..];
        let open_end = after.find('>').map_or(after.len(), |i| i + 1);
        let body = &after[open_end..];
        let close = find_ci(body, "</pre>").unwrap_or(body.len());
        let spans = dump_spans(&body[..close]);
        if !spans.is_empty() {
            out.push(Block::Dump(spans));
        }
        rest = body.get(close + "</pre>".len()..).unwrap_or("");
    }
    push_text(&mut out, rest);
    out
}

fn push_text(out: &mut Vec<Block>, html: &str) {
    let text = html_to_text(html);
    if !text.trim().is_empty() {
        out.push(Block::Text(text));
    }
}

/// Byte offset of the next `<pre` tag carrying the `sf-dump` class.
fn find_dump_pre(s: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = find_ci(&s[from..], "<pre") {
        let at = from + i;
        let tag_end = s[at..].find('>').map_or(s.len(), |e| at + e);
        if s[at..tag_end].contains("sf-dump") {
            return Some(at);
        }
        from = at + 4;
    }
    None
}

/// ASCII case-insensitive `find` (needle must be lowercase ASCII).
fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    let n = needle.as_bytes();
    hay.as_bytes()
        .windows(n.len())
        .position(|w| w.eq_ignore_ascii_case(n))
}

/// Coloured runs of a dump `<pre>` body. `<span class=sf-dump-*>` sets the
/// colour for its content; other tags (`samp`, `abbr`, `a`) are transparent.
fn dump_spans(body: &str) -> Vec<(String, DumpKind)> {
    let mut spans: Vec<(String, DumpKind)> = Vec::new();
    // Colour of each open tag; `None` = inherits.
    let mut stack: Vec<Option<DumpKind>> = Vec::new();
    let mut buf = String::new();
    let current = |stack: &[Option<DumpKind>]| {
        stack
            .iter()
            .rev()
            .find_map(|k| *k)
            .unwrap_or(DumpKind::Default)
    };
    let flush = |buf: &mut String, kind: DumpKind, spans: &mut Vec<(String, DumpKind)>| {
        if buf.is_empty() {
            return;
        }
        match spans.last_mut() {
            Some((text, k)) if *k == kind => text.push_str(buf),
            _ => spans.push((buf.clone(), kind)),
        }
        buf.clear();
    };
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' if chars
                .peek()
                .is_some_and(|p| p.is_ascii_alphabetic() || *p == '/') =>
            {
                let mut raw = String::new();
                let mut quote = None;
                for p in chars.by_ref() {
                    match quote {
                        Some(q) if p == q => quote = None,
                        Some(_) => {}
                        None if p == '"' || p == '\'' => quote = Some(p),
                        None if p == '>' => break,
                        None => {}
                    }
                    raw.push(p);
                }
                let kind = current(&stack);
                flush(&mut buf, kind, &mut spans);
                if raw.starts_with('/') {
                    stack.pop();
                } else if !raw.ends_with('/') && !raw.to_ascii_lowercase().starts_with("br") {
                    stack.push(class_kind(&raw));
                } else if raw.to_ascii_lowercase().starts_with("br") {
                    buf.push('\n');
                }
            }
            '&' => push_entity(&mut chars, &mut buf),
            '\r' => {}
            _ => buf.push(c),
        }
    }
    let kind = current(&stack);
    flush(&mut buf, kind, &mut spans);
    // A dump ends with its own newline; the block frame adds the spacing.
    if let Some((text, _)) = spans.last_mut() {
        let trimmed = text.trim_end().len();
        text.truncate(trimmed);
    }
    spans.retain(|(t, _)| !t.is_empty());
    spans
}

/// Colour of a tag from the first `sf-dump-*` token of its `class`.
fn class_kind(raw: &str) -> Option<DumpKind> {
    let i = find_ci(raw, "class=")?;
    let value = &raw[i + "class=".len()..];
    let value = match value.chars().next() {
        Some(q @ ('"' | '\'')) => value[1..].split(q).next().unwrap_or(""),
        _ => value
            .split(|c: char| c.is_whitespace())
            .next()
            .unwrap_or(""),
    };
    value.split_whitespace().find_map(DumpKind::from_class)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DD: &str = r#"<script> Sfdump = window.Sfdump || (function (doc) { var a = 1 < 2; })(document); </script><style>pre.sf-dump{color:#FF8400}</style><pre class=sf-dump id=sf-dump-1 data-indent-pad="  "><span class=sf-dump-note>array:2</span> [<samp data-depth=1 class=sf-dump-expanded>
  <span class=sf-dump-index>0</span> => <span class=sf-dump-num>11</span>
  <span class=sf-dump-index>1</span> => <span class=sf-dump-num>14</span>
</samp>]
</pre><script>Sfdump("sf-dump-1")</script>
<pre class=sf-dump id=sf-dump-2 data-indent-pad="  ">"<span class=sf-dump-str title="29 characters">2026-10-29T23:59:59.999+07:00</span>"
</pre><script>Sfdump("sf-dump-2")</script>"#;

    #[test]
    fn laravel_dd_becomes_coloured_dump_blocks() {
        let blocks = blocks(DD);
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        let Block::Dump(first) = &blocks[0] else {
            panic!("first block is a dump")
        };
        assert_eq!(first[0], ("array:2".into(), DumpKind::Note));
        assert!(first.contains(&("0".into(), DumpKind::Index)));
        assert!(first.contains(&("14".into(), DumpKind::Num)));
        let text: String = first.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(text, "array:2 [\n  0 => 11\n  1 => 14\n]");
        let Block::Dump(second) = &blocks[1] else {
            panic!("second block is a dump")
        };
        assert_eq!(
            second[1],
            ("2026-10-29T23:59:59.999+07:00".into(), DumpKind::Str)
        );
    }

    #[test]
    fn plain_pages_are_readable_text() {
        let blocks = blocks("<html><body><h1>Oops</h1><p>a &amp; b</p></body></html>");
        assert_eq!(blocks, vec![Block::Text("Oops\n\na & b".into())]);
    }

    #[test]
    fn unclosed_dump_does_not_panic() {
        let blocks = blocks("<pre class='sf-dump'><span class=\"sf-dump-str\">x &lt; y");
        assert_eq!(
            blocks,
            vec![Block::Dump(vec![("x < y".into(), DumpKind::Str)])]
        );
    }
}
