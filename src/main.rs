use clap::{Parser, Subcommand};
use fs2::FileExt;
use mihomo_web_panel::{
    AppState, api,
    config::{Config, private_dir},
};
use std::{fs::OpenOptions, path::PathBuf};

#[derive(Parser)]
#[command(
    version,
    about = "Secure independent web management for an existing Mihomo controller"
)]
struct Args {
    #[arg(long, global = true, default_value = "config.toml")]
    config: PathBuf,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Serve,
    BootstrapToken,
    CheckConfig,
    ResetWebSettings,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mihomo_web_panel=info,tower_http=warn".into()),
        )
        .with_target(false)
        .init();
    let args = Args::parse();
    let config = Config::load(&args.config)?;
    match args.command.unwrap_or(Commands::Serve) {
        Commands::BootstrapToken => {
            let token = std::fs::read_to_string(config.data_dir.join("bootstrap-token"))
                .map_err(|_| anyhow::anyhow!("initialization token unavailable; start the service first, or initialization is already complete"))?;
            let options = sqlx::sqlite::SqliteConnectOptions::new()
                .filename(config.data_dir.join("app.db"))
                .read_only(true);
            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(options)
                .await?;
            mihomo_web_panel::auth::check_bootstrap(&pool, token.trim())
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "initialization token has already been consumed or is unavailable"
                    )
                })?;
            pool.close().await;
            println!("{}", token.trim());
            return Ok(());
        }
        Commands::CheckConfig => {
            println!("配置校验通过");
            return Ok(());
        }
        Commands::ResetWebSettings => {
            #[cfg(unix)]
            {
                anyhow::ensure!(
                    unsafe { libc::geteuid() } != 0,
                    "run this command as the non-root web service user"
                );
                unsafe {
                    libc::umask(0o077);
                }
            }
            private_dir(&config.data_dir)?;
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(config.data_dir.join("instance.lock"))?;
            lock.try_lock_exclusive().map_err(|_| {
                anyhow::anyhow!("stop the web service before resetting its settings")
            })?;
            let pool = mihomo_web_panel::db::open(&config).await?;
            sqlx::query("DELETE FROM web_settings")
                .execute(&pool)
                .await?;
            pool.close().await;
            println!("Web 设置已恢复到本地 TOML 配置。管理员和内核连接配置保持可用。");
            return Ok(());
        }
        Commands::Serve => {}
    }
    #[cfg(unix)]
    {
        anyhow::ensure!(
            unsafe { libc::geteuid() } != 0,
            "the web service must run as a non-root user"
        );
        unsafe {
            libc::umask(0o077);
        }
    }
    private_dir(&config.data_dir)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(config.data_dir.join("instance.lock"))?;
    lock.try_lock_exclusive()
        .map_err(|_| anyhow::anyhow!("another service instance is using this data directory"))?;
    let state = AppState::new(config).await?;
    let address = state.active_settings.listen;
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(listen = %address, version = env!("CARGO_PKG_VERSION"), "mihomo-web-panel started");
    let signal_state = state.clone();
    let cleanup_state = state.clone();
    let app = api::router(state);
    let cleanup = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            let idle = cleanup_state.settings.read().await.session_idle_seconds;
            if sqlx::query(
                "DELETE FROM sessions WHERE absolute_expires_at <= ? OR last_seen + ? <= ?",
            )
            .bind(mihomo_web_panel::auth::now())
            .bind(idle)
            .bind(mihomo_web_panel::auth::now())
            .execute(&cleanup_state.pool)
            .await
            .is_err()
            {
                tracing::warn!("expired session cleanup failed");
            }
        }
    });
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        #[cfg(unix)]
        {
            let mut terminate =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("cannot install termination handler");
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        signal_state.shutdown.send_replace(true);
    })
    .await?;
    cleanup.abort();
    Ok(())
}
