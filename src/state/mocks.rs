//! Mock servers: their definitions, which ones run, their call logs, and
//! the mock page (a mock's detail, or the create/edit form).
use std::collections::{HashMap, VecDeque};

use crate::domain::mock::MockServer;
use crate::services::mock::{CallLog, SharedRoutes};

/// Calls kept per mock; older ones drop off.
pub const LOG_CAP: usize = 500;

#[derive(Default)]
pub struct MocksState {
    pub servers: Vec<MockServer>,
    /// Started mocks by id. A failed start stays here to show its error
    /// until it is started again or stopped.
    pub running: HashMap<String, Running>,
    /// Newest call first.
    pub logs: HashMap<String, VecDeque<CallLog>>,
    /// Calls and unmatched calls since the mock was started.
    pub stats: HashMap<String, (u64, u64)>,
    pub page: Option<MockPage>,
    pub collapsed: bool,
    pub hovered: Option<String>,
    pub detail_tab: DetailTab,
    pub log_filter: LogFilter,
    pub path_filter: String,
    /// Delete asked on the detail page, waiting for confirmation.
    pub delete_asked: Option<String>,
    /// Bumped per start, so a restart gets a fresh server stream.
    pub generation: u64,
}

pub struct Running {
    pub routes: SharedRoutes,
    pub generation: u64,
    pub status: RunStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunStatus {
    Starting,
    Running,
    Failed(String),
}

impl RunStatus {
    pub fn is_live(&self) -> bool {
        matches!(self, Self::Starting | Self::Running)
    }
}

pub enum MockPage {
    Detail(String),
    Form(MockForm),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DetailTab {
    #[default]
    Calls,
    Routes,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LogFilter {
    #[default]
    All,
    Matched,
    Unmatched,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MockForm {
    /// The mock being edited; `None` creates a new one.
    pub editing: Option<String>,
    pub name: String,
    pub collection_id: Option<String>,
    pub environment_id: Option<String>,
    pub port: String,
    /// Port check, done when the port changes (never per frame).
    pub port_note: Option<PortNote>,
    pub delay_on: bool,
    pub delay: String,
    pub auto_start: bool,
    /// Store the URL as `{{mock_url}}` in the chosen environment.
    pub save_var: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PortNote {
    Free,
    /// Taken by another mock (its name) or another program (`None`).
    Taken(Option<String>),
    Invalid,
}

impl MocksState {
    /// Checks `port` for the form; `editing` is the mock being edited.
    pub fn port_note(&self, port: &str, editing: Option<&str>) -> PortNote {
        let Ok(port) = port.parse::<u16>() else {
            return PortNote::Invalid;
        };
        if port < 1024 {
            return PortNote::Invalid;
        }
        if let Some(other) = self
            .servers
            .iter()
            .find(|m| m.port == port && Some(m.id.as_str()) != editing)
        {
            return PortNote::Taken(Some(other.name.clone()));
        }
        let own_live = editing
            .and_then(|id| self.get(id))
            .is_some_and(|m| m.port == port && self.is_live(&m.id));
        if own_live || crate::services::mock::port_free(port) {
            PortNote::Free
        } else {
            PortNote::Taken(None)
        }
    }

    pub fn get(&self, id: &str) -> Option<&MockServer> {
        self.servers.iter().find(|m| m.id == id)
    }

    pub fn status(&self, id: &str) -> Option<&RunStatus> {
        self.running.get(id).map(|r| &r.status)
    }

    pub fn is_live(&self, id: &str) -> bool {
        self.status(id).is_some_and(RunStatus::is_live)
    }

    pub fn live_count(&self) -> usize {
        self.running.values().filter(|r| r.status.is_live()).count()
    }

    /// The first port from `FIRST_PORT` no mock uses.
    pub fn next_port(&self) -> u16 {
        let mut port = crate::domain::mock::FIRST_PORT;
        while self.servers.iter().any(|m| m.port == port) {
            port += 1;
        }
        port
    }

    pub fn log_call(&mut self, id: &str, call: CallLog) {
        let stats = self.stats.entry(id.to_owned()).or_default();
        stats.0 += 1;
        if call.note.is_some() && call.example.is_none() && call.status >= 400 {
            stats.1 += 1;
        }
        let log = self.logs.entry(id.to_owned()).or_default();
        log.push_front(call);
        log.truncate(LOG_CAP);
    }
}
