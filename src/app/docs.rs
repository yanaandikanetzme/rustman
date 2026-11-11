//! Builds the collection documentation page from the saved requests and
//! their examples, in the sidebar's order.
use crate::domain::collection::SavedRequest;
use crate::domain::request::{AuthType, BodyType, FormFieldType};
use crate::services::format::head_capped;
use crate::services::snippet::Lang;
use crate::state::docs::{CollectionDocs, DocExample, DocRequest, DocSection, MAX_BLOCK_BYTES};

use super::AppState;

/// A credential shown on the page: a `{{variable}}` as written, anything
/// typed as a placeholder (Postman's `<password>`).
fn credential(value: &str, placeholder: &str) -> String {
    let t = value.trim();
    if t.starts_with("{{") && t.ends_with("}}") {
        t.to_owned()
    } else {
        format!("<{placeholder}>")
    }
}

fn plain(value: &str, placeholder: &str) -> String {
    if value.trim().is_empty() {
        format!("<{placeholder}>")
    } else {
        value.to_owned()
    }
}

fn auth_rows(req: &SavedRequest) -> Option<(&'static str, Vec<(String, String)>)> {
    let row = |k: &str, v: String| (k.to_owned(), v);
    Some(match req.auth_type {
        AuthType::None => return None,
        AuthType::Basic => (
            "Basic Auth",
            vec![
                row("Username", plain(&req.basic_user, "username")),
                row("Password", credential(&req.basic_pass, "password")),
            ],
        ),
        AuthType::Bearer => (
            "Bearer Token",
            vec![row("Token", credential(&req.bearer_token, "token"))],
        ),
        AuthType::ApiKey => (
            "API Key",
            vec![
                row("Key", plain(&req.api_key_name, "key")),
                row("Value", credential(&req.api_key_value, "value")),
                row(
                    "Add to",
                    match req.api_key_location {
                        crate::domain::request::ApiKeyLocation::Header => "Header".to_owned(),
                        crate::domain::request::ApiKeyLocation::Query => "Query Params".to_owned(),
                    },
                ),
            ],
        ),
        AuthType::JwtUser => (
            "JWT Bearer",
            vec![
                row("Subject", plain(&req.jwt_subject, "subject")),
                row("Secret", credential(&req.jwt_secret, "secret")),
                row("Algorithm", req.jwt_algo.clone()),
            ],
        ),
        AuthType::Cookie => (
            "Cookie",
            vec![row("Cookie", credential(&req.cookie_string, "cookie"))],
        ),
    })
}

/// JSON reads better indented; anything else stays as written. Capped.
fn block(text: &str) -> String {
    let pretty = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .filter(|v| v.is_object() || v.is_array())
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| text.to_owned());
    if pretty.len() <= MAX_BLOCK_BYTES {
        pretty
    } else {
        format!("{}\n…", head_capped(&pretty, MAX_BLOCK_BYTES))
    }
}

fn body(req: &SavedRequest) -> Option<(String, String)> {
    let form = |label: &str| {
        let lines: Vec<String> = req
            .form_data_fields
            .iter()
            .filter(|f| f.enabled && !f.key.is_empty())
            .map(|f| match f.field_type {
                FormFieldType::File => format!("{}: <file>", f.key),
                FormFieldType::Text => format!("{}: {}", f.key, f.value),
            })
            .collect();
        (!lines.is_empty()).then(|| (label.to_owned(), lines.join("\n")))
    };
    match req.body_type {
        BodyType::None => None,
        BodyType::FormData => form("form-data"),
        BodyType::UrlEncoded => form("x-www-form-urlencoded"),
        _ if req.body.trim().is_empty() => None,
        BodyType::Json => Some(("raw (json)".to_owned(), block(&req.body))),
        BodyType::GraphQL => Some(("GraphQL".to_owned(), block(&req.body))),
        _ => Some(("raw (text)".to_owned(), block(&req.body))),
    }
}

