use clap::Parser;

#[derive(Parser)]
#[command(version, about = "Fixed-unit, UID-checked systemd socket helper")]
struct Args {
    #[arg(long)]
    allowed_uid: u32,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();
    let args = Args::parse();
    #[cfg(target_os = "linux")]
    {
        mihomo_web_panel::helper::serve(args.allowed_uid).await
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = args;
        anyhow::bail!("systemd helper is supported only on Linux")
    }
}
