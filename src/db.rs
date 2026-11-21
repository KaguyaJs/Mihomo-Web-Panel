use crate::config::{Config, WebSettings, private_dir, private_file};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::time::Duration;

pub async fn open(config: &Config) -> anyhow::Result<SqlitePool> {
    private_dir(&config.data_dir)?;
    let path = config.data_dir.join("app.db");
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Full)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options)
        .await?;
    private_file(&path)?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    for suffix in ["app.db-wal", "app.db-shm"] {
        let extra = config.data_dir.join(suffix);
        if extra.exists() {
            private_file(&extra)?;
        }
    }
    Ok(pool)
}

pub async fn load_settings(pool: &SqlitePool, config: &Config) -> anyhow::Result<WebSettings> {
    let json: Option<String> =
        sqlx::query_scalar("SELECT settings_json FROM web_settings WHERE id = 1")
            .fetch_optional(pool)
            .await?;
    let settings = match json {
        Some(value) => serde_json::from_str(&value)?,
        None => WebSettings::from(config),
    };
    config.validate_web(&settings)?;
    Ok(settings)
}
