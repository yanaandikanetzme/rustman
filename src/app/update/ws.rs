use iced::Task;

use crate::app::AppState;
use crate::message::{Message, WsMsg};

pub(super) fn handle(state: &mut AppState, msg: WsMsg) -> Task<Message> {
    let (tab_id, connection_id) = match &msg {
        WsMsg::Handshake {
            tab_id,
            connection_id,
            ..
        }
        | WsMsg::TextFrame {
            tab_id,
            connection_id,
            ..
        }
        | WsMsg::Disconnected {
            tab_id,
            connection_id,
        }
        | WsMsg::Error {
            tab_id,
            connection_id,
            ..
        } => (tab_id, connection_id),
    };
    let Some(tab) = state.tabs.tabs.iter_mut().find(|tab| &tab.id == tab_id) else {
        return Task::none();
    };
    // Dropping a rejected Handshake also drops its sender. Queued events from
    // an earlier attempt must never revive or overwrite the current socket.
    if tab.ws.connection_id != Some(*connection_id) || !(tab.ws.connecting || tab.ws.connected) {
        return Task::none();
    }
    match msg {
        WsMsg::Handshake { sender, .. } => {
            tab.ws.connecting = false;
            tab.ws.connected = true;
            tab.ws.outgoing_tx = Some(sender);
        }
        WsMsg::TextFrame { text, .. } => {
            tab.ws.push_message(crate::state::tabs::WsMessage {
                text,
                is_outgoing: false,
            });
        }
        WsMsg::Disconnected { .. } | WsMsg::Error { .. } => {
            tab.ws.connection_id = None;
            tab.ws.connected = false;
            tab.ws.connecting = false;
            tab.ws.outgoing_tx = None;
            if let WsMsg::Error { error, .. } = msg {
                tab.ws.push_message(crate::state::tabs::WsMessage {
                    text: format!("Error: {error}"),
                    is_outgoing: false,
                });
            }
        }
    }
    Task::none()
}
#[cfg(test)]
mod security_tests {
    use super::*;
    #[test]
    fn security_websocket_old_attempt_cannot_replace_or_disconnect_reconnect() {
        let dir =
            std::env::temp_dir().join(format!("rustman-ws-reconnect-{}", uuid::Uuid::new_v4()));
        let (mut state, _) = crate::app::boot::init_at(dir.clone());
        let tab_id = state.tabs.active_tab().id.clone();
        state
            .tabs
            .active_tab_mut()
            .set_url("ws://synthetic.invalid/socket".into());
        let _ = super::super::request::handle(&mut state, crate::message::RequestMsg::WsConnect);
        let old_id = state.tabs.active_tab().ws.connection_id.unwrap();
        let _ = super::super::request::handle(&mut state, crate::message::RequestMsg::WsDisconnect);
        let _ = super::super::request::handle(&mut state, crate::message::RequestMsg::WsConnect);
        let new_id = state.tabs.active_tab().ws.connection_id.unwrap();
        assert_ne!(
            old_id, new_id,
            "same URL must start a distinct subscription"
        );
        let (current, mut current_rx) = tokio::sync::mpsc::channel(1);
        let _ = handle(
            &mut state,
            WsMsg::Handshake {
                tab_id: tab_id.clone(),
                connection_id: new_id,
                sender: current,
            },
        );
        let (stale, mut stale_rx) = tokio::sync::mpsc::channel(1);
        for event in [
            WsMsg::Handshake {
                tab_id: tab_id.clone(),
                connection_id: old_id,
                sender: stale,
            },
            WsMsg::TextFrame {
                tab_id: tab_id.clone(),
                connection_id: old_id,
                text: "old endpoint data".into(),
            },
            WsMsg::Error {
                tab_id: tab_id.clone(),
                connection_id: old_id,
                error: "old failure".into(),
            },
            WsMsg::Disconnected {
                tab_id: tab_id.clone(),
                connection_id: old_id,
            },
        ] {
            let _ = handle(&mut state, event);
        }
        let ws = &state.tabs.active_tab().ws;
        assert!(ws.connected);
        assert_eq!(ws.connection_id, Some(new_id));
        assert!(
            ws.messages.is_empty(),
            "old endpoint contaminated new history"
        );
        ws.outgoing_tx
            .as_ref()
            .unwrap()
            .try_send("current destination".into())
            .unwrap();
        assert_eq!(current_rx.try_recv().unwrap(), "current destination");
        assert!(matches!(
            stale_rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected)
        ));
        let _ = handle(
            &mut state,
            WsMsg::TextFrame {
                tab_id: tab_id.clone(),
                connection_id: new_id,
                text: "current response".into(),
            },
        );
        assert_eq!(
            state.tabs.active_tab().ws.messages[0].text,
            "current response"
        );
        let _ = handle(
            &mut state,
            WsMsg::Disconnected {
                tab_id,
                connection_id: new_id,
            },
        );
        assert!(!state.tabs.active_tab().ws.connected);
        assert!(state.tabs.active_tab().ws.outgoing_tx.is_none());
        drop(state);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn security_websocket_late_handshake_cannot_revive_disconnected_tab() {
        let dir = std::env::temp_dir().join(format!("rustman-ws-stale-{}", uuid::Uuid::new_v4()));
        let (mut state, _) = crate::app::boot::init_at(dir.clone());
        let tab_id = state.tabs.active_tab().id.clone();
        let _ = super::super::request::handle(&mut state, crate::message::RequestMsg::WsConnect);
        let connection_id = state.tabs.active_tab().ws.connection_id.unwrap();
        let _ = super::super::request::handle(&mut state, crate::message::RequestMsg::WsDisconnect);
        let (sender, _receiver) = tokio::sync::mpsc::channel(1);
        let _ = handle(
            &mut state,
            WsMsg::Handshake {
                tab_id,
                connection_id,
                sender,
            },
        );
        let ws = &state.tabs.active_tab().ws;
        assert!(
            !ws.connected,
            "late handshake revived a disconnected socket"
        );
        assert!(ws.outgoing_tx.is_none());
        drop(state);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn security_websocket_frames_stay_with_source_tab_and_ignore_closed_tabs() {
        let dir = std::env::temp_dir().join(format!("rustman-ws-tabs-{}", uuid::Uuid::new_v4()));
        let (mut state, _) = crate::app::boot::init_at(dir.clone());
        let source = state.tabs.active_tab().id.clone();
        let connection_id = uuid::Uuid::new_v4();
        state.tabs.active_tab_mut().ws.connection_id = Some(connection_id);
        state.tabs.active_tab_mut().ws.connected = true;
        state
            .tabs
            .tabs
            .push(crate::state::tabs::RequestTabState::new());
        state.tabs.active = 1;
        let _ = handle(
            &mut state,
            WsMsg::TextFrame {
                tab_id: source.clone(),
                connection_id,
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
                connection_id,
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
        let connection_id = uuid::Uuid::new_v4();
        state.tabs.active_tab_mut().ws.connection_id = Some(connection_id);
        state.tabs.active_tab_mut().ws.connected = true;
        for _ in 0..1100 {
            let _ = handle(
                &mut state,
                WsMsg::TextFrame {
                    tab_id: tab_id.clone(),
                    connection_id,
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
