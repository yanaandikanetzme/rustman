//! Autocomplete for the request Headers table, like Postman's: header
//! names for the Key column, common values for well-known headers, and
//! `{{variables}}` (active environment + dynamic `$` variables) once a `{`
//! is typed in either column.

use crate::domain::environment::AppEnvironment;

/// Request header names offered for the Key column (Postman's list).
pub const HEADER_NAMES: &[&str] = &[
    "Accept",
    "Accept-Charset",
    "Accept-Encoding",
    "Accept-Language",
    "Access-Control-Request-Headers",
    "Access-Control-Request-Method",
    "Authorization",
    "Cache-Control",
    "Connection",
    "Content-Encoding",
    "Content-Length",
    "Content-MD5",
    "Content-Type",
    "Cookie",
    "Date",
    "DNT",
    "Expect",
    "Forwarded",
    "From",
    "Host",
    "If-Match",
    "If-Modified-Since",
    "If-None-Match",
    "If-Range",
    "If-Unmodified-Since",
    "Max-Forwards",
    "Origin",
    "Pragma",
    "Prefer",
    "Proxy-Authorization",
    "Range",
    "Referer",
    "TE",
    "Upgrade",
    "User-Agent",
    "Via",
    "Warning",
    "X-Api-Key",
    "X-Correlation-ID",
    "X-Forwarded-For",
    "X-Forwarded-Host",
    "X-Forwarded-Proto",
    "X-Request-ID",
    "X-Requested-With",
];

/// MIME types offered for Content-Type and Accept, alphabetical like Postman.
pub const MIME_TYPES: &[&str] = &[
    "application/atom+xml",
    "application/ecmascript",
    "application/graphql-response+json",
    "application/grpc",
    "application/json",
    "application/vnd.api+json",
    "application/javascript",
    "application/ld+json",
    "application/merge-patch+json",
    "application/octet-stream",
    "application/ogg",
    "application/pdf",
    "application/postscript",
    "application/problem+json",
    "application/rdf+xml",
    "application/rss+xml",
    "application/soap+xml",
    "application/x-ndjson",
    "application/x-protobuf",
    "application/x-www-form-urlencoded",
    "application/xml",
    "application/yaml",
    "application/zip",
    "audio/mpeg",
    "image/gif",
    "image/jpeg",
    "image/png",
    "image/svg+xml",
    "multipart/form-data",
    "text/css",
    "text/csv",
    "text/html",
    "text/javascript",
    "text/plain",
    "text/xml",
    "video/mp4",
];

/// Common values for a header name; empty when there is no fixed set.
fn known_values(key: &str) -> &'static [&'static str] {
    match key.trim().to_ascii_lowercase().as_str() {
        "content-type" => MIME_TYPES,
        "accept" => &[
            "*/*",
            "application/json",
            "application/xml",
            "text/html",
            "text/plain",
            "application/json, text/plain, */*",
        ],
        "accept-encoding" => &[
            "gzip, deflate, br",
            "gzip",
            "deflate",
            "br",
            "identity",
            "*",
        ],
        "accept-language" => &["en-US,en;q=0.9", "en-US", "en", "id-ID,id;q=0.9", "*"],
        "accept-charset" => &["utf-8", "iso-8859-1", "*"],
        "authorization" => &["Bearer ", "Basic "],
        "cache-control" => &[
            "no-cache",
            "no-store",
            "max-age=0",
            "must-revalidate",
            "no-transform",
            "only-if-cached",
        ],
        "connection" => &["keep-alive", "close", "upgrade"],
        "pragma" => &["no-cache"],
        "dnt" => &["0", "1"],
        "expect" => &["100-continue"],
        "prefer" => &["return=representation", "return=minimal", "respond-async"],
        "te" => &["trailers", "gzip", "deflate"],
        "upgrade" => &["websocket", "h2c"],
        "x-requested-with" => &["XMLHttpRequest"],
        "x-forwarded-proto" => &["https", "http"],
        "access-control-request-method" => {
            &["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD"]
        }
        _ => &[],
    }
}

