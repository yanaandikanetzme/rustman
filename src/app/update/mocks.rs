//! Mock server messages: sidebar section, mock page, form and server events.
use iced::Task;

use crate::app::AppState;
use crate::domain::mock::{MOCK_URL_VAR, MockServer};
use crate::message::{Message, mock::MockMsg};
use crate::services::{mock::MockEvent, storage};
use crate::state::mocks::{MockForm, MockPage, RunStatus};

pub(super) fn handle(state: &mut AppState, msg: MockMsg) -> Task<Message> {
    match msg {
        MockMsg::SectionToggled => state.mocks.collapsed = !state.mocks.collapsed,
        MockMsg::Hovered(id) => state.mocks.hovered = id,
        MockMsg::CreateOpened => {
            let collection_id = state.collections.first().map(|c| c.id.clone());
            let environment_id = state.active_env().map(|e| e.id.clone());
            let name = collection_id
                .as_ref()
                .and_then(|id| state.collections.iter().find(|c| &c.id == id))
                .map(|c| format!("{} mock", c.name))
                .unwrap_or_default();
            open_page(
                state,
                MockPage::Form(MockForm {
                    name,
                    collection_id,
                    save_var: environment_id.is_some(),
                    environment_id,
                    port: state.mocks.next_port().to_string(),
                    delay: "300".into(),
                    ..Default::default()
                }),
            );
        }
        MockMsg::EditOpened(id) => {
            if let Some(mock) = state.mocks.get(&id).cloned() {
                open_page(
                    state,
                    MockPage::Form(MockForm {
                        editing: Some(mock.id),
                        name: mock.name,
                        collection_id: Some(mock.collection_id),
                        environment_id: mock.environment_id,
                        port: mock.port.to_string(),
                        delay_on: mock.delay_ms > 0,
                        delay: if mock.delay_ms > 0 {
                            mock.delay_ms.to_string()
                        } else {
                            "300".into()
                        },
                        auto_start: mock.auto_start,
                        save_var: false,
                        port_note: None,
                        error: None,
                    }),
                );
            }
        }
        MockMsg::Opened(id) => {
            if state.mocks.get(&id).is_some() {
                open_page(state, MockPage::Detail(id));
            }
        }
        MockMsg::PageClosed => {
            state.mocks.page = None;
            state.mocks.delete_asked = None;
        }
        MockMsg::FormName(v) => form(state, |f| f.name = v),
        MockMsg::FormCollection(id) => {
            let name = state
                .collections
                .iter()
                .find(|c| c.id == id)
                .map(|c| format!("{} mock", c.name));
            form(state, |f| {
                // Keep a typed name; replace the suggested one.
                if f.name.trim().is_empty() || f.name.ends_with(" mock") {
                    if let Some(name) = name {
                        f.name = name;
                    }
                }
                f.collection_id = Some(id);
            });
        }
        MockMsg::FormEnvironment(id) => form(state, |f| {
            f.save_var = f.save_var && id.is_some();
            f.environment_id = id;
        }),
        MockMsg::FormPort(v) => {
            form(state, |f| {
                f.port = v.chars().filter(char::is_ascii_digit).take(5).collect()
            });
            refresh_port_note(state);
        }
        MockMsg::FormDelayToggled(on) => form(state, |f| f.delay_on = on),
        MockMsg::FormDelay(v) => form(state, |f| {
            f.delay = v.chars().filter(char::is_ascii_digit).take(6).collect()
        }),
        MockMsg::FormAutoStartToggled(on) => form(state, |f| f.auto_start = on),
        MockMsg::FormSaveVarToggled(on) => form(state, |f| f.save_var = on),
        MockMsg::FormSubmitted => submit(state),
        MockMsg::Start(id) => {
            crate::app::mocks::start(state, &id);
        }
        MockMsg::Stop(id) => {
            crate::app::mocks::stop(state, &id);
            state.status_message = Some("Mock server stopped".into());
        }
        MockMsg::DeleteAsked(id) => state.mocks.delete_asked = Some(id),
        MockMsg::DeleteCancelled => state.mocks.delete_asked = None,
        MockMsg::DeleteConfirmed => {
            if let Some(id) = state.mocks.delete_asked.take() {
                if let Some(db) = &state.db
                    && let Err(e) = storage::mocks::delete(db, &id)
                {
                    state.status_message = Some(format!("Could not delete the mock: {e}"));
                    return Task::none();
                }
                crate::app::mocks::stop(state, &id);
                state.mocks.servers.retain(|m| m.id != id);
                state.mocks.logs.remove(&id);
                state.mocks.stats.remove(&id);
                state.mocks.page = None;
                state.status_message = Some("Mock server deleted".into());
            }
        }
        MockMsg::CopyUrl(id) => {
            if let Some(mock) = state.mocks.get(&id) {
                let url = mock.url();
                state.status_message = Some("Mock URL copied".into());
                return iced::clipboard::write::<Message>(url);
            }
        }
        MockMsg::NewRequest(id) => {
            if let Some(url) = state.mocks.get(&id).map(|m| format!("{}/", m.url())) {
                state.mocks.page = None;
                state.tabs.new_tab();
                state.tabs.active_tab_mut().set_url(url);
                crate::app::session::persist_session(state);
            }
        }
        MockMsg::ClearLog(id) => {
            state.mocks.logs.remove(&id);
            state.mocks.stats.insert(id, (0, 0));
        }
        MockMsg::DetailTabSelected(tab) => state.mocks.detail_tab = tab,
        MockMsg::LogFilterSelected(filter) => state.mocks.log_filter = filter,
        MockMsg::PathFilterChanged(v) => state.mocks.path_filter = v,
        MockMsg::Event(id, generation, event) => {
            // Events of an earlier start (or a stopped mock) are dropped.
            let Some(running) = state
                .mocks
                .running
                .get_mut(&id)
                .filter(|r| r.generation == generation)
            else {
                return Task::none();
            };
            match event {
                MockEvent::Started => running.status = RunStatus::Running,
                MockEvent::Failed(e) => {
                    state.status_message = Some(e.clone());
                    running.status = RunStatus::Failed(e);
                }
                MockEvent::Call(call) => state.mocks.log_call(&id, call),
            }
        }
    }
    Task::none()
}

