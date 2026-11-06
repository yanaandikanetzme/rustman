use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Message as WsMessage, protocol::WebSocketConfig},
};

/// UI/event queues never retain the full large frame. Transport still enforces
/// an 8 MiB wire-message limit; this is an explicitly truncated display preview.
pub fn preview_text(text: &str) -> String {
    const LIMIT: usize = 64 * 1024;
    if text.len() <= LIMIT {
        return text.to_owned();
    }
    format!(
        "{}\n[WebSocket preview truncated]",
        crate::services::format::head_capped(text, LIMIT - 64)
    )
}

#[derive(Debug, Clone)]
pub enum WsEvent {
    Connected,
    Text(String),
    Binary,
    Disconnected,
}

pub struct WsHandle {
    sender: mpsc::Sender<String>,
    task: tokio::task::AbortHandle,
}

impl Drop for WsHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl WsHandle {
    pub async fn send_text(&self, msg: String) {
        let _ = self.sender.send(msg).await;
    }
}

fn ensure_crypto_provider() {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        // Fails only if a provider was already installed, which is fine: the
        // goal is just that *some* provider is in place before a handshake.
        // aws-lc is the single provider linked into the binary (see Cargo
        // unification note) — installing ring here as well would drag the
        // second crypto stack back in.
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

pub async fn connect(url: String) -> Result<(WsHandle, mpsc::Receiver<WsEvent>), String> {
    ensure_crypto_provider();

    let config = WebSocketConfig::default()
        .max_message_size(Some(8 * 1024 * 1024))
        .max_frame_size(Some(2 * 1024 * 1024));
    let (mut stream, _) = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        connect_async_with_config(&url, Some(config), false),
    )
    .await
    .map_err(|_| "WebSocket handshake timed out".to_owned())?
    .map_err(|e| e.to_string())?;
    let (event_tx, event_rx) = mpsc::channel::<WsEvent>(4);
    let (send_tx, mut send_rx) = mpsc::channel::<String>(64);
    let task = tokio::spawn(async move {
        if event_tx.send(WsEvent::Connected).await.is_err() {
            return;
        }
        loop {
            tokio::select! {
                _ = event_tx.closed() => break,
                outgoing = send_rx.recv() => {
                    let Some(msg) = outgoing else { break; };
                    if stream.send(WsMessage::Text(msg.into())).await.is_err() { break; }
                }
                incoming = stream.next() => {
                    let event = match incoming {
                        Some(Ok(WsMessage::Text(t))) => WsEvent::Text(preview_text(t.as_str())),
                        Some(Ok(WsMessage::Binary(_))) => WsEvent::Binary,
                        Some(Ok(WsMessage::Close(_))) | Some(Err(_)) | None => break,
                        _ => continue,
                    };
                    if event_tx.send(event).await.is_err() { break; }
                }
            }
        }
        let _ = event_tx.try_send(WsEvent::Disconnected);
    });
    Ok((
        WsHandle {
            sender: send_tx,
            task: task.abort_handle(),
        },
        event_rx,
    ))
}

#[cfg(test)]
mod security_tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn transport_security_dropping_connection_stops_background_reader() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let peer = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            tokio::time::timeout(Duration::from_secs(1), socket.next()).await
        });
        let (handle, events) = connect(url).await.unwrap();
        drop(events);
        drop(handle);
        assert!(
            peer.await.unwrap().is_ok(),
            "dropped connection retained a live reader task/socket"
        );
    }
}