/// Postman dynamic variables resolved on send: name and description.
pub const DYNAMIC_VARIABLES: &[(&str, &str)] = &[
    ("$guid", "A v4 style guid"),
    ("$isoTimestamp", "The current ISO timestamp at zero UTC"),
    ("$randomAlphaNumeric", "A random alpha-numeric character"),
    ("$randomBankAccount", "A random 8-digit bank account number"),
    ("$randomBoolean", "A random boolean value"),
    ("$randomColor", "A random color"),
    ("$randomHexColor", "A random hex value"),
    ("$randomInt", "A random integer between 0 and 1000"),
    ("$randomIP", "A random IPv4 address"),
    ("$randomIPV6", "A random IPv6 address"),
    ("$randomMACAddress", "A random MAC address"),
    (
        "$randomPassword",
        "A random 15-character alpha-numeric password",
    ),
    ("$randomPhoneNumber", "A random 10-digit phone number"),
    ("$randomUUID", "A random 36-character UUID"),
    ("$timestamp", "The current UNIX timestamp in seconds"),
];

/// Where a suggestion comes from; shown as the row badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Header name or value.
    Plain,
    /// Active environment variable ("E").
    Environment,
    /// Dynamic `$` variable, resolved on send.
    Dynamic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// Text shown in the list.
    pub label: String,
    /// Cell content after picking it.
    pub apply: String,
    pub source: Source,
    /// Hint shown next to the row (dynamic variable description).
    pub detail: &'static str,
}

/// Most rows the list holds; it scrolls, but building more is wasted work.
const MAX: usize = 60;

/// The unfinished `{` / `{{` token at the end of `text`, as (byte offset of
/// its first `{`, query typed after it). The cell's caret is taken to be at
/// the end: iced exposes no caret position for `text_input`.
pub fn variable_trigger(text: &str) -> Option<(usize, &str)> {
    let open = text.rfind('{')?;
    let query = &text[open + 1..];
    if query.contains(['}', ' ', '/', '?', '#']) {
        return None;
    }
    let start = if open > 0 && text.as_bytes()[open - 1] == b'{' {
        open - 1
    } else {
        open
    };
    Some((start, query))
}

fn rank(label: &str, query: &str) -> Option<u8> {
    if query.is_empty() {
        return Some(1);
    }
    let l = label.to_ascii_lowercase();
    let q = query.to_ascii_lowercase();
    if l.starts_with(&q) {
        Some(0)
    } else if l.contains(&q) {
        Some(1)
    } else {
        None
    }
}

/// Variables matching the query typed after `{`, replacing the token.
fn variables(text: &str, env: Option<&AppEnvironment>) -> Option<Vec<Suggestion>> {
    let (start, query) = variable_trigger(text)?;
    Some(variable_list(query, env, &text[..start]))
}

/// Environment then dynamic variables matching `query`; each `apply` is
/// `head` + `{{name}}`.
pub fn variable_list(query: &str, env: Option<&AppEnvironment>, head: &str) -> Vec<Suggestion> {
    let query = query.trim_start_matches('$');
    let mut env_keys: Vec<&String> = env
        .into_iter()
        .flat_map(|e| e.variables.keys())
        .filter(|k| rank(k, query).is_some())
        .collect();
    env_keys.sort_unstable_by_key(|k| (rank(k, query), k.to_ascii_lowercase()));
    let mut out: Vec<Suggestion> = env_keys
        .into_iter()
        .map(|k| Suggestion {
            label: k.clone(),
            apply: format!("{head}{{{{{k}}}}}"),
            source: Source::Environment,
            detail: "",
        })
        .collect();
    let mut dynamic: Vec<_> = DYNAMIC_VARIABLES
        .iter()
        .filter_map(|(name, detail)| Some((rank(&name[1..], query)?, *name, *detail)))
        .collect();
    dynamic.sort_by_key(|(r, name, _)| (*r, *name));
    out.extend(dynamic.into_iter().map(|(_, name, detail)| Suggestion {
        label: name.to_owned(),
        apply: format!("{head}{{{{{name}}}}}"),
        source: Source::Dynamic,
        detail,
    }));
    out.truncate(MAX);
    out
}

