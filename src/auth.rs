use crate::{
    AppState,
    error::{AppError, Result},
};
use argon2::{
    Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version,
    password_hash::SaltString,
};
use axum::http::{HeaderMap, HeaderValue};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqlitePool};
use std::{
    net::IpAddr,
    time::{SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;

pub const COOKIE_NAME: &str = "mwp_session";

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}
pub fn digest(token: &str) -> String {
    let hash = Sha256::digest(token.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}
pub fn csrf_token(session: &str) -> String {
    digest(&format!("mihomo-web-panel/csrf/v1/{session}"))
}
pub fn constant_eq(left: &str, right: &str) -> bool {
    left.as_bytes().ct_eq(right.as_bytes()).into()
}
fn argon() -> Argon2<'static> {
    Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(19456, 2, 1, Some(32)).unwrap(),
    )
}
pub fn hash_password(password: &str) -> Result<String> {
    let mut salt_bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut salt_bytes);
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|_| AppError::internal())?;
    argon()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| AppError::internal())
}
pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash).ok().is_some_and(|parsed| {
        argon()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    })
}
pub fn validate_username(username: &str) -> Result<()> {
    if !(3..=32).contains(&username.len())
        || !username
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
    {
        return Err(AppError::bad(
            "用户名应为 3–32 位字母、数字、点、下划线或连字符",
        ));
    }
    Ok(())
}
pub fn validate_password(password: &str, confirmation: &str) -> Result<()> {
    if !(12..=128).contains(&password.chars().count()) || password.len() > 512 {
        return Err(AppError::bad("密码应为 12–128 个字符"));
    }
    if !constant_eq(password, confirmation) {
        return Err(AppError::bad("两次输入的密码不一致"));
    }
    Ok(())
}

#[derive(Clone, FromRow)]
pub struct Session {
    pub token_hash: String,
    pub created_at: i64,
    pub last_seen: i64,
    pub absolute_expires_at: i64,
    pub ip: String,
    pub user_agent: String,
}

#[derive(Clone)]
pub struct Identity {
    pub session: Session,
    pub csrf: String,
}

pub fn read_cookie(headers: &HeaderMap) -> Option<String> {
    let mut values = headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .flat_map(|header| header.to_str().unwrap_or("").split(';'));
    let mut found = None;
    for part in &mut values {
        if let Some((name, value)) = part.trim().split_once('=')
            && name == COOKIE_NAME
        {
            if found.is_some()
                || value.len() != 43
                || !value
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
            {
                return None;
            }
            found = Some(value.to_owned());
        }
    }
    found
}

pub fn cookie(token: &str, secure: bool, absolute_seconds: i64) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={absolute_seconds}{}",
        if secure { "; Secure" } else { "" }
    ))
    .unwrap()
}
pub fn clear_cookie(secure: bool) -> HeaderValue {
    cookie("", secure, 0)
}

pub async fn initialized(pool: &SqlitePool) -> Result<bool> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM administrator")
        .fetch_one(pool)
        .await?;
    Ok(count != 0)
}

pub async fn prepare_bootstrap(state: &AppState) -> anyhow::Result<()> {
    let path = state.config.data_dir.join("bootstrap-token");
    if initialized(&state.pool)
        .await
        .map_err(|_| anyhow::anyhow!("cannot inspect initialization"))?
    {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        return Ok(());
    }
    let stored: Option<(String, Option<i64>)> =
        sqlx::query_as("SELECT token_hash, consumed_at FROM bootstrap WHERE id = 1")
            .fetch_optional(&state.pool)
            .await?;
    if let Some((hash, None)) = &stored
        && let Ok(token) = std::fs::read_to_string(&path)
        && constant_eq(&digest(token.trim()), hash)
    {
        crate::config::private_file(&path)?;
        return Ok(());
    }
    let token = random_token();
    crate::config::atomic_private_write(&path, format!("{token}\n").as_bytes())?;
    sqlx::query("INSERT INTO bootstrap(id, token_hash, consumed_at) VALUES(1, ?, NULL) ON CONFLICT(id) DO UPDATE SET token_hash=excluded.token_hash, consumed_at=NULL")
        .bind(digest(&token)).execute(&state.pool).await?;
    tracing::info!(
        "initialization required; use the bootstrap-token CLI command to read the one-time token"
    );
    Ok(())
}

pub async fn check_bootstrap(pool: &SqlitePool, token: &str) -> Result<()> {
    if token.len() != 43 || initialized(pool).await? {
        return Err(AppError::forbidden());
    }
    let hash: Option<String> =
        sqlx::query_scalar("SELECT token_hash FROM bootstrap WHERE id = 1 AND consumed_at IS NULL")
            .fetch_optional(pool)
            .await?;
    if !hash.is_some_and(|value| constant_eq(&digest(token), &value)) {
        return Err(AppError::forbidden());
    }
    Ok(())
}

pub async fn create_session(
    pool: &SqlitePool,
    ip: IpAddr,
    user_agent: &str,
    absolute_seconds: i64,
) -> Result<String> {
    let token = random_token();
    let timestamp = now();
    let mut transaction = pool.begin().await?;
    sqlx::query("DELETE FROM sessions WHERE absolute_expires_at <= ? OR token_hash IN (SELECT token_hash FROM sessions ORDER BY created_at DESC LIMIT -1 OFFSET 19)")
        .bind(timestamp).execute(&mut *transaction).await?;
    sqlx::query("INSERT INTO sessions(token_hash, created_at, last_seen, absolute_expires_at, ip, user_agent) VALUES(?, ?, ?, ?, ?, ?)")
        .bind(digest(&token)).bind(timestamp).bind(timestamp).bind(timestamp + absolute_seconds)
        .bind(ip.to_string()).bind(user_agent.chars().take(200).collect::<String>()).execute(&mut *transaction).await?;
    transaction.commit().await?;
    Ok(token)
}

