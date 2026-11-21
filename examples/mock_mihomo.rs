//! Test fixture only. This is never compiled into or started by the production application.
#[path = "../tests/support/mod.rs"]
mod support;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let address = std::env::var("MOCK_BIND").unwrap_or_else(|_| "127.0.0.1:19090".into());
    let state = support::MockState::default();
    *state.url.lock().await = format!("http://{address}");
    let listener = tokio::net::TcpListener::bind(&address).await?;
    println!("Test-only Mock Mihomo ready on {address}");
    axum::serve(listener, support::mock_router(state)).await?;
    Ok(())
}
