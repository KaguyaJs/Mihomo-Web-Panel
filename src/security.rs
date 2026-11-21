use crate::{
    AppState,
    auth::{self, Identity},
    error::{AppError, Result},
};
use axum::{
    body::Body,
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Mutex,
    time::{Duration, Instant},
};
use url::Url;

#[derive(Default)]
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, Bucket>>,
}
struct Bucket {
    started: Instant,
    attempts: u32,
    failures: u32,
    locked_until: Option<Instant>,
}

impl RateLimiter {
    fn attempt(&self, key: String, maximum: u32, window: Duration) -> Result<()> {
        let now = Instant::now();
        let mut buckets = self.buckets.lock().map_err(|_| AppError::internal())?;
        if buckets.len() >= 8192 {
            buckets.retain(|_, bucket| {
                now.duration_since(bucket.started) < Duration::from_secs(900)
                    || bucket.locked_until.is_some_and(|until| until > now)
            });
            if buckets.len() >= 8192 && !buckets.contains_key(&key) {
                return Err(AppError::limited());
            }
        }
        let bucket = buckets.entry(key).or_insert(Bucket {
            started: now,
            attempts: 0,
            failures: 0,
            locked_until: None,
        });
        if bucket.locked_until.is_some_and(|until| until > now) {
            return Err(AppError::limited());
        }
        if now.duration_since(bucket.started) >= window {
            *bucket = Bucket {
                started: now,
                attempts: 0,
                failures: 0,
                locked_until: None,
            };
        }
        if bucket.attempts >= maximum {
            return Err(AppError::limited());
        }
        bucket.attempts += 1;
        Ok(())
    }
    pub fn request(&self, ip: IpAddr, auth: bool, mutation: bool) -> Result<()> {
        self.attempt(
            format!(
                "{}:{ip}",
                if auth {
                    "auth-ip"
                } else if mutation {
                    "write-ip"
                } else {
                    "read-ip"
                }
            ),
            if auth {
                30
            } else if mutation {
                180
            } else {
                900
            },
            Duration::from_secs(60),
        )
    }
    pub fn login_attempt(&self, ip: IpAddr, username: &str) -> Result<()> {
        self.attempt(format!("login-ip:{ip}"), 20, Duration::from_secs(60))?;
        self.attempt(
            format!(
                "user:{}",
                auth::digest(&username.trim().to_ascii_lowercase())
            ),
            10,
            Duration::from_secs(900),
        )
    }
    pub fn login_failed(&self, ip: IpAddr, username: &str) -> Duration {
        let now = Instant::now();
        let mut maximum = 0;
        if let Ok(mut buckets) = self.buckets.lock() {
            for key in [
                format!("login-ip:{ip}"),
                format!(
                    "user:{}",
                    auth::digest(&username.trim().to_ascii_lowercase())
                ),
            ] {
                if let Some(bucket) = buckets.get_mut(&key) {
                    bucket.failures += 1;
                    maximum = maximum.max(bucket.failures);
                    if bucket.failures >= 8 {
                        bucket.locked_until = Some(now + Duration::from_secs(300));
                    }
                }
            }
        }
        Duration::from_millis((100u64 << maximum.min(4)).min(2000))
    }
    pub fn login_succeeded(&self, username: &str) {
        if let Ok(mut buckets) = self.buckets.lock() {
            buckets.remove(&format!(
                "user:{}",
                auth::digest(&username.trim().to_ascii_lowercase())
            ));
        }
    }
}

pub fn client_ip(request: &Request, state: &AppState) -> Result<IpAddr> {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip())
        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    if !state
        .config
        .trusted_proxies
        .iter()
        .any(|network| network.contains(&peer))
    {
        return Ok(peer);
    }
    if let Some(value) = request.headers().get("x-forwarded-for") {
        let text = value.to_str().map_err(|_| AppError::forbidden())?;
        if text.len() > 512 {
            return Err(AppError::forbidden());
        }
        let chain: Vec<IpAddr> = text
            .split(',')
            .map(|part| part.trim().parse::<IpAddr>())
            .collect::<std::result::Result<_, _>>()
            .map_err(|_| AppError::forbidden())?;
        if chain.is_empty() || chain.len() > 10 {
            return Err(AppError::forbidden());
        }
        // Work backwards through known proxies; ignore all client-controlled prefixes.
        for ip in chain.into_iter().rev() {
            if !state
                .config
                .trusted_proxies
                .iter()
                .any(|network| network.contains(&ip))
            {
                return Ok(ip);
            }
        }
    }
    Ok(peer)
}

