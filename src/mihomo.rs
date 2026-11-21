use crate::{
    config::{Config, validate_controller},
    error::{AppError, Result},
};
use axum::http::StatusCode;
use futures_util::StreamExt;
use reqwest::{
    Client, Method,
    header::{AUTHORIZATION, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use url::Url;

pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_STREAM_BYTES: usize = 64 * 1024;

#[derive(Clone)]
pub struct BackendConfig {
    pub controller_url: String,
    pub secret: String,
}

pub struct MihomoClient {
    pub(crate) base: Url,
    pub(crate) secret: String,
    http: Client,
    timeout: Duration,
    test_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Version {
    pub version: String,
    #[serde(default)]
    pub meta: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DelayHistory {
    #[serde(default)]
    pub time: String,
    pub delay: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Proxy {
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub all: Vec<String>,
    #[serde(default)]
    pub now: Option<String>,
    #[serde(default)]
    pub history: Vec<DelayHistory>,
    #[serde(default)]
    pub udp: bool,
    #[serde(default)]
    pub alive: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Proxies {
    pub proxies: BTreeMap<String, Proxy>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Traffic {
    pub up: u64,
    pub down: u64,
    #[serde(default, rename = "upTotal", skip_serializing_if = "Option::is_none")]
    pub up_total: Option<u64>,
    #[serde(default, rename = "downTotal", skip_serializing_if = "Option::is_none")]
    pub down_total: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogEntry {
    #[serde(rename = "type")]
    pub level: String,
    pub payload: String,
    #[serde(default)]
    pub timestamp: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Connection {
    pub id: String,
    pub network: String,
    pub source: String,
    pub destination: String,
    pub host: String,
    pub chains: Vec<String>,
    pub upload: u64,
    pub download: u64,
    pub start: String,
    pub rule: String,
    pub process: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Connections {
    #[serde(rename = "downloadTotal")]
    pub download_total: u64,
    #[serde(rename = "uploadTotal")]
    pub upload_total: u64,
    pub connections: Vec<Connection>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Rule,
    Global,
    Direct,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModeConfig {
    pub mode: Mode,
}

impl MihomoClient {
    pub fn new(config: &Config, backend: BackendConfig) -> Result<Self> {
        let base = validate_controller(&backend.controller_url, &config.trusted_backend_networks)?;
        if backend.secret.len() > 1024 || backend.secret.chars().any(char::is_control) {
            return Err(AppError::bad("Secret 格式无效"));
        }
        let timeout = Duration::from_secs(config.request_timeout_seconds);
        let http = Client::builder()
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .pool_idle_timeout(Duration::from_secs(60))
            .user_agent("mihomo-web-panel")
            .build()
            .map_err(|_| AppError::internal())?;
        Ok(Self {
            base,
            secret: backend.secret,
            http,
            timeout,
            test_url: config.delay_test_url.clone(),
        })
    }
    fn url(&self, segments: &[&str]) -> Result<Url> {
        let mut url = self.base.clone();
        {
            let mut path = url.path_segments_mut().map_err(|_| AppError::internal())?;
            path.clear();
            for segment in segments {
                validate_name(segment)?;
                path.push(segment);
            }
        }
        Ok(url)
    }
    fn authorization(&self) -> Result<HeaderValue> {
        let mut header = HeaderValue::from_str(&format!("Bearer {}", self.secret))
            .map_err(|_| AppError::bad("Secret 格式无效"))?;
        header.set_sensitive(true);
        Ok(header)
    }
    async fn request(
        &self,
        method: Method,
        segments: &[&str],
        body: Option<Value>,
        query: &[(&str, String)],
        timeout: Option<Duration>,
    ) -> Result<Value> {
        let url = self.url(segments)?;
        let mut request = self.http.request(method, url).query(query);
        if !self.secret.is_empty() {
            request = request.header(AUTHORIZATION, self.authorization()?);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        if let Some(timeout) = timeout {
            request = request.timeout(timeout);
        }
        let response = request.send().await.map_err(request_error)?;
        match response.status() {
            status if status.is_success() => {}
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                return Err(AppError::new(
                    StatusCode::BAD_GATEWAY,
                    "backend_auth_failed",
                    "Mihomo 拒绝认证，请重新设置 API Secret",
                ));
            }
            StatusCode::GATEWAY_TIMEOUT | StatusCode::REQUEST_TIMEOUT => {
                return Err(AppError::timeout());
            }
            StatusCode::NOT_FOUND => {
                return Err(AppError::new(
                    StatusCode::NOT_FOUND,
                    "backend_not_found",
                    "Mihomo 中没有找到该项目，请刷新后重试",
                ));
            }
            StatusCode::BAD_REQUEST => {
                return Err(AppError::bad("Mihomo 拒绝了此操作，请刷新状态后重试"));
            }
            _ => return Err(AppError::unavailable()),
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(AppError::invalid_response());
        }
        if response.status() == StatusCode::NO_CONTENT {
            return Ok(Value::Null);
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(request_error)?;
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(AppError::invalid_response());
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&bytes).map_err(|_| AppError::invalid_response())
    }
    pub async fn version(&self) -> Result<Version> {
        let mut version: Version = serde_json::from_value(
            self.request(Method::GET, &["version"], None, &[], None)
                .await?,
        )
        .map_err(|_| AppError::invalid_response())?;
        if version.version.len() > 128 {
            return Err(AppError::invalid_response());
        }
        version.version = self.redact(&version.version);
        Ok(version)
    }
    pub async fn proxies(&self) -> Result<Proxies> {
        let value = self
            .request(Method::GET, &["proxies"], None, &[], None)
            .await?;
        let mut proxies: Proxies =
            serde_json::from_value(value).map_err(|_| AppError::invalid_response())?;
        if proxies.proxies.len() > 20000 {
            return Err(AppError::invalid_response());
        }
        for (name, proxy) in &mut proxies.proxies {
            validate_name(name).map_err(|_| AppError::invalid_response())?;
            proxy.name = name.clone();
            if proxy.all.len() > 20000 || proxy.kind.len() > 64 {
                return Err(AppError::invalid_response());
            }
            proxy.history = proxy
                .history
                .iter()
                .rev()
                .take(64)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
        }
        self.sanitize(proxies)
    }
    pub async fn proxy(&self, name: &str) -> Result<Proxy> {
        let value = self
            .request(Method::GET, &["proxies", name], None, &[], None)
            .await?;
        let mut proxy: Proxy =
            serde_json::from_value(value).map_err(|_| AppError::invalid_response())?;
        proxy.name = name.to_string();
        proxy.history = proxy
            .history
            .into_iter()
            .rev()
            .take(64)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        self.sanitize(proxy)
    }
    pub async fn select(&self, group: &str, node: &str) -> Result<()> {
        validate_name(node)?;
        let proxy = self.proxy(group).await?;
        if proxy.kind != "Selector" || !proxy.all.iter().any(|name| name == node) {
            return Err(AppError::bad("仅 Selector 代理组允许选择其成员节点"));
        }
        self.request(
            Method::PUT,
            &["proxies", group],
            Some(json!({"name": node})),
            &[],
            None,
        )
        .await?;
        Ok(())
    }
    pub async fn delay(&self, name: &str, timeout_ms: u64) -> Result<u32> {
        if !(1000..=15000).contains(&timeout_ms) {
            return Err(AppError::bad("测速超时应为 1000–15000 毫秒"));
        }
        let value = self
            .request(
                Method::GET,
                &["proxies", name, "delay"],
                None,
                &[
                    ("url", self.test_url.clone()),
                    ("timeout", timeout_ms.to_string()),
                ],
                Some(Duration::from_millis(timeout_ms + 1000)),
            )
            .await?;
        let delay = value
            .get("delay")
            .and_then(Value::as_u64)
            .filter(|delay| *delay > 0 && *delay <= u32::MAX as u64)
            .ok_or_else(AppError::invalid_response)?;
        Ok(delay as u32)
    }
    pub async fn mode(&self) -> Result<ModeConfig> {
        let value = self
            .request(Method::GET, &["configs"], None, &[], None)
            .await?;
        serde_json::from_value(json!({"mode": value.get("mode")}))
            .map_err(|_| AppError::invalid_response())
    }
    pub async fn set_mode(&self, mode: Mode) -> Result<()> {
        self.request(
            Method::PATCH,
            &["configs"],
            Some(json!({"mode": mode})),
            &[],
            None,
        )
        .await?;
        Ok(())
    }
    pub async fn connections(&self) -> Result<Connections> {
        let value = self
            .request(Method::GET, &["connections"], None, &[], None)
            .await?;
        self.sanitize(parse_connections(value)?)
    }
    pub async fn close_connection(&self, id: &str) -> Result<()> {
        if id.len() > 128
            || id.is_empty()
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        {
            return Err(AppError::bad("连接 ID 无效"));
        }
        self.request(Method::DELETE, &["connections", id], None, &[], None)
            .await?;
        Ok(())
    }
    pub async fn close_all(&self) -> Result<()> {
        self.request(Method::DELETE, &["connections"], None, &[], None)
            .await?;
        Ok(())
    }
    pub(crate) fn websocket_request(
        &self,
        logs: bool,
    ) -> Result<tokio_tungstenite::tungstenite::http::Request<()>> {
        let mut url = self.url(&[if logs { "logs" } else { "traffic" }])?;
        url.set_scheme(if self.base.scheme() == "https" {
            "wss"
        } else {
            "ws"
        })
        .map_err(|_| AppError::internal())?;
        if logs {
            url.query_pairs_mut().append_pair("level", "debug");
        }
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|_| AppError::internal())?;
        if !self.secret.is_empty() {
            request
                .headers_mut()
                .insert(AUTHORIZATION, self.authorization()?);
        }
        Ok(request)
    }
    pub(crate) fn timeout(&self) -> Duration {
        self.timeout
    }
    pub fn redact(&self, input: &str) -> String {
        let mut output = input.to_string();
        for sensitive in [
            self.secret.as_str(),
            self.base.as_str(),
            self.base.host_str().unwrap_or(""),
        ] {
            if !sensitive.is_empty() {
                output = output.replace(sensitive, "[redacted]");
            }
        }
        output
    }
    fn sanitize<T: Serialize + for<'de> Deserialize<'de>>(&self, input: T) -> Result<T> {
        let mut value = serde_json::to_value(input).map_err(|_| AppError::internal())?;
        self.redact_value(&mut value);
        serde_json::from_value(value).map_err(|_| AppError::invalid_response())
    }
    fn redact_value(&self, value: &mut Value) {
        match value {
            Value::String(text) => *text = self.redact(text),
            Value::Array(values) => values.iter_mut().for_each(|value| self.redact_value(value)),
            Value::Object(object) => {
                let old = std::mem::take(object);
                for (key, mut value) in old {
                    self.redact_value(&mut value);
                    object.insert(self.redact(&key), value);
                }
            }
            _ => {}
        }
    }
}

pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 512
        || name == "."
        || name == ".."
        || name.chars().any(char::is_control)
    {
        return Err(AppError::bad("节点或代理组名称无效"));
    }
    Ok(())
}

fn request_error(error: reqwest::Error) -> AppError {
    if error.is_timeout() {
        AppError::timeout()
    } else {
        AppError::unavailable()
    }
}

pub fn parse_connections(value: Value) -> Result<Connections> {
    let download_total = value
        .get("downloadTotal")
        .and_then(Value::as_u64)
        .ok_or_else(AppError::invalid_response)?;
    let upload_total = value
        .get("uploadTotal")
        .and_then(Value::as_u64)
        .ok_or_else(AppError::invalid_response)?;
    let entries = match value.get("connections") {
        Some(Value::Null) => vec![],
        Some(Value::Array(entries)) if entries.len() <= 10000 => entries.clone(),
        _ => return Err(AppError::invalid_response()),
    };
    let mut connections = Vec::with_capacity(entries.len());
    for entry in entries {
        let metadata = entry
            .get("metadata")
            .and_then(Value::as_object)
            .ok_or_else(AppError::invalid_response)?;
        let string = |key: &str| {
            entry
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or("")
                .chars()
                .take(512)
                .collect::<String>()
        };
        let meta = |key: &str| {
            metadata
                .get(key)
                .map(|value| match value {
                    Value::String(text) => text.chars().take(512).collect(),
                    Value::Number(n) => n.to_string(),
                    _ => String::new(),
                })
                .unwrap_or_default()
        };
        let id = string("id");
        if id.is_empty() {
            return Err(AppError::invalid_response());
        }
        let host = meta("host");
        let destination_ip = meta("destinationIP");
        connections.push(Connection {
            id,
            network: meta("network"),
            source: format!("{}:{}", meta("sourceIP"), meta("sourcePort")),
            destination: format!(
                "{}:{}",
                if host.is_empty() {
                    &destination_ip
                } else {
                    &host
                },
                meta("destinationPort")
            ),
            host,
            chains: entry
                .get("chains")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .take(32)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            upload: entry.get("upload").and_then(Value::as_u64).unwrap_or(0),
            download: entry.get("download").and_then(Value::as_u64).unwrap_or(0),
            start: string("start"),
            rule: string("rule"),
            process: meta("process"),
        });
    }
    Ok(Connections {
        download_total,
        upload_total,
        connections,
    })
}
