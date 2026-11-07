//! Variable lookup shared by URL assistance and snippet previews.
use crate::domain::environment::AppEnvironment;
use std::ops::Range;

pub const MASK: &str = "••••••";

/// Exactly one reference, with no literal suffix/prefix or nested braces.
pub fn reference_key(value: &str) -> Option<&str> {
    let key = value.trim().strip_prefix("{{")?.strip_suffix("}}")?.trim();
    (!key.is_empty() && !key.contains(['{', '}', '\n', '\r'])).then_some(key)
}

/// Template source stays legible while a single reference is being typed.
pub fn template_input(value: &str) -> bool {
    if value.len() > crate::consts::INLINE_LIMIT {
        return false;
    }
    if reference_key(value).is_some() {
        return true;
    }
    value
        .trim_start()
        .strip_prefix("{{")
        .is_some_and(|key| !key.contains(['{', '}', '\n', '\r']))
}

pub fn sensitive(key: &str) -> bool {
    let key = key.to_ascii_lowercase().replace('-', "_");
    [
        "token",
        "secret",
        "password",
        "passwd",
        "api_key",
        "apikey",
        "authorization",
        "cookie",
        "session",
        "credential",
        "access_key",
        "private_key",
        "jwt",
    ]
    .iter()
    .any(|part| key.contains(part))
        || matches!(key.as_str(), "sid" | "auth")
}

/// The unfinished token at the caret, including an existing closing pair
/// when editing a completed token. Offsets are UTF-8 byte positions like iced.
pub fn completion(text: &str, caret: usize) -> Option<(Range<usize>, &str)> {
    let before = text.get(..caret)?;
    let start = before.rfind("{{")?;
    let query = &text[start + 2..caret];
    if query.contains(['{', '}', '/', '?', '#']) {
        return None;
    }
    let end = text[caret..]
        .find("}}")
        .filter(|&n| !text[caret..caret + n].contains(['{', '/', '?', '#']))
        .map_or(caret, |n| caret + n + 2);
    Some((start..end, query.trim()))
}

pub fn candidates(env: Option<&AppEnvironment>, query: &str) -> Vec<String> {
    let query = query.to_lowercase();
    let mut keys: Vec<_> = env
        .into_iter()
        .flat_map(|e| e.variables.keys())
        .filter(|key| key.to_lowercase().starts_with(&query))
        .cloned()
        .collect();
    keys.sort_unstable();
    keys.truncate(8);
    keys
}

pub fn variable_at(text: &str, caret: usize) -> Option<String> {
    let mut from = 0;
    while let Some(start) = text[from..].find("{{").map(|i| i + from) {
        let end = text[start + 2..].find("}}")? + start + 2;
        if (start..end + 2).contains(&caret) {
            return Some(text[start + 2..end].trim().to_owned());
        }
        from = end + 2;
    }
    None
}

/// A closed `{{name}}` in a text, by line and char columns (end exclusive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarToken {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub name: String,
}

/// Tokens the body editor marks; past this a generated payload is not worth
/// the per-frame geometry.
const MAX_TOKENS: usize = 500;

/// All `{{name}}` tokens of a text, each within one line.
pub fn variable_tokens(text: &str) -> Vec<VarToken> {
    let mut out = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        let mut from = 0;
        while let Some(open) = line[from..].find("{{").map(|i| i + from) {
            let Some(close) = line[open + 2..].find("}}").map(|i| i + open + 2) else {
                break;
            };
            let name = line[open + 2..close].trim();
            if !name.is_empty() && !name.contains(['{', '}']) {
                let start = line[..open].chars().count();
                out.push(VarToken {
                    line: line_no,
                    start,
                    end: start + line[open..close + 2].chars().count(),
                    name: name.to_owned(),
                });
                if out.len() == MAX_TOKENS {
                    return out;
                }
            }
            from = close + 2;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completion_replaces_only_the_current_token() {
        let text = "https://x/{{base_}}/users/{{id}}";
        let (range, query) = completion(text, 17).unwrap();
        assert_eq!(query, "base_");
        let mut replaced = text.to_owned();
        replaced.replace_range(range, "{{base_url}}");
        assert_eq!(replaced, "https://x/{{base_url}}/users/{{id}}");
        assert!(completion("{{base}}/users", 14).is_none());
        assert!(completion("é/{{ba", 1).is_none());
        assert_eq!(completion("é/{{ba", 7).unwrap().1, "ba");
    }
    #[test]
    fn hover_only_matches_tokens() {
        assert_eq!(
            variable_at("é/{{base_url}}/x", 7).as_deref(),
            Some("base_url")
        );
        assert!(variable_at("{{base_url}}/x", 12).is_none());
    }
}

#[cfg(test)]
mod token_tests {
    use super::*;

    #[test]
    fn tokens_use_char_columns_and_skip_open_or_empty_ones() {
        let t = variable_tokens("é {{ a }} {{b}}\n{{}} {{open\n  {{c}}");
        let found: Vec<_> = t
            .iter()
            .map(|t| (t.line, t.start, t.end, t.name.as_str()))
            .collect();
        assert_eq!(found, [(0, 2, 9, "a"), (0, 10, 15, "b"), (2, 2, 7, "c")]);
    }
}