/// Key column: header names, the table's own keys first (Postman lists
/// what the request already uses at the top).
pub fn for_key(text: &str, used: &[&str], env: Option<&AppEnvironment>) -> Vec<Suggestion> {
    if let Some(vars) = variables(text, env) {
        return vars;
    }
    let mut seen = std::collections::HashSet::new();
    let mut ranked: Vec<(u8, usize, &str)> = Vec::new();
    for (order, name) in used
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .chain(HEADER_NAMES.iter().copied())
        .enumerate()
    {
        if !seen.insert(name.to_ascii_lowercase()) {
            continue;
        }
        if let Some(r) = rank(name, text) {
            ranked.push((r, order, name));
        }
    }
    // Typing a prefix ranks prefix matches first; an empty cell keeps order.
    ranked.sort_by_key(|(r, order, _)| (*r, *order));
    ranked
        .into_iter()
        // The exact value is already in the cell: listing it is noise.
        .filter(|(_, _, name)| *name != text)
        .take(MAX)
        .map(|(_, _, name)| Suggestion {
            label: name.to_owned(),
            apply: name.to_owned(),
            source: Source::Plain,
            detail: "",
        })
        .collect()
}

/// Value column: values for well-known headers, or variables after `{`.
pub fn for_value(key: &str, text: &str, env: Option<&AppEnvironment>) -> Vec<Suggestion> {
    if let Some(vars) = variables(text, env) {
        return vars;
    }
    let mut ranked: Vec<(u8, usize, &str)> = known_values(key)
        .iter()
        .enumerate()
        .filter(|(_, v)| **v != text)
        .filter_map(|(i, v)| Some((rank(v, text)?, i, *v)))
        .collect();
    ranked.sort_by_key(|(r, i, _)| (*r, *i));
    ranked
        .into_iter()
        .take(MAX)
        .map(|(_, _, v)| Suggestion {
            label: v.to_owned(),
            apply: v.to_owned(),
            source: Source::Plain,
            detail: "",
        })
        .collect()
}