pub fn validate_origin(
    headers: &HeaderMap,
    method: &Method,
    public_origin: &str,
    websocket: bool,
) -> Result<()> {
    let origin = Url::parse(public_origin).map_err(|_| AppError::internal())?;
    let expected_host = &origin[url::Position::BeforeHost..url::Position::AfterPort];
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(AppError::forbidden)?;
    if !host.eq_ignore_ascii_case(expected_host) || headers.get_all("host").iter().count() != 1 {
        return Err(AppError::forbidden());
    }
    let supplied = headers.get("origin").and_then(|h| h.to_str().ok());
    if supplied.is_some_and(|value| value != public_origin)
        || headers.get_all("origin").iter().count() > 1
    {
        return Err(AppError::forbidden());
    }
    if (websocket || !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS))
        && supplied != Some(public_origin)
    {
        return Err(AppError::forbidden());
    }
    if headers.get("sec-fetch-site").and_then(|h| h.to_str().ok()) == Some("cross-site") {
        return Err(AppError::forbidden());
    }
    Ok(())
}

pub async fn perimeter(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let websocket = request
        .headers()
        .get("upgrade")
        .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"websocket"));
    if let Err(error) = validate_origin(
        request.headers(),
        request.method(),
        &state.active_settings.public_origin,
        websocket,
    ) {
        return secure_headers(error.into_response(), state.config.secure_cookies);
    }
    let path = request.uri().path();
    let api = path.starts_with("/api/");
    if api {
        let ip = match client_ip(&request, &state) {
            Ok(ip) => ip,
            Err(error) => {
                return secure_headers(error.into_response(), state.config.secure_cookies);
            }
        };
        let auth_route = path.starts_with("/api/setup/") || path == "/api/auth/login";
        let mutation = !matches!(*request.method(), Method::GET | Method::HEAD);
        if let Err(error) = state.limiter.request(ip, auth_route, mutation) {
            return secure_headers(error.into_response(), state.config.secure_cookies);
        }
        request.extensions_mut().insert(ip);
    }
    let response = next.run(request).await;
    secure_headers(response, state.config.secure_cookies)
}

pub async fn protected(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let identity = match auth::authenticate(&state, request.headers()).await {
        Ok(identity) => identity,
        Err(error) => return error.into_response(),
    };
    if !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        let supplied = request
            .headers()
            .get("x-csrf-token")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        if !auth::constant_eq(supplied, &identity.csrf) {
            return AppError::forbidden().into_response();
        }
    }
    if request.uri().path() == "/api/auth/me"
        || !matches!(
            *request.method(),
            Method::GET | Method::HEAD | Method::OPTIONS
        )
    {
        let idle = state.settings.read().await.session_idle_seconds;
        if let Err(error) =
            auth::session_by_hash(&state.pool, &identity.session.token_hash, idle, true).await
        {
            return error.into_response();
        }
    }
    request.extensions_mut().insert::<Identity>(identity);
    next.run(request).await
}

fn secure_headers(mut response: Response<Body>, https: bool) -> Response<Body> {
    let headers = response.headers_mut();
    for (key, value) in [
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
        (
            "permissions-policy",
            "camera=(), microphone=(), geolocation=()",
        ),
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'; object-src 'none'",
        ),
        ("cross-origin-resource-policy", "same-origin"),
        ("cache-control", "no-store"),
    ] {
        headers.insert(
            axum::http::header::HeaderName::from_static(key),
            value.parse().unwrap(),
        );
    }
    if https {
        headers.insert(
            "strict-transport-security",
            "max-age=31536000".parse().unwrap(),
        );
    }
    if response.status() == StatusCode::INTERNAL_SERVER_ERROR {
        tracing::error!("request failed with an internal error");
    }
    response
}