/// One page in the main area at a time: the mock page replaces the
/// environment and documentation pages.
fn open_page(state: &mut AppState, page: MockPage) {
    state.close_environment_workspace();
    state.mocks.delete_asked = None;
    state.mocks.page = Some(page);
    refresh_port_note(state);
}

fn refresh_port_note(state: &mut AppState) {
    let Some(MockPage::Form(form)) = &state.mocks.page else {
        return;
    };
    let note = state
        .mocks
        .port_note(&form.port, form.editing.as_deref());
    if let Some(MockPage::Form(form)) = &mut state.mocks.page {
        form.port_note = Some(note);
    }
}

fn form(state: &mut AppState, f: impl FnOnce(&mut MockForm)) {
    if let Some(MockPage::Form(form)) = &mut state.mocks.page {
        f(form);
        form.error = None;
    }
}

fn set_error(state: &mut AppState, error: impl Into<String>) {
    if let Some(MockPage::Form(form)) = &mut state.mocks.page {
        form.error = Some(error.into());
    }
}

/// Validates the form; saves and starts the mock.
fn submit(state: &mut AppState) {
    let Some(MockPage::Form(form)) = &state.mocks.page else {
        return;
    };
    let form = form.clone();
    let name = form.name.trim();
    if name.is_empty() {
        return set_error(state, "Give the mock a name.");
    }
    let Some(collection_id) = form
        .collection_id
        .clone()
        .filter(|id| state.collections.iter().any(|c| &c.id == id))
    else {
        return set_error(state, "Pick the collection to mock.");
    };
    let port = match form.port.parse::<u16>() {
        Ok(p) if p >= 1024 => p,
        _ => return set_error(state, "Use a port from 1024 to 65535."),
    };
    let editing = form.editing.as_deref();
    if let Some(other) = state
        .mocks
        .servers
        .iter()
        .find(|m| m.port == port && Some(m.id.as_str()) != editing)
    {
        return set_error(state, format!("Port {port} belongs to \"{}\".", other.name));
    }
    let own_live_port = editing
        .and_then(|id| state.mocks.get(id))
        .is_some_and(|m| m.port == port && state.mocks.is_live(&m.id));
    if !own_live_port && !crate::services::mock::port_free(port) {
        return set_error(state, format!("Port {port} is in use by another program."));
    }
    let delay_ms = if form.delay_on {
        form.delay.parse::<u64>().unwrap_or(0).min(60_000)
    } else {
        0
    };
    let previous = editing.and_then(|id| state.mocks.get(id)).cloned();
    let mock = MockServer {
        id: previous
            .as_ref()
            .map(|m| m.id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        name: name.to_owned(),
        collection_id,
        environment_id: form.environment_id.clone(),
        port,
        delay_ms,
        auto_start: form.auto_start,
        created_at: previous
            .as_ref()
            .map(|m| m.created_at)
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis()),
    };
    if let Some(db) = &state.db
        && let Err(e) = storage::mocks::save(db, &mock)
    {
        return set_error(state, format!("Could not save the mock: {e}"));
    }
    if form.save_var
        && let Some(env) = form
            .environment_id
            .as_ref()
            .and_then(|id| state.environments.iter_mut().find(|e| &e.id == id))
    {
        env.variables.insert(MOCK_URL_VAR.to_owned(), mock.url());
        if let Some(db) = &state.db {
            let _ = storage::save_environment(db, env);
        }
    }
    let id = mock.id.clone();
    let was_live = state.mocks.is_live(&id);
    match state.mocks.servers.iter_mut().find(|m| m.id == id) {
        Some(existing) => *existing = mock,
        None => state.mocks.servers.push(mock),
    }
    // A new mock starts at once; an edited one restarts if it was running.
    if previous.is_none() || was_live {
        crate::app::mocks::start(state, &id);
    }
    state.mocks.collapsed = false;
    state.mocks.page = Some(MockPage::Detail(id));
    state.status_message = Some(if previous.is_some() {
        "Mock server saved".into()
    } else {
        "Mock server created".into()
    });
}
