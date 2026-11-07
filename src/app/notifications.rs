use crate::{
    message::{AppMsg, Message},
    state::notifications::{Toast, ToastKind, ToastPlacement},
};
use iced::Task;

pub(crate) fn success(
    state: &mut super::AppState,
    title: &str,
    detail: Option<String>,
    placement: ToastPlacement,
) -> Task<Message> {
    let id = uuid::Uuid::new_v4().to_string();
    state.notification = Some(Toast {
        kind: ToastKind::Success,
        id: id.clone(),
        title: title.to_owned(),
        detail,
        placement,
    });
    Task::perform(
        async move {
            tokio::time::sleep(std::time::Duration::from_secs(8)).await;
            id
        },
        |id| Message::App(AppMsg::ToastDismissed(id)),
    )
}

/// Errors remain visible until dismissed; no timer can hide a missed rejection.
pub(crate) fn error(state: &mut super::AppState, title: &str, detail: &str) -> Task<Message> {
    state.notification = Some(Toast {
        kind: ToastKind::Error,
        id: uuid::Uuid::new_v4().to_string(),
        title: title.to_owned(),
        detail: Some(detail.to_owned()),
        placement: ToastPlacement::BottomRight,
    });
    Task::none()
}
