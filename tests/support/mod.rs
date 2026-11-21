#![allow(dead_code)]
use axum::{
    Json, Router,
    extract::{Path, State, WebSocketUpgrade, ws::Message},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Mutex;

// Public test fixture; the Mock only accepts this value on its loopback API.
pub const SECRET: &str = "mock-controller-secret-for-tests";
pub const NODE_A: &str = "香港 · HK 01";
pub const NODE_B: &str = "日本 / Tokyo 02";

#[derive(Clone)]
pub struct MockState {
    pub selected: Arc<Mutex<String>>,
    pub mode: Arc<Mutex<String>>,
    pub behavior: Arc<Mutex<String>>,
    pub switches: Arc<AtomicUsize>,
    pub traffic_sockets: Arc<AtomicUsize>,
    pub log_sockets: Arc<AtomicUsize>,
    pub delay_active: Arc<AtomicUsize>,
    pub delay_max: Arc<AtomicUsize>,
    pub url: Arc<Mutex<String>>,
}

impl Default for MockState {
    fn default() -> Self {
        Self {
            selected: Arc::new(Mutex::new(NODE_A.into())),
            mode: Arc::new(Mutex::new("rule".into())),
            behavior: Arc::new(Mutex::new("normal".into())),
            switches: Arc::new(AtomicUsize::new(0)),
            traffic_sockets: Arc::new(AtomicUsize::new(0)),
            log_sockets: Arc::new(AtomicUsize::new(0)),
            delay_active: Arc::new(AtomicUsize::new(0)),
            delay_max: Arc::new(AtomicUsize::new(0)),
            url: Arc::new(Mutex::new(String::new())),
        }
    }
}

fn allowed(headers: &HeaderMap) -> bool {
    headers.get("authorization").and_then(|h| h.to_str().ok()) == Some(&format!("Bearer {SECRET}"))
}

async fn version(State(state): State<MockState>, headers: HeaderMap) -> Response {
    if !allowed(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match state.behavior.lock().await.as_str() {
        "offline" => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("{SECRET} {}", state.url.lock().await),
            )
                .into_response();
        }
        "timeout" => tokio::time::sleep(Duration::from_secs(2)).await,
        "invalid" => return Json(json!({"unexpected": true, "secret": SECRET})).into_response(),
        "redirect" => {
            return (
                StatusCode::FOUND,
                [("location", "http://169.254.169.254/latest/meta-data/")],
            )
                .into_response();
        }
        _ => {}
    }
    Json(json!({"version": "v1.19.20", "meta": true})).into_response()
}

