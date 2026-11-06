//! Redaction boundary for diagnostic output. This never mutates request data.
#[derive(Default)]
pub struct Redactor {
    values: Vec<String>,
}

impl Redactor {
    pub fn add(&mut self, value: &str) {
        if !value.is_empty() && !self.values.iter().any(|s| s == value) {
            self.values.push(value.to_owned());
            // Longest first prevents a short overlapping value exposing suffixes.
            self.values.sort_by_key(|v| std::cmp::Reverse(v.len()));
        }
    }
    pub fn add_header(&mut self, name: &str, value: &str) {
        if !crate::services::variables::sensitive(name) {
            return;
        }
        self.add(value);
        if name.eq_ignore_ascii_case("cookie") {
            for part in value.split(';') {
                if let Some((_, secret)) = part.split_once('=') {
                    self.add(secret.trim());
                }
            }
        } else if name.eq_ignore_ascii_case("set-cookie") {
            for line in value.lines() {
                if let Some((_, secret)) = line.split(';').next().unwrap_or("").split_once('=') {
                    self.add(secret.trim());
                }
            }
        } else if name.eq_ignore_ascii_case("authorization")
            || name.eq_ignore_ascii_case("proxy-authorization")
        {
            if let Some((_, secret)) = value.split_once(' ') {
                self.add(secret.trim());
            }
        }
    }
    pub fn redact(&self, text: &str) -> String {
        let mut safe = text.to_owned();
        for secret in &self.values {
            safe = safe.replace(secret, crate::services::variables::MASK);
        }
        safe
    }
}
/// URL presentation only. Request/history records retain their original bytes.
/// Bound processing before parsing so a giant imported URL cannot stall layout.
pub fn display_url(url: &str) -> String {
    use crate::services::variables::{MASK, sensitive};
    if url.len() > crate::consts::LABEL_RENDER_LIMIT {
        return "[Long URL hidden; open request to inspect]".into();
    }
    let (without_fragment, fragment) = url
        .split_once('#')
        .map_or((url, None), |(a, b)| (a, Some(b)));
    let (base, query) = without_fragment
        .split_once('?')
        .map_or((without_fragment, None), |(a, b)| (a, Some(b)));
    let mut shown = base.to_owned();
    let authority_start = base.find("://").map_or(0, |i| i + 3);
    let authority_end = base[authority_start..]
        .find('/')
        .map_or(base.len(), |i| authority_start + i);
    if let Some(at) = base[authority_start..authority_end].rfind('@') {
        shown.replace_range(authority_start..authority_start + at, MASK);
    }
    if let Some(query) = query {
        shown.push('?');
        let parts: Vec<String> = query
            .split('&')
            .map(|part| {
                let Some((raw_key, _)) = part.split_once('=') else {
                    return part.to_owned();
                };
                let key = reqwest::Url::parse(&format!("https://display.invalid/?{part}"))
                    .ok()
                    .and_then(|u| u.query_pairs().next().map(|(k, _)| k.into_owned()))
                    .unwrap_or_else(|| raw_key.to_owned());
                if sensitive(&key) {
                    format!("{raw_key}={MASK}")
                } else {
                    part.to_owned()
                }
            })
            .collect();
        shown.push_str(&parts.join("&"));
    }
    if fragment.is_some_and(|s| !s.is_empty()) {
        shown.push('#');
        shown.push_str(MASK);
    }
    shown
}