fn doc_request(state: &AppState, req: &SavedRequest, lang: Lang) -> DocRequest {
    let examples = state
        .examples
        .items
        .get(&req.id)
        .map(|list| {
            list.iter()
                .map(|e| {
                    let mut headers: Vec<(String, String)> = e
                        .response
                        .headers
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    headers.sort();
                    DocExample {
                        name: e.name.clone(),
                        snippet: super::snippet::doc_snippet(&e.request, lang),
                        status: e.response.status,
                        status_text: e.response.status_text.clone(),
                        body: block(&e.response.body),
                        headers,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    DocRequest {
        request: req.clone(),
        auth: auth_rows(req),
        body: body(req),
        snippet: super::snippet::doc_snippet(req, lang),
        examples,
    }
}

/// Folders first (each its own section, depth first), then the parent's own
/// requests: the sidebar's order.
fn walk(
    state: &AppState,
    parent: &str,
    is_collection: bool,
    path: Option<String>,
    lang: Lang,
    out: &mut Vec<DocSection>,
) {
    let mut groups: Vec<_> = state
        .groups
        .iter()
        .filter(|g| g.parent_id == parent && g.parent_is_collection == is_collection)
        .collect();
    groups.sort_by_key(|g| g.sort_order);
    let mut own: Vec<&SavedRequest> = state
        .requests
        .values()
        .flatten()
        .filter(|r| {
            if is_collection {
                r.collection_id == parent && r.group_id.is_none()
            } else {
                r.group_id.as_deref() == Some(parent)
            }
        })
        .collect();
    own.sort_by_key(|r| r.sort_order);
    if !own.is_empty() {
        out.push(DocSection {
            title: path.clone(),
            requests: own.iter().map(|r| doc_request(state, r, lang)).collect(),
        });
    }
    for g in groups {
        let title = match &path {
            Some(p) => format!("{p} / {}", g.name),
            None => g.name.clone(),
        };
        walk(state, &g.id, false, Some(title), lang, out);
    }
}

pub(crate) fn build(state: &AppState, collection_id: &str, lang: Lang) -> Option<CollectionDocs> {
    let collection = state.collections.iter().find(|c| c.id == collection_id)?;
    let mut sections = Vec::new();
    let mut folder_count = 0;
    let mut parents = vec![collection_id.to_owned()];
    while let Some(parent) = parents.pop() {
        for g in state.groups.iter().filter(|g| g.parent_id == parent) {
            folder_count += 1;
            parents.push(g.id.clone());
        }
    }
    walk(state, collection_id, true, None, lang, &mut sections);
    // The collection's own requests read first, like an overview.
    sections.sort_by_key(|s| s.title.is_some());
    Some(CollectionDocs {
        collection_id: collection_id.to_owned(),
        name: collection.name.clone(),
        lang,
        request_count: sections.iter().map(|s| s.requests.len()).sum(),
        folder_count,
        created_at: collection.created_at,
        full: false,
        sections,
        expanded: Default::default(),
        chosen: Default::default(),
    })
}

/// Open the page of a collection, keeping the reader's language.
pub(crate) fn open(state: &mut AppState, collection_id: &str) {
    // An example with unsaved edits keeps the screen: leaving it would drop them.
    if state.examples.editor.as_ref().is_some_and(|e| e.dirty) {
        return;
    }
    state.mocks.page = None;
    let lang = state
        .collection_docs
        .as_ref()
        .map_or(Lang::Curl, |d| d.lang);
    let docs = build(state, collection_id, lang);
    if docs.is_some() {
        state.close_environment_workspace();
        state.examples.editor = None;
    }
    state.collection_docs = docs;
}

/// Rebuild in another language, keeping what is expanded and chosen.
pub(crate) fn set_lang(state: &mut AppState, lang: Lang) {
    let Some(old) = state.collection_docs.take() else {
        return;
    };
    let mut docs = build(state, &old.collection_id, lang);
    if let Some(docs) = &mut docs {
        docs.expanded = old.expanded;
        docs.full = old.full;
        docs.chosen = old.chosen;
    }
    state.collection_docs = docs;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{collection::Collection, example::RequestExample, group::Group};

    #[test]
    fn the_page_lists_requests_in_sidebar_order_without_credentials() {
        let dir = std::env::temp_dir().join(format!("rustman-docs-{}", uuid::Uuid::new_v4()));
        let (mut state, _) = crate::app::boot::init_at(dir.clone());
        state.collections.push(Collection {
            id: "c".into(),
            name: "Payments".into(),
            created_at: 0,
        });
        state.groups.push(Group {
            id: "g".into(),
            parent_id: "c".into(),
            parent_is_collection: true,
            name: "Auth".into(),
            sort_order: 0,
        });
        let mut login = SavedRequest::new_in("c".into(), "Login".into());
        login.group_id = Some("g".into());
        login.auth_type = AuthType::Basic;
        login.basic_user = "demo".into();
        login.basic_pass = "synthetic-pass".into();
        login.body_type = BodyType::Json;
        login.body = r#"{"a":1}"#.into();
        let mut health = SavedRequest::new_in("c".into(), "Health".into());
        health.url = "{{host}}/health".into();
        let mut example = RequestExample::new(login.clone(), None);
        example.response.body = r#"{"ok":true}"#.into();
        state.examples.items.insert(login.id.clone(), vec![example]);
        state.requests.insert("c".into(), vec![login, health]);

        open(&mut state, "c");
        let docs = state.collection_docs.as_mut().unwrap();
        assert!(!docs.full, "a collection opens on its Overview");
        assert_eq!((docs.request_count, docs.folder_count), (2, 1));
        docs.full = true;
        let docs = state.collection_docs.as_ref().unwrap();
        assert_eq!(docs.sections[0].title, None, "root requests first");
        assert_eq!(docs.sections[0].requests[0].request.name, "Health");
        assert_eq!(docs.sections[1].title.as_deref(), Some("Auth"));
        let login = &docs.sections[1].requests[0];
        let (kind, rows) = login.auth.clone().unwrap();
        assert_eq!(kind, "Basic Auth");
        assert_eq!(rows[1].1, "<password>");
        assert_eq!(login.body.as_ref().unwrap().1, "{\n  \"a\": 1\n}");
        let page = format!("{} {}", login.snippet, login.examples[0].snippet);
        assert!(!page.contains("synthetic-pass"), "{page}");
        assert_eq!(login.examples[0].body, "{\n  \"ok\": true\n}");

        set_lang(&mut state, Lang::PythonRequests);
        let docs = state.collection_docs.as_ref().unwrap();
        assert!(docs.full, "a language change stays in the documentation");
        assert!(docs.sections[1].requests[0].snippet.contains("requests"));
        drop(state);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn clicking_the_active_tab_leaves_the_overview() {
        let dir = std::env::temp_dir().join(format!("rustman-docs-{}", uuid::Uuid::new_v4()));
        let (mut state, _) = crate::app::boot::init_at(dir.clone());
        state.collections.push(Collection {
            id: "c".into(),
            name: "Payments".into(),
            created_at: 0,
        });
        open(&mut state, "c");
        assert!(state.collection_docs.is_some());
        let active = state.tabs.active;
        let _ = crate::app::update::update(
            &mut state,
            crate::message::Message::Request(crate::message::RequestMsg::TabDragStart(active)),
        );
        assert!(state.collection_docs.is_none(), "the tab's request shows again");
        drop(state);
        let _ = std::fs::remove_dir_all(dir);
    }
}