pub async fn session_by_hash(
    pool: &SqlitePool,
    hash: &str,
    idle_seconds: i64,
    touch: bool,
) -> Result<Session> {
    let session: Option<Session> = sqlx::query_as("SELECT * FROM sessions WHERE token_hash = ?")
        .bind(hash)
        .fetch_optional(pool)
        .await?;
    let mut session = session.ok_or_else(AppError::unauthorized)?;
    let timestamp = now();
    if session.absolute_expires_at <= timestamp || session.last_seen + idle_seconds <= timestamp {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(hash)
            .execute(pool)
            .await?;
        return Err(AppError::unauthorized());
    }
    if touch && timestamp - session.last_seen >= 30 {
        sqlx::query("UPDATE sessions SET last_seen = ? WHERE token_hash = ?")
            .bind(timestamp)
            .bind(hash)
            .execute(pool)
            .await?;
        session.last_seen = timestamp;
    }
    Ok(session)
}

pub async fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<Identity> {
    let token = read_cookie(headers).ok_or_else(AppError::unauthorized)?;
    let idle = state.settings.read().await.session_idle_seconds;
    let session = session_by_hash(&state.pool, &digest(&token), idle, false).await?;
    Ok(Identity {
        session,
        csrf: csrf_token(&token),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginInput {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct AuthUser {
    pub username: String,
    pub csrf_token: String,
}

pub async fn username(pool: &SqlitePool) -> Result<String> {
    sqlx::query_scalar("SELECT username FROM administrator WHERE id = 1")
        .fetch_optional(pool)
        .await?
        .ok_or_else(AppError::unauthorized)
}

pub async fn login(
    state: &AppState,
    input: LoginInput,
    ip: IpAddr,
    user_agent: &str,
) -> Result<String> {
    state.limiter.login_attempt(ip, &input.username)?;
    if input.password.len() > 512 || input.username.len() > 128 {
        return Err(AppError::bad("用户名或密码格式不正确"));
    }
    let permit = state
        .auth_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| AppError::limited())?;
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT username, password_hash FROM administrator WHERE id = 1")
            .fetch_optional(&state.pool)
            .await?;
    let (stored_username, hash) = row.unwrap_or_else(|| (String::new(), state.dummy_hash.clone()));
    let candidate = input.username.trim().to_ascii_lowercase();
    let matches = constant_eq(&candidate, &stored_username.to_ascii_lowercase());
    let password = input.password;
    let verified = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        verify_password(&password, &hash)
    })
    .await
    .map_err(|_| AppError::internal())?;
    if !(verified && matches) {
        let delay = state.limiter.login_failed(ip, &input.username);
        tokio::time::sleep(delay).await;
        return Err(AppError::new(
            axum::http::StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "用户名或密码不正确",
        ));
    }
    state.limiter.login_succeeded(&input.username);
    let absolute = state.settings.read().await.session_absolute_seconds;
    create_session(&state.pool, ip, user_agent, absolute).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountInput {
    pub current_password: String,
    pub username: String,
    pub new_password: Option<String>,
    pub confirm_password: Option<String>,
}

pub async fn update_account(
    state: &AppState,
    input: AccountInput,
    identity: &Identity,
    ip: IpAddr,
) -> Result<String> {
    state.limiter.login_attempt(ip, "account-change")?;
    validate_username(&input.username)?;
    if input.current_password.len() > 512 {
        return Err(AppError::bad("密码长度无效"));
    }
    let old_hash: String =
        sqlx::query_scalar("SELECT password_hash FROM administrator WHERE id = 1")
            .fetch_one(&state.pool)
            .await?;
    let original_hash = old_hash.clone();
    let permit = state
        .auth_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| AppError::limited())?;
    let current = input.current_password;
    let verified = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        verify_password(&current, &old_hash)
    })
    .await
    .map_err(|_| AppError::internal())?;
    if !verified {
        tokio::time::sleep(state.limiter.login_failed(ip, "account-change")).await;
        return Err(AppError::new(
            axum::http::StatusCode::UNAUTHORIZED,
            "invalid_password",
            "原密码不正确",
        ));
    }
    let new_hash = if let Some(password) = input.new_password.filter(|value| !value.is_empty()) {
        validate_password(&password, input.confirm_password.as_deref().unwrap_or(""))?;
        let permit = state
            .auth_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::limited())?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            hash_password(&password)
        })
        .await
        .map_err(|_| AppError::internal())??
    } else {
        original_hash.clone()
    };
    let token = random_token();
    let timestamp = now();
    let absolute = state.settings.read().await.session_absolute_seconds;
    let mut transaction = state.pool.begin().await?;
    // Compare-and-swap also prevents concurrent password changes based on a stale password check.
    let updated = sqlx::query("UPDATE administrator SET username = ?, password_hash = ?, updated_at = ? WHERE id = 1 AND password_hash = ?")
        .bind(input.username).bind(new_hash).bind(timestamp).bind(original_hash).execute(&mut *transaction).await?;
    if updated.rows_affected() != 1 {
        return Err(AppError::unauthorized());
    }
    sqlx::query("DELETE FROM sessions")
        .execute(&mut *transaction)
        .await?;
    sqlx::query("INSERT INTO sessions(token_hash, created_at, last_seen, absolute_expires_at, ip, user_agent) VALUES(?, ?, ?, ?, ?, ?)")
        .bind(digest(&token)).bind(timestamp).bind(timestamp).bind(timestamp + absolute).bind(ip.to_string()).bind(&identity.session.user_agent)
        .execute(&mut *transaction).await?;
    transaction.commit().await?;
    state.limiter.login_succeeded("account-change");
    Ok(token)
}
