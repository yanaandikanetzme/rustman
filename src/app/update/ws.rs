use iced::Task;

use crate::app::AppState;
use crate::message::{Message, WsMsg};

pub(super) fn handle(state: &mut AppState, msg: WsMsg) -> Task<Message> {
    match msg {
        WsMsg::Handshake { tab_id, sender } => {
            if let Some(tab) = state.tabs.tabs.iter_mut().find(|t| t.id == tab_id) {
                tab.ws.connecting = false;
                tab.ws.connected = true;
                tab.ws.outgoing_tx = Some(sender);
            }
        }
        WsMsg::TextFrame { tab_id, text } => {
            let Some(tab) = state.tabs.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                return Task::none();
            };
            tab.ws.push_message(crate::state::tabs::WsMessage {
                text,
                is_outgoing: false,
            });
        }
        WsMsg::Disconnected { tab_id } => {
            let Some(tab) = state.tabs.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                return Task::none();
            };
            tab.ws.connected = false;
            tab.ws.connecting = false;
            tab.ws.outgoing_tx = None;
        }
        WsMsg::Error { tab_id, error: e } => {
            let Some(tab) = state.tabs.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                return Task::none();
            };
            tab.ws.connected = false;
            tab.ws.connecting = false;
            tab.ws.outgoing_tx = None;
            tab.ws.push_message(crate::state::tabs::WsMessage {
                text: format!("Error: {e}"),
                is_outgoing: false,
            });
        }
    }
    Task::none()
}
#[cfg(test)]
mod security_tests {
    use super::*;
    #[test]
    fn security_websocket_frames_stay_with_source_tab_and_ignore_closed_tabs() {
        let dir = std::env::temp_dir().join(format!("rustman-ws-tabs-{}", uuid::Uuid::new_v4()));
        let (mut state, _) = crate::app::boot::init_at(dir.clone());
        let source = state.tabs.active_tab().id.clone();
        state
            .tabs
            .tabs
            .push(crate::state::tabs::RequestTabState::new());
        state.tabs.active = 1;
        let _ = handle(
            &mut state,
            WsMsg::TextFrame {
                tab_id: source.clone(),
                text: "source-only".into(),
            },
        );
        assert_eq!(state.tabs.tabs[0].ws.messages[0].text, "source-only");
        assert!(state.tabs.tabs[1].ws.messages.is_empty());
        state.tabs.tabs.remove(0);
        state.tabs.active = 0;
        let _ = handle(
            &mut state,
            WsMsg::TextFrame {
                tab_id: source,
                text: "closed-source".into(),
            },
        );
        assert!(state.tabs.active_tab().ws.messages.is_empty());
        drop(state);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn security_websocket_history_is_bounded() {
        let dir = std::env::temp_dir().join(format!("rustman-ws-history-{}", uuid::Uuid::new_v4()));
        let (mut state, _) = crate::app::boot::init_at(dir.clone());
        let tab_id = state.tabs.active_tab().id.clone();
        for _ in 0..1100 {
            let _ = handle(
                &mut state,
                WsMsg::TextFrame {
                    tab_id: tab_id.clone(),
                    text: "synthetic-frame".repeat(10),
                },
            );
        }
        assert!(state.tabs.active_tab().ws.messages.len() <= 500);
        assert!(
            state
                .tabs
                .active_tab()
                .ws
                .messages
                .iter()
                .map(|m| m.text.len())
                .sum::<usize>()
                <= 1024 * 1024
        );
        drop(state);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