/// Known credential-named JSON fields in automatically generated previews.
/// Not a universal secret detector; opaque/non-JSON bodies need separate policy.
pub fn redact_json_fields(body: &str) -> String {
    fn walk(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(fields) => {
                for (key, value) in fields {
                    if crate::services::variables::sensitive(key) {
                        *value = serde_json::Value::String(crate::services::variables::MASK.into());
                    } else {
                        walk(value);
                    }
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    walk(value);
                }
            }
            _ => {}
        }
    }
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(mut value) => {
            walk(&mut value);
            value.to_string()
        }
        Err(_) => body.to_owned(),
    }
}
/// Sharing guard for known structured credential fields. This intentionally
/// rejects rather than mutating local requests. It is not a universal DLP scanner.
fn variable_reference(value: &str) -> bool {
    let value = value.trim();
    value
        .strip_prefix("{{")
        .and_then(|s| s.strip_suffix("}}"))
        .is_some_and(|key| !key.trim().is_empty() && !key.contains(['{', '}', '\n', '\r']))
}
fn inline_secret(value: &str) -> bool {
    !value.trim().is_empty() && !variable_reference(value)
}
fn inline_header(value: &str) -> bool {
    let value = value.trim();
    let value = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("Basic "))
        .unwrap_or(value);
    inline_secret(value)
}
fn inline_cookie(value: &str) -> bool {
    if value.trim().is_empty() || variable_reference(value) {
        return false;
    }
    value
        .split(';')
        .filter(|v| !v.trim().is_empty())
        .any(|part| {
            part.split_once('=')
                .is_none_or(|(_, value)| inline_secret(value))
        })
}
fn private_key_material(value: &str) -> bool {
    value.contains("-----BEGIN ") && value.contains("PRIVATE KEY-----")
}
fn json_inline_secret(value: &serde_json::Value, protected: bool) -> bool {
    match value {
        serde_json::Value::Object(fields) => fields.iter().any(|(key, value)| {
            json_inline_secret(
                value,
                protected || crate::services::variables::sensitive(key),
            )
        }),
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| json_inline_secret(value, protected)),
        serde_json::Value::String(value) => {
            private_key_material(value) || (protected && inline_secret(value))
        }
        serde_json::Value::Null => false,
        _ => protected,
    }
}
fn url_inline_secret(url: &str) -> bool {
    let base = url.split(['?', '#']).next().unwrap_or(url);
    let start = base.find("://").map_or(0, |i| i + 3);
    let authority = base[start..].split('/').next().unwrap_or("");
    if let Some((userinfo, _)) = authority.rsplit_once('@') {
        if userinfo.split(':').any(inline_secret) {
            return true;
        }
    }
    for section in [
        url.split_once('?')
            .map(|(_, q)| q.split('#').next().unwrap_or(q)),
        url.split_once('#').map(|(_, f)| f),
    ]
    .into_iter()
    .flatten()
    {
        if let Ok(parsed) = reqwest::Url::parse(&format!("https://sharing.invalid/?{section}")) {
            if parsed.query_pairs().any(|(key, value)| {
                crate::services::variables::sensitive(&key) && inline_secret(&value)
            }) {
                return true;
            }
        }
    }
    false
}

pub fn validate_shared_script(script: &str) -> Result<(), String> {
    if private_key_material(script) {
        return Err("Git publication blocked: private-key material must stay local".into());
    }
    Ok(())
}

pub fn validate_shared_requests(
    requests: &[crate::domain::collection::SavedRequest],
    groups: &[crate::domain::group::ImportedGroup],
) -> Result<(), String> {
    fn request_is_private(req: &crate::domain::collection::SavedRequest) -> bool {
        [
            &req.bearer_token,
            &req.basic_pass,
            &req.api_key_value,
            &req.jwt_secret,
        ]
        .into_iter()
        .any(|v| inline_secret(v))
            || inline_cookie(&req.cookie_string)
            || req.cookies.iter().any(|kv| inline_secret(&kv.value))
            || req.headers.iter().any(|kv| {
                (crate::services::variables::sensitive(&kv.key) || kv.key.contains("{{"))
                    && if kv.key.eq_ignore_ascii_case("cookie") {
                        inline_cookie(&kv.value)
                    } else {
                        inline_header(&kv.value)
                    }
            })
            || req.params.iter().any(|kv| {
                crate::services::variables::sensitive(&kv.key) && inline_secret(&kv.value)
            })
            || req.form_data_fields.iter().any(|f| {
                f.file_data.as_ref().is_some_and(|v| !v.is_empty())
                    || (crate::services::variables::sensitive(&f.key) && inline_secret(&f.value))
            })
            || url_inline_secret(&req.url)
            || [&req.body, &req.graphql_variables].into_iter().any(|body| {
                private_key_material(body)
                    || serde_json::from_str::<serde_json::Value>(body)
                        .is_ok_and(|v| json_inline_secret(&v, false))
            })
            || private_key_material(&req.pre_request_script)
            || private_key_material(&req.test_script)
    }
    let mut batches = vec![requests];
    let mut pending: Vec<_> = groups.iter().collect();
    while let Some(group) = pending.pop() {
        batches.push(group.requests.as_slice());
        pending.extend(&group.groups);
    }
    if batches.into_iter().flatten().any(request_is_private) {
        return Err("Git publication blocked: inline credentials or file data must stay local. Use {{variable}} references for shared credentials; local request values were not changed.".into());
    }
    Ok(())
}
