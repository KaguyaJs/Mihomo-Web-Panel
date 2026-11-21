use crate::{
    AppState,
    error::{AppError, Result},
};
use serde::{Deserialize, Serialize};

pub const UNIT: &str = "mihomo-web.service";

pub fn caller_allowed(expected_uid: u32, actual_uid: u32) -> bool {
    expected_uid != 0 && expected_uid == actual_uid
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Operation {
    Enable,
    Disable,
    Status,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HelperRequest {
    pub operation: Operation,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct HelperStatus {
    pub supported: bool,
    pub enabled: bool,
    pub active: String,
    pub message: String,
}

pub fn fixed_args(operation: Operation) -> [&'static str; 3] {
    [
        match operation {
            Operation::Enable => "enable",
            Operation::Disable => "disable",
            Operation::Status => "is-enabled",
        },
        "--",
        UNIT,
    ]
}

fn unsupported() -> HelperStatus {
    HelperStatus {
        supported: false,
        enabled: false,
        active: "unsupported".into(),
        message: "开机自启控制需要 Linux systemd 和已安装的权限辅助程序，请按部署文档安装".into(),
    }
}

pub async fn request(state: &AppState, operation: Operation) -> Result<HelperStatus> {
    #[cfg(target_os = "linux")]
    {
        use tokio::{
            io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
            net::UnixStream,
        };
        let socket = &state.config.helper_socket;
        if !socket.exists() {
            return if operation == Operation::Status {
                Ok(unsupported())
            } else {
                Err(AppError::new(
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    "helper_unavailable",
                    "未安装 systemd 权限辅助程序，请按部署文档安装",
                ))
            };
        }
        let result = tokio::time::timeout(std::time::Duration::from_secs(12), async {
            let mut stream = UnixStream::connect(socket)
                .await
                .map_err(|_| AppError::internal())?;
            let peer = stream.peer_cred().map_err(|_| AppError::internal())?;
            if peer.uid() != 0 {
                return Err(AppError::forbidden());
            }
            let mut bytes = serde_json::to_vec(&HelperRequest { operation })
                .map_err(|_| AppError::internal())?;
            bytes.push(b'\n');
            stream
                .write_all(&bytes)
                .await
                .map_err(|_| AppError::internal())?;
            let reader = BufReader::new(stream);
            let mut line = String::new();
            let length = reader
                .take(2049)
                .read_line(&mut line)
                .await
                .map_err(|_| AppError::internal())?;
            if length > 2048 {
                return Err(AppError::internal());
            }
            serde_json::from_str::<HelperStatus>(&line).map_err(|_| AppError::internal())
        })
        .await;
        result.unwrap_or_else(|_| {
            Err(AppError::new(
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "helper_timeout",
                "权限辅助程序响应超时",
            ))
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = state;
        if operation == Operation::Status {
            Ok(unsupported())
        } else {
            Err(AppError::new(
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "systemd_unsupported",
                "当前系统不支持 systemd，开机自启操作无法执行",
            ))
        }
    }
}

#[cfg(target_os = "linux")]
pub async fn serve(allowed_uid: u32) -> anyhow::Result<()> {
    use std::{os::fd::FromRawFd, sync::Arc, time::Duration};
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::{UnixListener, UnixStream},
        process::Command,
        sync::Semaphore,
    };
    anyhow::ensure!(
        unsafe { libc::geteuid() } == 0,
        "helper must run as root through systemd socket activation"
    );
    anyhow::ensure!(allowed_uid != 0, "allowed uid must be non-root");
    anyhow::ensure!(
        std::env::var("LISTEN_FDS").ok().as_deref() == Some("1")
            && std::env::var("LISTEN_PID").ok().as_deref() == Some(&std::process::id().to_string()),
        "helper requires exactly one systemd-activated socket"
    );
    anyhow::ensure!(
        std::path::Path::new("/run/systemd/system").exists(),
        "systemd is unavailable"
    );
    let listener = unsafe { std::os::unix::net::UnixListener::from_raw_fd(3) };
    listener.set_nonblocking(true)?;
    let listener = UnixListener::from_std(listener)?;
    let slots = Arc::new(Semaphore::new(8));

    async fn command(args: &[&str]) -> anyhow::Result<std::process::Output> {
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            Command::new("/usr/bin/systemctl")
                .args(args)
                .env_clear()
                .env("LANG", "C")
                .kill_on_drop(true)
                .output(),
        )
        .await??;
        Ok(result)
    }
    async fn handle(mut socket: UnixStream, uid: u32) -> anyhow::Result<()> {
        anyhow::ensure!(
            caller_allowed(uid, socket.peer_cred()?.uid()),
            "caller uid rejected"
        );
        let request = {
            let reader = BufReader::new(&mut socket);
            let mut bytes = Vec::new();
            use tokio::io::AsyncReadExt;
            let length = tokio::time::timeout(
                Duration::from_secs(2),
                reader.take(257).read_until(b'\n', &mut bytes),
            )
            .await??;
            anyhow::ensure!(
                length <= 256 && bytes.last() == Some(&b'\n'),
                "invalid request length"
            );
            serde_json::from_slice::<HelperRequest>(&bytes)?
        };
        let operation = request.operation;
        if operation != Operation::Status {
            let changed = command(&fixed_args(operation)).await?;
            anyhow::ensure!(changed.status.success(), "systemd operation failed");
            tracing::info!(caller_uid = uid, operation = ?operation, "autostart setting changed");
        }
        let enabled = command(&fixed_args(Operation::Status)).await?;
        let enabled_text = String::from_utf8_lossy(&enabled.stdout).trim().to_string();
        let active = command(&["is-active", "--", UNIT]).await?;
        let active_text = String::from_utf8_lossy(&active.stdout).trim().to_string();
        let active = if matches!(
            active_text.as_str(),
            "active" | "inactive" | "failed" | "activating" | "deactivating"
        ) {
            active_text
        } else {
            "unknown".into()
        };
        let status = HelperStatus {
            supported: true,
            enabled: enabled.status.success() && enabled_text == "enabled",
            active,
            message: "".into(),
        };
        let mut bytes = serde_json::to_vec(&status)?;
        bytes.push(b'\n');
        socket.write_all(&bytes).await?;
        Ok(())
    }
    loop {
        let (socket, _) = listener.accept().await?;
        let Ok(permit) = slots.clone().try_acquire_owned() else {
            continue;
        };
        tokio::spawn(async move {
            let _permit = permit;
            if handle(socket, allowed_uid).await.is_err() {
                tracing::warn!("helper rejected or failed a request");
            }
        });
    }
}
