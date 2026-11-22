pub mod api;
pub mod assets;
pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod helper;
pub mod licenses;
pub mod mihomo;
pub mod runtime;
pub mod security;
pub mod streams;

#[cfg(windows)]
mod windows;

use config::{Config, WebSettings};
use mihomo::{BackendConfig, MihomoClient};
use sqlx::SqlitePool;
use std::{sync::Arc, time::Instant};
use tokio::sync::{RwLock, Semaphore, watch};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub active_settings: Arc<WebSettings>,
    pub settings: Arc<RwLock<WebSettings>>,
    pub pool: SqlitePool,
    pub backend: Arc<RwLock<Option<Arc<MihomoClient>>>>,
    pub backend_changes: watch::Sender<u64>,
    pub limiter: Arc<security::RateLimiter>,
    pub auth_slots: Arc<Semaphore>,
    pub delay_slots: Arc<Semaphore>,
    pub select_lock: Arc<tokio::sync::Mutex<()>>,
    pub bootstrap_lock: Arc<tokio::sync::Mutex<()>>,
    pub browser_stream_slots: Arc<Semaphore>,
    pub traffic_stream: Arc<streams::SharedStream>,
    pub log_stream: Arc<streams::SharedStream>,
    pub dummy_hash: String,
    pub started: Instant,
    pub shutdown: watch::Sender<bool>,
}

impl AppState {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        config.validate()?;
        let pool = db::open(&config).await?;
        let settings = db::load_settings(&pool, &config).await?;
        let stored: Option<(String, String)> =
            sqlx::query_as("SELECT controller_url, secret FROM backend WHERE id=1")
                .fetch_optional(&pool)
                .await?;
        let backend = stored
            .map(|(controller_url, secret)| {
                MihomoClient::new(
                    &config,
                    BackendConfig {
                        controller_url,
                        secret,
                    },
                )
            })
            .transpose()
            .map_err(|_| anyhow::anyhow!("saved controller is outside the local trust policy"))?
            .map(Arc::new);
        let (backend_changes, _) = watch::channel(0);
        let (shutdown, _) = watch::channel(false);
        let dummy_hash =
            tokio::task::spawn_blocking(|| auth::hash_password(&auth::random_token())).await??;
        let state = Self {
            config: Arc::new(config),
            active_settings: Arc::new(settings.clone()),
            settings: Arc::new(RwLock::new(settings)),
            pool,
            backend: Arc::new(RwLock::new(backend)),
            backend_changes,
            limiter: Arc::new(security::RateLimiter::default()),
            auth_slots: Arc::new(Semaphore::new(4)),
            delay_slots: Arc::new(Semaphore::new(4)),
            select_lock: Arc::new(tokio::sync::Mutex::new(())),
            bootstrap_lock: Arc::new(tokio::sync::Mutex::new(())),
            browser_stream_slots: Arc::new(Semaphore::new(32)),
            traffic_stream: streams::SharedStream::new(false),
            log_stream: streams::SharedStream::new(true),
            dummy_hash,
            started: Instant::now(),
            shutdown,
        };
        auth::prepare_bootstrap(&state).await?;
        Ok(state)
    }
    pub async fn client(&self) -> error::Result<Arc<MihomoClient>> {
        self.backend
            .read()
            .await
            .clone()
            .ok_or_else(error::AppError::unavailable)
    }
}
