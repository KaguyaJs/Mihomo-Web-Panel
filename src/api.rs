use crate::{
    AppState, assets,
    auth::{self, AuthUser, Identity},
    config::WebSettings,
    error::{AppError, Result},
    helper::{self, HelperRequest, Operation},
    mihomo::{BackendConfig, MihomoClient, ModeConfig},
    security, streams,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Extension, FromRequest, Path, Request, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode, header::SET_COOKIE},
    middleware,
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
};
use futures_util::{StreamExt, stream};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{net::IpAddr, sync::Arc};

pub struct SafeJson<T>(pub T);
impl<S, T> FromRequest<S> for SafeJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = AppError;
    async fn from_request(
        request: Request,
        state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(|error| {
                if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
                    AppError::new(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "payload_too_large",
                        "提交内容过大",
                    )
                } else {
                    AppError::bad("请求格式无效，请检查表单内容")
                }
            })
    }
}

pub fn router(state: AppState) -> Router {
    let authenticated = Router::new()
        .route("/api/auth/me", get(me))
        .route("/api/auth/logout", post(logout))
        .route("/api/status", get(backend_status))
        .route("/api/proxies", get(proxies))
        .route("/api/proxies/{name}", get(proxy).put(select_proxy))
        .route("/api/proxies/{name}/delay", post(delay))
        .route("/api/delay/batch", post(batch_delay))
        .route("/api/config", get(mode).patch(set_mode))
        .route("/api/connections", get(connections).delete(close_all))
        .route("/api/connections/{id}", delete(close_connection))
        .route("/api/traffic", get(traffic_ws))
        .route("/api/logs", get(logs_ws))
        .route("/api/settings", get(settings).put(save_settings))
        .route(
            "/api/settings/backend",
            get(backend_status).put(save_backend),
        )
        .route("/api/settings/backend/test", post(test_backend))
        .route("/api/settings/security", patch(update_account))
        .route(
            "/api/settings/sessions",
            get(sessions).delete(revoke_sessions),
        )
        .route(
            "/api/settings/autostart",
            get(autostart).post(change_autostart),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            security::protected,
        ));
    Router::new()
        .route(
            "/api/health",
            get(|| async { Json(json!({"status": "ok"})) }),
        )
        .route("/api/setup/status", get(setup_status))
        .route("/api/setup/test", post(setup_test))
        .route("/api/setup/complete", post(setup_complete))
        .route("/api/auth/login", post(login))
        .merge(authenticated)
        .fallback(assets::serve)
        .layer(DefaultBodyLimit::max(32768))
        .layer(tower_http::limit::RequestBodyLimitLayer::new(32768))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security::perimeter,
        ))
        .with_state(state)
}

