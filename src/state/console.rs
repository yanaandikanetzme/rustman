//! The Console's log: every request sent and what scripts wrote, across tabs.
//! Memory only, never persisted, and already redacted when it gets here.
use std::collections::HashSet;

use crate::domain::{request::HttpMethod, response::ResponseNetwork};

/// Oldest entries go first past this.
const MAX_ENTRIES: usize = 200;
/// A body kept in the log; the response pane has the whole one.
pub const MAX_BODY_BYTES: usize = 64 * 1024;

impl Default for ConsoleRequest {
    fn default() -> Self {
        Self {
            method: HttpMethod::Get,
            url: String::new(),
            headers: Vec::new(),
            body: String::new(),
        }
    }
}

/// What a request looked like when it was sent, kept on its tab until the
/// response arrives.
#[derive(Debug, Clone)]
pub struct ConsoleRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct NetworkEntry {
    pub id: u64,
    pub request: ConsoleRequest,
    pub status: u16,
    pub status_text: String,
    pub duration_ms: u64,
    pub size: usize,
    pub error: Option<String>,
    pub network: ResponseNetwork,
    pub response_headers: Vec<(String, String)>,
    pub response_body: String,
}

#[derive(Debug, Clone)]
pub enum ConsoleEntry {
    Log { text: String, error: bool },
    Network(Box<NetworkEntry>),
}

/// The panel's log filter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ConsoleFilter {
    #[default]
    All,
    Network,
    Logs,
    Errors,
}

impl ConsoleFilter {
    pub const ALL: [Self; 4] = [Self::All, Self::Network, Self::Logs, Self::Errors];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All Logs",
            Self::Network => "Network",
            Self::Logs => "Logs",
            Self::Errors => "Errors",
        }
    }

    pub fn shows(self, entry: &ConsoleEntry) -> bool {
        match (self, entry) {
            (Self::All, _)
            | (Self::Network, ConsoleEntry::Network(_))
            | (Self::Logs, ConsoleEntry::Log { .. }) => true,
            (Self::Errors, ConsoleEntry::Log { error, .. }) => *error,
            (Self::Errors, ConsoleEntry::Network(n)) => n.error.is_some() || n.status >= 400,
            _ => false,
        }
    }
}

#[derive(Debug, Default)]
pub struct ConsoleState {
    pub entries: Vec<ConsoleEntry>,
    pub filter: ConsoleFilter,
    /// Expanded rows and sections: `"{id}"`, `"{id}:{section}"`.
    pub expanded: HashSet<String>,
    next_id: u64,
}

impl ConsoleState {
    pub fn log(&mut self, text: impl Into<String>, error: bool) {
        self.push(ConsoleEntry::Log {
            text: text.into(),
            error,
        });
    }

    pub fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub fn push(&mut self, entry: ConsoleEntry) {
        self.entries.push(entry);
        if self.entries.len() > MAX_ENTRIES {
            let excess = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(..excess);
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.expanded.clear();
    }

    pub fn toggle(&mut self, key: String) {
        if !self.expanded.remove(&key) {
            self.expanded.insert(key);
        }
    }

    /// The whole log as text, for the Copy button.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        let lines = |out: &mut String, title: &str, pairs: &[(String, String)]| {
            if !pairs.is_empty() {
                out.push_str(&format!("{title}\n"));
                for (key, value) in pairs {
                    out.push_str(&format!("  {key}: {value}\n"));
                }
            }
        };
        for entry in &self.entries {
            match entry {
                ConsoleEntry::Log { text, .. } => {
                    out.push_str(text);
                    out.push('\n');
                }
                ConsoleEntry::Network(n) => {
                    out.push_str(&format!(
                        "{} {} {} {} {} ms\n",
                        n.request.method.as_str(),
                        n.request.url,
                        n.status,
                        n.status_text,
                        n.duration_ms
                    ));
                    if let Some(error) = &n.error {
                        out.push_str(&format!("Error: {error}\n"));
                    }
                    lines(&mut out, "Request Headers", &n.request.headers);
                    if !n.request.body.is_empty() {
                        out.push_str(&format!("Request Body\n{}\n", n.request.body));
                    }
                    lines(&mut out, "Response Headers", &n.response_headers);
                    if !n.response_body.is_empty() {
                        out.push_str(&format!("Response Body\n{}\n", n.response_body));
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_pick_their_entries() {
        let log = ConsoleEntry::Log {
            text: "a".into(),
            error: false,
        };
        let failed = ConsoleEntry::Log {
            text: "b".into(),
            error: true,
        };
        assert!(ConsoleFilter::All.shows(&log));
        assert!(ConsoleFilter::Logs.shows(&failed));
        assert!(!ConsoleFilter::Network.shows(&log));
        assert!(!ConsoleFilter::Errors.shows(&log));
        assert!(ConsoleFilter::Errors.shows(&failed));
    }

    #[test]
    fn the_log_is_bounded_and_clear_forgets_expansion() {
        let mut console = ConsoleState::default();
        for i in 0..MAX_ENTRIES + 5 {
            console.log(format!("line {i}"), false);
        }
        assert_eq!(console.entries.len(), MAX_ENTRIES);
        assert!(console.to_text().starts_with("line 5\n"));
        console.toggle("1".into());
        assert!(console.expanded.contains("1"));
        console.toggle("1".into());
        assert!(console.expanded.is_empty());
        console.toggle("1".into());
        console.clear();
        assert!(console.entries.is_empty() && console.expanded.is_empty());
    }
}
