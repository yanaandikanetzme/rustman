//! Running mock servers: start, stop, route refresh and their subscriptions.
use std::hash::{Hash, Hasher};
use std::sync::{Arc, RwLock};

use iced::Subscription;

use super::AppState;
use crate::message::{Message, mock::MockMsg};
use crate::services::mock::{Route, SharedRoutes, build_routes};
use crate::state::mocks::{RunStatus, Running};

/// The routes a mock answers right now, from its collection's saved
/// requests and examples.
pub(crate) fn routes_for(state: &AppState, id: &str) -> Vec<Route> {
    let Some(mock) = state.mocks.get(id) else {
        return Vec::new();
    };
    let env = mock
        .environment_id
        .as_ref()
        .and_then(|e| state.environments.iter().find(|env| &env.id == e));
    let requests = state
        .requests
        .get(&mock.collection_id)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    build_routes(requests, &state.examples.items, env)
}

/// Starts (or restarts) a mock. The server itself runs in its subscription.
pub(crate) fn start(state: &mut AppState, id: &str) {
    let Some(mock) = state.mocks.get(id) else {
        return;
    };
    if !state.collections.iter().any(|c| c.id == mock.collection_id) {
        let msg = "Its collection was deleted. Edit the mock to pick another.".to_owned();
        state.mocks.running.insert(
            id.to_owned(),
            Running {
                routes: Arc::new(RwLock::new(Vec::new())),
                generation: 0,
                status: RunStatus::Failed(msg),
            },
        );
        return;
    }
    let routes = routes_for(state, id);
    state.mocks.generation += 1;
    state.mocks.running.insert(
        id.to_owned(),
        Running {
            routes: Arc::new(RwLock::new(routes)),
            generation: state.mocks.generation,
            status: RunStatus::Starting,
        },
    );
    state.mocks.stats.insert(id.to_owned(), (0, 0));
}

/// Stopping drops the subscription, which closes the port.
pub(crate) fn stop(state: &mut AppState, id: &str) {
    state.mocks.running.remove(id);
}

pub(crate) fn start_on_launch(state: &mut AppState) {
    let ids: Vec<String> = state
        .mocks
        .servers
        .iter()
        .filter(|m| m.auto_start)
        .map(|m| m.id.clone())
        .collect();
    for id in ids {
        start(state, &id);
    }
}

/// Saved requests or examples changed: running mocks answer with them at
/// once, no restart needed.
pub(crate) fn refresh_routes(state: &mut AppState) {
    let ids: Vec<String> = state
        .mocks
        .running
        .iter()
        .filter(|(_, r)| r.status.is_live())
        .map(|(id, _)| id.clone())
        .collect();
    for id in ids {
        let routes = routes_for(state, &id);
        if let Some(running) = state.mocks.running.get(&id)
            && let Ok(mut shared) = running.routes.write()
        {
            *shared = routes;
        }
    }
}

/// Identity of a running server: a new start, port or delay restarts it.
struct MockRun {
    id: String,
    port: u16,
    delay_ms: u64,
    generation: u64,
    routes: SharedRoutes,
}

impl Hash for MockRun {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "mock-server".hash(state);
        self.id.hash(state);
        self.port.hash(state);
        self.delay_ms.hash(state);
        self.generation.hash(state);
    }
}

fn mock_stream(run: &MockRun) -> impl iced::futures::Stream<Item = Message> + use<> {
    use iced::futures::StreamExt as _;
    let id = run.id.clone();
    let generation = run.generation;
    crate::services::mock::serve(run.port, run.routes.clone(), run.delay_ms)
        .map(move |event| Message::Mock(MockMsg::Event(id.clone(), generation, event)))
}

pub(crate) fn subscriptions(state: &AppState) -> Vec<Subscription<Message>> {
    state
        .mocks
        .running
        .iter()
        .filter(|(_, r)| r.status.is_live())
        .filter_map(|(id, r)| {
            let mock = state.mocks.get(id)?;
            Some(Subscription::run_with(
                MockRun {
                    id: id.clone(),
                    port: mock.port,
                    delay_ms: mock.delay_ms,
                    generation: r.generation,
                    routes: r.routes.clone(),
                },
                mock_stream,
            ))
        })
        .collect()
}