async fn setup_status(State(state): State<AppState>) -> Result<Json<Value>> {
    Ok(Json(
        json!({"required": !auth::initialized(&state.pool).await?}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BackendInput {
    controller_url: String,
    secret: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetupTestInput {
    token: String,
    controller_url: String,
    secret: String,
}

async fn setup_test(
    State(state): State<AppState>,
    SafeJson(input): SafeJson<SetupTestInput>,
) -> Result<Json<Value>> {
    auth::check_bootstrap(&state.pool, &input.token).await?;
    let client = MihomoClient::new(
        &state.config,
        BackendConfig {
            controller_url: input.controller_url,
            secret: input.secret,
        },
    )?;
    let version = client.version().await?;
    Ok(Json(json!({"connected": true, "version": version.version})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetupInput {
    token: String,
    username: String,
    password: String,
    confirm_password: String,
    controller_url: String,
    secret: String,
}

async fn setup_complete(
    State(state): State<AppState>,
    SafeJson(input): SafeJson<SetupInput>,
) -> Result<(StatusCode, Json<Value>)> {
    let _guard = state.bootstrap_lock.try_lock().map_err(|_| {
        AppError::new(
            StatusCode::CONFLICT,
            "setup_busy",
            "初始化正在进行，请稍后重试",
        )
    })?;
    auth::check_bootstrap(&state.pool, &input.token).await?;
    auth::validate_username(&input.username)?;
    auth::validate_password(&input.password, &input.confirm_password)?;
    let client = Arc::new(MihomoClient::new(
        &state.config,
        BackendConfig {
            controller_url: input.controller_url.clone(),
            secret: input.secret.clone(),
        },
    )?);
    client.version().await?;
    let permit = state
        .auth_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| AppError::limited())?;
    let password = input.password;
    let hash = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        auth::hash_password(&password)
    })
    .await
    .map_err(|_| AppError::internal())??;
    let mut transaction = state.pool.begin().await?;
    let consumed = sqlx::query("UPDATE bootstrap SET consumed_at = ? WHERE id = 1 AND token_hash = ? AND consumed_at IS NULL AND NOT EXISTS (SELECT 1 FROM administrator)")
        .bind(auth::now()).bind(auth::digest(&input.token)).execute(&mut *transaction).await?;
    if consumed.rows_affected() != 1 {
        return Err(AppError::forbidden());
    }
    sqlx::query(
        "INSERT INTO administrator(id, username, password_hash, updated_at) VALUES(1, ?, ?, ?)",
    )
    .bind(input.username)
    .bind(hash)
    .bind(auth::now())
    .execute(&mut *transaction)
    .await?;
    sqlx::query("INSERT INTO backend(id, controller_url, secret, updated_at) VALUES(1, ?, ?, ?)")
        .bind(input.controller_url)
        .bind(input.secret)
        .bind(auth::now())
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    *state.backend.write().await = Some(client);
    state
        .backend_changes
        .send_modify(|generation| *generation += 1);
    if std::fs::remove_file(state.config.data_dir.join("bootstrap-token")).is_err() {
        tracing::warn!(
            "consumed bootstrap token file could not be removed; the token is already invalidated"
        );
    }
    tracing::info!("administrator initialization completed");
    Ok((StatusCode::CREATED, Json(json!({"initialized": true}))))
}

fn user_agent(headers: &HeaderMap) -> &str {
    headers
        .get("user-agent")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
}

async fn login(
    State(state): State<AppState>,
    Extension(ip): Extension<IpAddr>,
    headers: HeaderMap,
    SafeJson(input): SafeJson<auth::LoginInput>,
) -> Result<Response> {
    let token = auth::login(&state, input, ip, user_agent(&headers)).await?;
    let user = AuthUser {
        username: auth::username(&state.pool).await?,
        csrf_token: auth::csrf_token(&token),
    };
    let mut response = Json(user).into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        auth::cookie(
            &token,
            state.config.secure_cookies,
            state.settings.read().await.session_absolute_seconds,
        ),
    );
    Ok(response)
}

async fn me(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
) -> Result<Json<AuthUser>> {
    Ok(Json(AuthUser {
        username: auth::username(&state.pool).await?,
        csrf_token: identity.csrf,
    }))
}
async fn logout(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
) -> Result<Response> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
        .bind(identity.session.token_hash)
        .execute(&state.pool)
        .await?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .insert(SET_COOKIE, auth::clear_cookie(state.config.secure_cookies));
    Ok(response)
}

async fn backend_status(State(state): State<AppState>) -> Json<Value> {
    let client = state.client().await;
    match client {
        Ok(client) => match client.version().await {
            Ok(version) => Json(
                json!({"configured": true, "connected": true, "version": version.version, "meta": version.meta}),
            ),
            Err(error) => Json(
                json!({"configured": true, "connected": false, "version": null, "error": error.code, "message": error.message}),
            ),
        },
        Err(_) => Json(
            json!({"configured": false, "connected": false, "version": null, "message": "尚未配置 Mihomo"}),
        ),
    }
}
async fn test_backend(
    State(state): State<AppState>,
    SafeJson(input): SafeJson<BackendInput>,
) -> Result<Json<Value>> {
    let client = MihomoClient::new(
        &state.config,
        BackendConfig {
            controller_url: input.controller_url,
            secret: input.secret,
        },
    )?;
    let version = client.version().await?;
    Ok(Json(json!({"connected": true, "version": version.version})))
}
async fn save_backend(
    State(state): State<AppState>,
    SafeJson(input): SafeJson<BackendInput>,
) -> Result<Json<Value>> {
    let client = Arc::new(MihomoClient::new(
        &state.config,
        BackendConfig {
            controller_url: input.controller_url.clone(),
            secret: input.secret.clone(),
        },
    )?);
    let version = client.version().await?;
    let mut transaction = state.pool.begin().await?;
    sqlx::query("INSERT INTO backend(id, controller_url, secret, updated_at) VALUES(1, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET controller_url=excluded.controller_url, secret=excluded.secret, updated_at=excluded.updated_at")
        .bind(input.controller_url).bind(input.secret).bind(auth::now()).execute(&mut *transaction).await?;
    // Serialize commit and in-memory replacement so concurrent saves cannot leave divergent state.
    let mut backend = state.backend.write().await;
    transaction.commit().await?;
    *backend = Some(client);
    state
        .backend_changes
        .send_modify(|generation| *generation += 1);
    Ok(Json(json!({"connected": true, "version": version.version})))
}

async fn proxies(State(state): State<AppState>) -> Result<Json<crate::mihomo::Proxies>> {
    Ok(Json(state.client().await?.proxies().await?))
}
async fn proxy(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<crate::mihomo::Proxy>> {
    Ok(Json(state.client().await?.proxy(&name).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectInput {
    name: String,
}
async fn select_proxy(
    State(state): State<AppState>,
    Path(name): Path<String>,
    SafeJson(input): SafeJson<SelectInput>,
) -> Result<Json<crate::mihomo::Proxy>> {
    let _guard = state.select_lock.try_lock().map_err(|_| {
        AppError::new(
            StatusCode::CONFLICT,
            "selection_busy",
            "节点切换正在进行，请稍后重试",
        )
    })?;
    let client = state.client().await?;
    client.select(&name, &input.name).await?;
    Ok(Json(client.proxy(&name).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DelayInput {
    timeout_ms: u64,
}
async fn delay(
    State(state): State<AppState>,
    Path(name): Path<String>,
    SafeJson(input): SafeJson<DelayInput>,
) -> Result<Json<Value>> {
    let permit = state
        .delay_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| AppError::limited())?;
    let _permit = permit;
    let value = state.client().await?.delay(&name, input.timeout_ms).await?;
    Ok(Json(json!({"name": name, "delay": value, "status": "ok"})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchInput {
    names: Vec<String>,
    timeout_ms: u64,
}
async fn batch_delay(
    State(state): State<AppState>,
    SafeJson(input): SafeJson<BatchInput>,
) -> Result<Json<Value>> {
    if input.names.is_empty()
        || input.names.len() > 100
        || !(1000..=15000).contains(&input.timeout_ms)
    {
        return Err(AppError::bad(
            "每次批量测速应为 1–100 个节点，超时为 1000–15000 毫秒",
        ));
    }
    let client = state.client().await?;
    let known = client.proxies().await?;
    let mut names = input.names;
    names.sort();
    names.dedup();
    if names.iter().any(|name| !known.proxies.contains_key(name)) {
        return Err(AppError::bad("测速列表包含不存在的节点"));
    }
    let results = stream::iter(names).map(|name| {
        let client = client.clone(); let slots = state.delay_slots.clone();
        async move {
            let Ok(permit) = tokio::time::timeout(std::time::Duration::from_secs(20), slots.acquire_owned()).await else { return json!({"name": name, "status": "busy", "delay": null}); };
            let Ok(_permit) = permit else { return json!({"name": name, "status": "busy", "delay": null}); };
            match client.delay(&name, input.timeout_ms).await {
                Ok(delay) => json!({"name": name, "status": "ok", "delay": delay}),
                Err(error) => json!({"name": name, "status": if error.code == "backend_timeout" { "timeout" } else { "unavailable" }, "delay": null}),
            }
        }
    }).buffer_unordered(4).collect::<Vec<_>>().await;
    Ok(Json(json!({"results": results})))
}
async fn mode(State(state): State<AppState>) -> Result<Json<ModeConfig>> {
    Ok(Json(state.client().await?.mode().await?))
}
async fn set_mode(
    State(state): State<AppState>,
    SafeJson(input): SafeJson<ModeConfig>,
) -> Result<Json<Value>> {
    state.client().await?.set_mode(input.mode).await?;
    Ok(Json(json!({"saved": true})))
}
async fn connections(State(state): State<AppState>) -> Result<Json<crate::mihomo::Connections>> {
    Ok(Json(state.client().await?.connections().await?))
}
async fn close_connection(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    state.client().await?.close_connection(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CloseAllInput {
    confirm: bool,
}
async fn close_all(
    State(state): State<AppState>,
    SafeJson(input): SafeJson<CloseAllInput>,
) -> Result<StatusCode> {
    if !input.confirm {
        return Err(AppError::bad("关闭全部连接需要再次确认"));
    }
    state.client().await?.close_all().await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn traffic_ws(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    ws: WebSocketUpgrade,
) -> Response {
    if state.browser_stream_slots.available_permits() == 0 {
        return AppError::limited().into_response();
    }
    ws.max_message_size(1024)
        .max_frame_size(1024)
        .on_upgrade(move |socket| streams::browser_stream(socket, state, identity, false))
}
async fn logs_ws(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    ws: WebSocketUpgrade,
) -> Response {
    if state.browser_stream_slots.available_permits() == 0 {
        return AppError::limited().into_response();
    }
    ws.max_message_size(1024)
        .max_frame_size(1024)
        .on_upgrade(move |socket| streams::browser_stream(socket, state, identity, true))
}

async fn settings(State(state): State<AppState>) -> Json<Value> {
    let requested = state.settings.read().await.clone();
    Json(
        json!({"version": env!("CARGO_PKG_VERSION"), "uptime_seconds": state.started.elapsed().as_secs(),
        "platform": std::env::consts::OS, "architecture": std::env::consts::ARCH,
        "active_listen": state.active_settings.listen, "active_origin": state.active_settings.public_origin,
        "secure_cookies": state.config.secure_cookies, "settings": requested,
        "restart_required": requested.listen != state.active_settings.listen || requested.public_origin != state.active_settings.public_origin}),
    )
}
async fn save_settings(
    State(state): State<AppState>,
    SafeJson(input): SafeJson<WebSettings>,
) -> Result<Json<Value>> {
    state.config.validate_web(&input).map_err(|_| {
        AppError::bad(
            "服务设置无效：监听端口需为 1024–65535，公网需 HTTPS，超时与刷新频率需在允许范围内",
        )
    })?;
    let encoded = serde_json::to_string(&input).map_err(|_| AppError::internal())?;
    let mut settings = state.settings.write().await;
    sqlx::query("INSERT INTO web_settings(id, settings_json) VALUES(1, ?) ON CONFLICT(id) DO UPDATE SET settings_json=excluded.settings_json")
        .bind(encoded).execute(&state.pool).await?;
    *settings = input;
    let restart = settings.listen != state.active_settings.listen
        || settings.public_origin != state.active_settings.public_origin;
    Ok(Json(json!({"saved": true, "restart_required": restart})))
}
async fn update_account(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(ip): Extension<IpAddr>,
    SafeJson(input): SafeJson<auth::AccountInput>,
) -> Result<Response> {
    let token = auth::update_account(&state, input, &identity, ip).await?;
    let mut response = Json(AuthUser {
        username: auth::username(&state.pool).await?,
        csrf_token: auth::csrf_token(&token),
    })
    .into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        auth::cookie(
            &token,
            state.config.secure_cookies,
            state.settings.read().await.session_absolute_seconds,
        ),
    );
    Ok(response)
}
async fn sessions(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
) -> Result<Json<Value>> {
    let idle = state.settings.read().await.session_idle_seconds;
    let sessions = sqlx::query_as::<_, auth::Session>("SELECT * FROM sessions WHERE absolute_expires_at > ? AND last_seen + ? > ? ORDER BY last_seen DESC")
        .bind(auth::now()).bind(idle).bind(auth::now()).fetch_all(&state.pool).await?;
    Ok(Json(
        json!({"sessions": sessions.into_iter().map(|session| json!({"id": session.token_hash,
        "current": session.token_hash == identity.session.token_hash, "created_at": session.created_at,
        "last_seen": session.last_seen, "expires_at": session.absolute_expires_at,
        "ip": session.ip, "user_agent": session.user_agent})).collect::<Vec<_>>()}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevokeInput {
    scope: String,
}
async fn revoke_sessions(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    SafeJson(input): SafeJson<RevokeInput>,
) -> Result<Response> {
    match input.scope.as_str() {
        "others" => {
            sqlx::query("DELETE FROM sessions WHERE token_hash != ?")
                .bind(identity.session.token_hash)
                .execute(&state.pool)
                .await?;
        }
        "all" => {
            sqlx::query("DELETE FROM sessions")
                .execute(&state.pool)
                .await?;
        }
        _ => return Err(AppError::bad("会话撤销范围只能为 others 或 all")),
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    if input.scope == "all" {
        response
            .headers_mut()
            .insert(SET_COOKIE, auth::clear_cookie(state.config.secure_cookies));
    }
    Ok(response)
}
async fn autostart(State(state): State<AppState>) -> Result<Json<helper::HelperStatus>> {
    Ok(Json(helper::request(&state, Operation::Status).await?))
}
async fn change_autostart(
    State(state): State<AppState>,
    SafeJson(input): SafeJson<HelperRequest>,
) -> Result<Json<helper::HelperStatus>> {
    if input.operation == Operation::Status {
        return Err(AppError::bad("请使用状态查询接口"));
    }
    Ok(Json(helper::request(&state, input.operation).await?))
}
