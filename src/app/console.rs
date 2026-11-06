//! Fills the Console's log from the send path. Everything is redacted here,
//! before it is stored: the Console is for debugging a script, and a screen
//! share of it should not hand out credentials.
use crate::domain::{
    collection::SavedRequest, environment::AppEnvironment, environment::substitute,
    response::HttpResponse,
};
use crate::services::{format::head_capped, secrets, variables};
use crate::state::console::{ConsoleEntry, ConsoleRequest, MAX_BODY_BYTES, NetworkEntry};

use super::AppState;

/// A `{{$dynamic}}` variable gets a new value on every substitution, so
/// resolving it here would show a value that was not the one sent. Such text
/// stays a template; everything else shows what went out.
fn resolved(text: &str, env: Option<&AppEnvironment>) -> String {
    if text.contains("{{$") {
        text.to_owned()
    } else {
        substitute(text, env)
    }
}

fn bounded_body(body: &str, redactor: &secrets::Redactor) -> String {
    let safe = redactor.redact(&secrets::redact_json_fields(body));
    if safe.len() <= MAX_BODY_BYTES {
        return safe;
    }
    format!(
        "{}\n… truncated, the response pane has the whole body",
        head_capped(&safe, MAX_BODY_BYTES)
    )
}

fn masked_header(key: &str, value: String, redactor: &secrets::Redactor) -> (String, String) {
    let shown = if variables::sensitive(key) {
        variables::MASK.to_owned()
    } else {
        redactor.redact(&value)
    };
    (key.to_owned(), shown)
}

/// Note what is going out, on its tab, until the response arrives; and the
/// log lines of the Before request script, which came first.
pub(crate) fn capture_request(
    state: &mut AppState,
    req: &SavedRequest,
    env: Option<&AppEnvironment>,
) {
    let redactor = super::scripting::diagnostic_redactor(state);
    let tab = state.tabs.active_tab();
    let mut headers: Vec<(String, String)> = req
        .headers
        .iter()
        .filter(|h| h.enabled && !h.key.trim().is_empty())
        .map(|h| masked_header(&h.key, resolved(&h.value, env), &redactor))
        .collect();
    for auto in crate::ui::request::headers::auto_headers(tab) {
        if !auto.overridden {
            headers.push((auto.name, auto.value));
        }
    }
    let captured = ConsoleRequest {
        method: req.method.clone(),
        url: redactor.redact(&resolved(&req.url, env)),
        headers,
        body: bounded_body(&resolved(&req.body, env), &redactor),
    };
    let logs = tab.script_logs.clone();
    state.tabs.active_tab_mut().console_request = Some(captured);
    for line in logs {
        state.console.log(line, false);
    }
}

/// The response's row, then what the After response script wrote.
pub(crate) fn record_response(
    state: &mut AppState,
    tab_id: &str,
    response: &HttpResponse,
    logs: &[String],
    script_error: Option<&str>,
) {
    let redactor = super::scripting::diagnostic_redactor(state);
    let request = state
        .tabs
        .tabs
        .iter_mut()
        .find(|t| t.id == tab_id)
        .and_then(|t| t.console_request.take())
        .unwrap_or_default();
    let mut response_headers: Vec<(String, String)> = response
        .headers
        .iter()
        .map(|(k, v)| masked_header(k, v.clone(), &redactor))
        .collect();
    response_headers.sort();
    let id = state.console.next_id();
    state
        .console
        .push(ConsoleEntry::Network(Box::new(NetworkEntry {
            id,
            request,
            status: response.status,
            status_text: response.status_text.clone(),
            duration_ms: response.duration_ms,
            size: response.body_size,
            error: response.error.as_deref().map(|e| redactor.redact(e)),
            network: response.network.clone(),
            response_headers,
            response_body: if response.is_binary {
                String::new()
            } else {
                bounded_body(&response.body, &redactor)
            },
        })));
    for line in logs {
        state.console.log(line.clone(), false);
    }
    if let Some(error) = script_error {
        state.console.log(error, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::request::KeyValue;

    #[test]
    fn a_request_and_its_response_are_logged_with_secrets_masked() {
        let dir = std::env::temp_dir().join(format!("rustman-console-{}", uuid::Uuid::new_v4()));
        let (mut state, _) = crate::app::boot::init_at(dir.clone());
        let env = AppEnvironment {
            id: "e".into(),
            name: "E".into(),
            variables: [
                ("host".to_owned(), "api.test".to_owned()),
                ("secret_key".to_owned(), "synthetic-secret".to_owned()),
            ]
            .into(),
            is_active: true,
        };
        state.environments.push(env.clone());
        state
            .tabs
            .active_tab_mut()
            .set_script_logs(vec!["from before".into()]);
        let header = |key: &str, value: &str| KeyValue {
            id: "h".into(),
            key: key.into(),
            value: value.into(),
            enabled: true,
            description: String::new(),
        };
        let mut req = crate::app::request_ops::saved_request_from_tab(
            state.tabs.active_tab(),
            String::new(),
            String::new(),
        );
        req.url = "https://{{host}}/a".into();
        req.headers = vec![
            header("X-Trace", "{{$guid}}"),
            header("Authorization", "Bearer synthetic-token"),
        ];
        req.body = "{\"k\":\"{{secret_key}}\"}".into();

        capture_request(&mut state, &req, Some(&env));
        let tab_id = state.tabs.active_tab().id.clone();
        let response = HttpResponse {
            status: 200,
            status_text: "OK".into(),
            body: "x".repeat(MAX_BODY_BYTES + 10),
            headers: [("set-cookie".to_owned(), "sid=synthetic-cookie".to_owned())].into(),
            ..Default::default()
        };
        record_response(
            &mut state,
            &tab_id,
            &response,
            &["from after".into()],
            Some("boom"),
        );

        let kinds: Vec<&ConsoleEntry> = state.console.entries.iter().collect();
        assert!(matches!(kinds[0], ConsoleEntry::Log { text, .. } if text == "from before"));
        let ConsoleEntry::Network(n) = kinds[1] else {
            panic!("{kinds:?}");
        };
        assert_eq!(n.request.url, "https://api.test/a");
        assert_eq!(
            n.request.headers[0].1, "{{$guid}}",
            "not a value never sent"
        );
        assert_eq!(n.request.headers[1].1, variables::MASK);
        assert!(
            !n.request.body.contains("synthetic-secret"),
            "{}",
            n.request.body
        );
        assert_eq!(n.response_headers[0].1, variables::MASK);
        assert!(n.response_body.contains("truncated"));
        assert!(
            matches!(kinds[2], ConsoleEntry::Log { text, error: false } if text == "from after")
        );
        assert!(matches!(kinds[3], ConsoleEntry::Log { text, error: true } if text == "boom"));
        assert!(state.tabs.active_tab().console_request.is_none());
        drop(state);
        let _ = std::fs::remove_dir_all(dir);
    }
}
