use crate::{
    AppState, auth,
    mihomo::{LogEntry, MAX_STREAM_BYTES, Traffic},
};
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::broadcast;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Message as UpstreamMessage, protocol::WebSocketConfig},
};

pub struct SharedStream {
    tx: broadcast::Sender<String>,
    running: AtomicBool,
    logs: bool,
}

impl SharedStream {
    pub fn new(logs: bool) -> Arc<Self> {
        let (tx, receiver) = broadcast::channel(if logs { 256 } else { 16 });
        drop(receiver);
        Arc::new(Self {
            tx,
            running: AtomicBool::new(false),
            logs,
        })
    }
    pub fn subscribe(self: &Arc<Self>, state: AppState) -> broadcast::Receiver<String> {
        let receiver = self.tx.subscribe();
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let stream = self.clone();
            tokio::spawn(async move {
                stream.run(state).await;
            });
        }
        receiver
    }
    fn publish_status(&self, state: &str) {
        let _ = self
            .tx
            .send(json!({"type": "status", "state": state}).to_string());
    }
    async fn run(self: Arc<Self>, state: AppState) {
        let mut changes = state.backend_changes.subscribe();
        let mut backoff = 1u64;
        loop {
            if self.tx.receiver_count() == 0 || *state.shutdown.borrow() {
                self.running.store(false, Ordering::SeqCst);
                // A subscriber arriving during teardown either starts a worker itself or is handled here.
                if !*state.shutdown.borrow()
                    && self.tx.receiver_count() > 0
                    && self
                        .running
                        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok()
                {
                    continue;
                }
                return;
            }
            let client = state.backend.read().await.clone();
            let mut connected = false;
            if let Some(client) = client {
                let request = client.websocket_request(self.logs);
                if let Ok(request) = request {
                    let mut config = WebSocketConfig::default();
                    config.max_message_size = Some(MAX_STREAM_BYTES);
                    config.max_frame_size = Some(MAX_STREAM_BYTES);
                    if let Ok(Ok((mut upstream, _))) = tokio::time::timeout(
                        client.timeout(),
                        connect_async_with_config(request, Some(config), false),
                    )
                    .await
                    {
                        connected = true;
                        backoff = 1;
                        self.publish_status("online");
                        let mut check = tokio::time::interval(Duration::from_secs(1));
                        let mut last_message = tokio::time::Instant::now();
                        loop {
                            tokio::select! {
                                _ = changes.changed() => { let _ = upstream.close(None).await; break; },
                                _ = check.tick() => {
                                    if self.tx.receiver_count() == 0 || *state.shutdown.borrow() { let _ = upstream.close(None).await; break; }
                                    if !self.logs && last_message.elapsed() > Duration::from_secs(15) { break; }
                                },
                                message = upstream.next() => {
                                    match message {
                                        Some(Ok(UpstreamMessage::Text(text))) => {
                                            last_message = tokio::time::Instant::now();
                                            if self.logs {
                                                if let Ok(mut log) = serde_json::from_str::<LogEntry>(&text) {
                                                    log.timestamp = auth::now();
                                                    log.payload = client.redact(&log.payload.chars().take(4096).collect::<String>());
                                                    if !matches!(log.level.as_str(), "debug" | "info" | "warning" | "error" | "warn") { log.level = "info".into(); }
                                                    let _ = self.tx.send(json!({"type": "log", "data": log}).to_string());
                                                } else { self.publish_status("invalid_data"); }
                                            } else if let Ok(traffic) = serde_json::from_str::<Traffic>(&text) {
                                                let _ = self.tx.send(json!({"type": "traffic", "data": traffic}).to_string());
                                            } else { self.publish_status("invalid_data"); }
                                        },
                                        Some(Ok(UpstreamMessage::Ping(data))) => { if upstream.send(UpstreamMessage::Pong(data)).await.is_err() { break; } },
                                        Some(Ok(UpstreamMessage::Close(_))) | Some(Err(_)) | None => break,
                                        _ => {},
                                    }
                                }
                            }
                        }
                    }
                }
            }
            self.publish_status("offline");
            let mut check = tokio::time::interval(Duration::from_secs(1));
            let deadline = tokio::time::sleep(Duration::from_secs(backoff));
            tokio::pin!(deadline);
            loop {
                tokio::select! {
                    _ = changes.changed() => break,
                    _ = &mut deadline => break,
                    _ = check.tick() => { if self.tx.receiver_count() == 0 || *state.shutdown.borrow() { break; } },
                }
            }
            if !connected {
                backoff = (backoff * 2).min(30);
            }
        }
    }
}

pub async fn browser_stream(
    socket: WebSocket,
    state: AppState,
    identity: auth::Identity,
    logs: bool,
) {
    if *state.shutdown.borrow() {
        return;
    }
    let Ok(permit) = state.browser_stream_slots.clone().try_acquire_owned() else {
        return;
    };
    let _permit = permit;
    let stream = if logs {
        &state.log_stream
    } else {
        &state.traffic_stream
    };
    let mut receiver = stream.subscribe(state.clone());
    let (mut sender, mut incoming) = socket.split();
    let mut check = tokio::time::interval(Duration::from_secs(5));
    let mut last_traffic = tokio::time::Instant::now() - Duration::from_secs(10);
    let mut shutdown = state.shutdown.subscribe();
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = check.tick() => {
                let idle = state.settings.read().await.session_idle_seconds;
                // Server-to-browser data is not user activity and never extends the idle timeout.
                if auth::session_by_hash(&state.pool, &identity.session.token_hash, idle, false).await.is_err() { break; }
            },
            message = incoming.next() => {
                match message {
                    Some(Ok(Message::Ping(data))) => { if sender.send(Message::Pong(data)).await.is_err() { break; } },
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    // The protocol is receive-only. Do not accept arbitrary forwarded payloads.
                    Some(Ok(Message::Text(_))) | Some(Ok(Message::Binary(_))) => break,
                    _ => {},
                }
            },
            message = receiver.recv() => {
                match message {
                    Ok(text) => {
                        if !logs && text.contains("\"type\":\"traffic\"") {
                            let interval = state.settings.read().await.traffic_interval_ms;
                            if last_traffic.elapsed() < Duration::from_millis(interval) { continue; }
                            last_traffic = tokio::time::Instant::now();
                        }
                        if !matches!(tokio::time::timeout(Duration::from_secs(5), sender.send(Message::Text(text.into()))).await, Ok(Ok(()))) { break; }
                    },
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if !matches!(tokio::time::timeout(Duration::from_secs(5), sender.send(Message::Text(json!({"type": "status", "state": "lagged"}).to_string().into()))).await, Ok(Ok(()))) { break; }
                    },
                    Err(_) => break,
                }
            }
        }
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), sender.send(Message::Close(None))).await;
}
