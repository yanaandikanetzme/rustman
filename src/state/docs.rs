//! The collection documentation page (Postman's collection Overview with its
//! complete documentation). Built once when it opens or its language
//! changes: a snippet needs a full request rebuild, too costly per frame.
use std::collections::{HashMap, HashSet};

use crate::domain::collection::SavedRequest;
use crate::services::snippet::Lang;

/// Code blocks show this many lines until "View more".
pub const COLLAPSED_LINES: usize = 14;
/// A body kept on the page; the request or example has the whole one.
pub const MAX_BLOCK_BYTES: usize = 32 * 1024;

pub struct CollectionDocs {
    pub collection_id: String,
    pub name: String,
    pub lang: Lang,
    pub request_count: usize,
    pub folder_count: usize,
    /// As stored: milliseconds, or seconds for older imports.
    pub created_at: i64,
    /// The complete documentation is open; otherwise the Overview.
    pub full: bool,
    pub sections: Vec<DocSection>,
    /// Expanded code blocks and Headers tabs, by key.
    pub expanded: HashSet<String>,
    /// The example shown per request id, when it has several.
    pub chosen: HashMap<String, usize>,
}

/// The requests of one folder (`title` its path), or of the collection's
/// root (`title` `None`).
pub struct DocSection {
    pub title: Option<String>,
    pub requests: Vec<DocRequest>,
}

pub struct DocRequest {
    pub request: SavedRequest,
    /// The auth type's name and its fields; credentials stay placeholders.
    pub auth: Option<(&'static str, Vec<(String, String)>)>,
    /// How the body is sent ("raw (json)") and the body itself.
    pub body: Option<(String, String)>,
    /// The request itself as code, for requests without an example.
    pub snippet: String,
    pub examples: Vec<DocExample>,
}

pub struct DocExample {
    pub name: String,
    pub snippet: String,
    pub status: u16,
    pub status_text: String,
    pub body: String,
    pub headers: Vec<(String, String)>,
}

impl CollectionDocs {
    pub fn toggle(&mut self, key: String) {
        if !self.expanded.remove(&key) {
            self.expanded.insert(key);
        }
    }
}
