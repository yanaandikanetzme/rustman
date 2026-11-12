//! Mock server messages: the sidebar section, the mock page and the
//! running servers' events.
use crate::services::mock::MockEvent;
use crate::state::mocks::{DetailTab, LogFilter};

#[derive(Debug, Clone)]
pub enum MockMsg {
    SectionToggled,
    Hovered(Option<String>),
    /// Open the create form.
    CreateOpened,
    /// Open a mock's detail page.
    Opened(String),
    PageClosed,
    EditOpened(String),
    FormName(String),
    FormCollection(String),
    /// `None` = no environment.
    FormEnvironment(Option<String>),
    FormPort(String),
    FormDelayToggled(bool),
    FormDelay(String),
    FormAutoStartToggled(bool),
    FormSaveVarToggled(bool),
    FormSubmitted,
    Start(String),
    Stop(String),
    DeleteAsked(String),
    DeleteConfirmed,
    DeleteCancelled,
    CopyUrl(String),
    /// A new request tab pointed at the mock.
    NewRequest(String),
    ClearLog(String),
    DetailTabSelected(DetailTab),
    LogFilterSelected(LogFilter),
    PathFilterChanged(String),
    /// From a running server: (mock id, start generation, event).
    Event(String, u64, MockEvent),
}