/// Value of a dynamic variable (`$guid`, `$timestamp`, …), fresh per call.
/// `None` for names that are not dynamic variables.
pub fn dynamic_value(name: &str) -> Option<String> {
    let bytes = || *uuid::Uuid::new_v4().as_bytes();
    let num = |max: u64| {
        let b = bytes();
        u64::from_le_bytes(b[..8].try_into().unwrap_or([0; 8])) % max
    };
    let alnum = |n: usize| {
        const SET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
        let mut out = String::with_capacity(n);
        while out.len() < n {
            for b in bytes() {
                if out.len() == n {
                    break;
                }
                out.push(SET[b as usize % SET.len()] as char);
            }
        }
        out
    };
    let value = match name {
        "$guid" | "$randomUUID" => uuid::Uuid::new_v4().to_string(),
        "$timestamp" => chrono::Utc::now().timestamp().to_string(),
        "$isoTimestamp" => chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "$randomInt" => num(1001).to_string(),
        "$randomAlphaNumeric" => alnum(1),
        "$randomPassword" => alnum(15),
        "$randomBoolean" => (num(2) == 1).to_string(),
        "$randomBankAccount" => format!("{:08}", num(100_000_000)),
        "$randomPhoneNumber" => format!("{:03}-{:03}-{:04}", num(1000), num(1000), num(10_000)),
        "$randomHexColor" => {
            let b = bytes();
            format!("#{:02x}{:02x}{:02x}", b[0], b[1], b[2])
        }
        "$randomColor" => {
            const COLORS: &[&str] = &[
                "red", "orange", "yellow", "green", "blue", "indigo", "violet", "black", "white",
                "gray", "teal", "purple", "pink", "brown",
            ];
            COLORS[num(COLORS.len() as u64) as usize].to_owned()
        }
        "$randomIP" => {
            let b = bytes();
            format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3])
        }
        "$randomIPV6" => {
            let b = bytes();
            b.chunks(2)
                .map(|c| format!("{:x}", u16::from_be_bytes([c[0], c[1]])))
                .collect::<Vec<_>>()
                .join(":")
        }
        "$randomMACAddress" => {
            let b = bytes();
            b[..6]
                .iter()
                .map(|x| format!("{x:02x}"))
                .collect::<Vec<_>>()
                .join(":")
        }
        _ => return None,
    };
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(keys: &[&str]) -> AppEnvironment {
        AppEnvironment {
            id: "e".into(),
            name: "Dev".into(),
            variables: keys
                .iter()
                .map(|k| ((*k).to_owned(), "v".to_owned()))
                .collect::<HashMap<_, _>>(),
            is_active: true,
        }
    }

    #[test]
    fn empty_key_lists_used_headers_first_then_postman_list() {
        let s = for_key("", &["Content-Type", "X-Test", ""], None);
        let labels: Vec<_> = s.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(
            &labels[..4],
            ["Content-Type", "X-Test", "Accept", "Accept-Charset"]
        );
        // Used headers are not listed twice.
        assert_eq!(labels.iter().filter(|l| **l == "Content-Type").count(), 1);
    }

    #[test]
    fn typing_filters_prefix_matches_first() {
        let s = for_key("con", &[], None);
        assert_eq!(s[0].label, "Connection");
        assert!(s.iter().any(|s| s.label == "Content-Type"));
        assert!(
            s.iter()
                .all(|s| s.label.to_ascii_lowercase().contains("con"))
        );
    }

    #[test]
    fn content_type_values_and_unknown_headers() {
        let s = for_value("content-type", "", None);
        assert_eq!(s[0].label, "application/atom+xml");
        assert!(s.iter().any(|s| s.label == "application/json"));
        assert_eq!(
            for_value("content-type", "json", None)[0].label,
            "application/graphql-response+json"
        );
        assert!(for_value("X-Test", "", None).is_empty());
    }

    #[test]
    fn brace_offers_environment_then_dynamic_variables() {
        let e = env(&["TOKO_TOKEN", "base_url"]);
        let s = for_value("Authorization", "Bearer {", Some(&e));
        assert_eq!(s[0].source, Source::Environment);
        assert!(
            s.iter()
                .any(|s| s.label == "$guid" && s.source == Source::Dynamic)
        );
        let guid = s.iter().find(|s| s.label == "$guid").unwrap();
        assert_eq!(guid.apply, "Bearer {{$guid}}");
        assert_eq!(guid.detail, "A v4 style guid");

        let s = for_value("X", "{{base", Some(&e));
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].apply, "{{base_url}}");
        assert!(for_value("X", "{{done}}", Some(&e)).is_empty());
    }

    #[test]
    fn trigger_finds_the_open_token() {
        assert_eq!(variable_trigger("a{"), Some((1, "")));
        assert_eq!(variable_trigger("a{{to"), Some((1, "to")));
        assert_eq!(variable_trigger("{{x}} y"), None);
        assert_eq!(variable_trigger("plain"), None);
    }

    #[test]
    fn dynamic_values_have_the_documented_shapes() {
        assert_eq!(dynamic_value("$guid").unwrap().len(), 36);
        assert!(dynamic_value("$timestamp").unwrap().parse::<i64>().is_ok());
        assert!(dynamic_value("$randomInt").unwrap().parse::<u32>().unwrap() <= 1000);
        assert_eq!(dynamic_value("$randomBankAccount").unwrap().len(), 8);
        assert_eq!(dynamic_value("$randomPassword").unwrap().len(), 15);
        assert!(dynamic_value("$randomHexColor").unwrap().starts_with('#'));
        assert!(dynamic_value("$nope").is_none());
        for (name, _) in DYNAMIC_VARIABLES {
            assert!(dynamic_value(name).is_some(), "{name} resolves");
        }
    }
}
