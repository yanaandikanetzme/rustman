use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppEnvironment {
    pub id: String,
    pub name: String,
    pub variables: HashMap<String, String>,
    pub is_active: bool,
}

/// Replaces `{{name}}` with the active environment's value. Dynamic
/// variables (`{{$guid}}`, `{{$timestamp}}`, …) resolve to a fresh value
/// even without an environment; an environment variable of the same name
/// wins, like Postman.
pub fn substitute(text: &str, env: Option<&AppEnvironment>) -> String {
    if !text.contains("{{") {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                let key = after[..end].trim();
                let value = env
                    .and_then(|env| env.variables.get(key).cloned())
                    .or_else(|| crate::services::header_suggest::dynamic_value(key));
                match value {
                    Some(value) => out.push_str(&value),
                    None => out.push_str(&rest[start..start + end + 4]),
                }
                rest = &after[end + 2..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(vars: &[(&str, &str)]) -> AppEnvironment {
        let mut e = AppEnvironment {
            id: "test".to_owned(),
            name: "test".to_owned(),
            variables: HashMap::new(),
            is_active: false,
        };
        for (k, v) in vars {
            e.variables.insert((*k).to_owned(), (*v).to_owned());
        }
        e
    }

    #[test]
    fn replaces_known_variables() {
        let e = env(&[("base_url", "https://api.example.com")]);
        assert_eq!(
            substitute("{{base_url}}/posts", Some(&e)),
            "https://api.example.com/posts"
        );
    }

    #[test]
    fn tolerates_whitespace_inside_braces() {
        let e = env(&[("token", "abc")]);
        assert_eq!(substitute("Bearer {{ token }}", Some(&e)), "Bearer abc");
    }

    #[test]
    fn leaves_unknown_variables_literal() {
        let e = env(&[("a", "1")]);
        assert_eq!(substitute("{{missing}}/x", Some(&e)), "{{missing}}/x");
    }

    #[test]
    fn no_env_returns_input() {
        assert_eq!(substitute("{{a}}", None), "{{a}}");
    }

    #[test]
    fn dynamic_variables_resolve_without_an_environment() {
        let id = substitute("id={{$guid}}", None);
        assert_eq!(id.len(), "id=".len() + 36);
        assert_ne!(substitute("{{$guid}}", None), substitute("{{$guid}}", None));
        // An environment variable named like a dynamic one wins.
        let e = env(&[("$guid", "fixed")]);
        assert_eq!(substitute("{{$guid}}", Some(&e)), "fixed");
        assert_eq!(substitute("{{$unknown}}", None), "{{$unknown}}");
    }

    #[test]
    fn handles_multiple_and_unclosed() {
        let e = env(&[("a", "1"), ("b", "2")]);
        assert_eq!(substitute("{{a}}-{{b}}-{{", Some(&e)), "1-2-{{");
    }
}