async fn proxies(State(state): State<MockState>, headers: HeaderMap) -> Response {
    if !allowed(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if *state.behavior.lock().await == "invalid-proxies" {
        return Json(json!({"proxies": "broken"})).into_response();
    }
    let selected = state.selected.lock().await.clone();
    Json(json!({"proxies": {
        "PROXY": {"name": "PROXY", "type": "Selector", "all": [NODE_A, NODE_B, "新加坡 · SG 03", "DIRECT"], "now": selected, "history": [], "udp": true, "secret": SECRET},
        "自动选择": {"type": "URLTest", "all": [NODE_A, NODE_B], "now": NODE_A},
        "故障转移": {"type": "Fallback", "all": [NODE_A, NODE_B], "now": NODE_B},
        "负载均衡": {"type": "LoadBalance", "all": [NODE_A, NODE_B]},
        "中继链": {"type": "Relay", "all": [NODE_A, NODE_B]},
        NODE_A: {"name": NODE_A, "type": "Shadowsocks", "udp": true, "alive": true, "history": [{"time": "2026-11-21T10:00:00Z", "delay": 36}], "server": "private-server", "password": SECRET},
        NODE_B: {"name": NODE_B, "type": "Vless", "udp": true, "alive": true, "history": [{"time": "2026-11-21T10:00:00Z", "delay": 68}]},
        "新加坡 · SG 03": {"type": "Trojan", "udp": true, "history": [{"time": "2026-11-21T10:00:00Z", "delay": 82}]},
        "DIRECT": {"type": "Direct", "history": []}, "REJECT": {"type": "Reject", "history": []}
    }, "secret": SECRET, "controller": state.url.lock().await.clone()})).into_response()
}

async fn proxy(
    State(state): State<MockState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !allowed(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if name == "PROXY" {
        return Json(json!({"type": "Selector", "all": [NODE_A, NODE_B, "DIRECT"], "now": state.selected.lock().await.clone()})).into_response();
    }
    if name == "自动选择" {
        return Json(json!({"type": "URLTest", "all": [NODE_A, NODE_B], "now": NODE_A}))
            .into_response();
    }
    if [NODE_A, NODE_B, "DIRECT", "新加坡 · SG 03"].contains(&name.as_str()) {
        return Json(json!({"name": name, "type": "Shadowsocks", "history": []})).into_response();
    }
    StatusCode::NOT_FOUND.into_response()
}
async fn select(
    State(state): State<MockState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(input): Json<Value>,
) -> Response {
    if !allowed(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if name != "PROXY" {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(node) = input.get("name").and_then(Value::as_str) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if ![NODE_A, NODE_B, "DIRECT"].contains(&node) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    *state.selected.lock().await = node.to_string();
    state.switches.fetch_add(1, Ordering::SeqCst);
    StatusCode::NO_CONTENT.into_response()
}
async fn delay(
    State(state): State<MockState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !allowed(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let active = state.delay_active.fetch_add(1, Ordering::SeqCst) + 1;
    state.delay_max.fetch_max(active, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(30)).await;
    state.delay_active.fetch_sub(1, Ordering::SeqCst);
    if name == "DIRECT" {
        return StatusCode::GATEWAY_TIMEOUT.into_response();
    }
    Json(json!({"delay": if name == NODE_A { 35 } else { 67 }})).into_response()
}
async fn config(State(state): State<MockState>, headers: HeaderMap) -> Response {
    if !allowed(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(json!({"mode": state.mode.lock().await.clone(), "external-controller": state.url.lock().await.clone(), "secret": SECRET, "mixed-port": 7890})).into_response()
}
async fn set_config(
    State(state): State<MockState>,
    headers: HeaderMap,
    Json(input): Json<Value>,
) -> Response {
    if !allowed(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(mode) = input.get("mode").and_then(Value::as_str) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    *state.mode.lock().await = mode.to_string();
    StatusCode::NO_CONTENT.into_response()
}
async fn connections(State(state): State<MockState>, headers: HeaderMap) -> Response {
    if !allowed(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(json!({"downloadTotal": 2803589120u64, "uploadTotal": 163577856u64, "connections": [
        {"id": "abc-123", "metadata": {"network": "tcp", "sourceIP": "192.168.1.20", "sourcePort": "54218", "host": "github.com", "destinationIP": "140.82.112.4", "destinationPort": "443", "process": "browser"}, "chains": [state.selected.lock().await.clone(), "PROXY"], "upload": 15600, "download": 1234000, "start": "2026-11-21T10:00:00Z", "rule": "DomainSuffix"},
        {"id": "def-456", "metadata": {"network": "udp", "sourceIP": "192.168.1.20", "sourcePort": 52525, "host": "dns.google", "destinationPort": 443}, "chains": ["DIRECT"], "upload": 2400, "download": 3200, "start": "2026-11-21T10:05:00Z", "rule": "Match"}
    ]})).into_response()
}
async fn close_connection(headers: HeaderMap) -> StatusCode {
    if allowed(&headers) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::UNAUTHORIZED
    }
}

async fn traffic(
    State(state): State<MockState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !allowed(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if *state.behavior.lock().await == "disconnect" {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    state.traffic_sockets.fetch_add(1, Ordering::SeqCst);
    upgrade.on_upgrade(move |mut socket| async move {
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        let mut sample = 0u64;
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if *state.behavior.lock().await == "disconnect" { let _ = socket.send(Message::Close(None)).await; break; }
                    sample += 1;
                    let down = 1_300_000 + (sample % 14) * 180_000;
                    if socket.send(Message::Text(json!({"up": 148000 + (sample % 5) * 8000, "down": down, "upTotal": 163577856u64 + sample * 148000, "downTotal": 2803589120u64 + sample * down}).to_string().into())).await.is_err() { break; }
                },
                message = socket.recv() => { if matches!(message, Some(Ok(Message::Close(_))) | None | Some(Err(_))) { break; } }
            }
        }
    })
}
async fn logs(
    State(state): State<MockState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !allowed(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    state.log_sockets.fetch_add(1, Ordering::SeqCst);
    upgrade.on_upgrade(move |mut socket| async move {
        let url = state.url.lock().await.clone();
        let events = [
            json!({"type": "info", "payload": format!("[TCP] github.com:443 match DomainSuffix using PROXY[{NODE_A}]")}),
            json!({"type": "warning", "payload": format!("test redaction {SECRET} {url}")}),
        ];
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        let mut index = 0;
        loop {
            tokio::select! {
                _ = interval.tick() => { if socket.send(Message::Text(events[index % events.len()].to_string().into())).await.is_err() { break; } index += 1; },
                message = socket.recv() => { if matches!(message, Some(Ok(Message::Close(_))) | None | Some(Err(_))) { break; } }
            }
        }
    })
}

pub fn mock_router(state: MockState) -> Router {
    Router::new()
        .route("/version", get(version))
        .route("/proxies", get(proxies))
        .route("/proxies/{name}", get(proxy).put(select))
        .route("/proxies/{name}/delay", get(delay))
        .route("/configs", get(config).patch(set_config))
        .route("/connections", get(connections).delete(close_connection))
        .route("/connections/{id}", axum::routing::delete(close_connection))
        .route("/traffic", get(traffic))
        .route("/logs", get(logs))
        .with_state(state)
}

pub async fn start_mock() -> (String, MockState, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let state = MockState::default();
    *state.url.lock().await = url.clone();
    let app = mock_router(state.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, state, task)
}
