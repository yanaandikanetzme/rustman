//! A local mock server: serves one collection's saved examples on a port.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MockServer {
    pub id: String,
    pub name: String,
    pub collection_id: String,
    /// Resolves `{{vars}}` in example URLs and bodies.
    #[serde(default)]
    pub environment_id: Option<String>,
    pub port: u16,
    /// Wait before each response, in milliseconds (0 = none).
    #[serde(default)]
    pub delay_ms: u64,
    /// Start when Rustman opens.
    #[serde(default)]
    pub auto_start: bool,
    /// Milliseconds since the epoch.
    pub created_at: i64,
}

impl MockServer {
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

/// The variable a created mock can store its URL in.
pub const MOCK_URL_VAR: &str = "mock_url";

/// First port offered for a new mock.
pub const FIRST_PORT: u16 = 4010;
