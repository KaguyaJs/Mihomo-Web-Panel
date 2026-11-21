use anyhow::{Context, bail};
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
};
use url::{Host, Url};

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub listen: SocketAddr,
    pub public_origin: String,
    pub data_dir: PathBuf,
    pub secure_cookies: bool,
    pub trusted_proxies: Vec<IpNet>,
    pub trusted_backend_networks: Vec<IpNet>,
    pub session_idle_seconds: i64,
    pub session_absolute_seconds: i64,
    pub traffic_interval_ms: u64,
    pub request_timeout_seconds: u64,
    pub delay_test_url: String,
    pub helper_socket: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:8787".parse().unwrap(),
            public_origin: "http://127.0.0.1:8787".into(),
            data_dir: "./data".into(),
            secure_cookies: false,
            trusted_proxies: vec![],
            trusted_backend_networks: vec![],
            session_idle_seconds: 1800,
            session_absolute_seconds: 86400,
            traffic_interval_ms: 1000,
            request_timeout_seconds: 10,
            delay_test_url: "https://www.gstatic.com/generate_204".into(),
            helper_socket: "/run/mihomo-web-helper/control.sock".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WebSettings {
    pub listen: SocketAddr,
    pub public_origin: String,
    pub session_idle_seconds: i64,
    pub session_absolute_seconds: i64,
    pub traffic_interval_ms: u64,
}

impl From<&Config> for WebSettings {
    fn from(c: &Config) -> Self {
        Self {
            listen: c.listen,
            public_origin: c.public_origin.clone(),
            session_idle_seconds: c.session_idle_seconds,
            session_absolute_seconds: c.session_absolute_seconds,
            traffic_interval_ms: c.traffic_interval_ms,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path).context("cannot read configuration")?;
        if content.len() > 65536 {
            bail!("configuration is too large")
        }
        let config: Self = toml::from_str(&content).context("invalid configuration")?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        self.validate_web(&WebSettings::from(self))?;
        if !(1..=30).contains(&self.request_timeout_seconds) {
            bail!("request_timeout_seconds must be 1..30")
        }
        let test_url = Url::parse(&self.delay_test_url).context("invalid delay_test_url")?;
        if test_url.scheme() != "https"
            || test_url.host().is_none()
            || !test_url.username().is_empty()
            || test_url.password().is_some()
        {
            bail!("delay_test_url must be an HTTPS URL without credentials")
        }
        for network in &self.trusted_backend_networks {
            if !is_private(network.network()) || !is_private(network.broadcast()) {
                bail!("trusted_backend_networks must be limited to RFC1918 or IPv6 ULA ranges")
            }
        }
        Ok(())
    }
    pub fn validate_web(&self, settings: &WebSettings) -> anyhow::Result<()> {
        let origin = Url::parse(&settings.public_origin).context("invalid public_origin")?;
        if origin.path() != "/"
            || origin.query().is_some()
            || origin.fragment().is_some()
            || !origin.username().is_empty()
            || origin.password().is_some()
        {
            bail!("public_origin must be an origin without a path or credentials")
        }
        if origin.origin().ascii_serialization() != settings.public_origin
            || settings.listen.port() < 1024
        {
            bail!(
                "public_origin must be canonical and the non-root listen port must be 1024..65535"
            )
        }
        match origin.scheme() {
            "https" if self.secure_cookies => {}
            "http"
                if !self.secure_cookies
                    && settings.listen.ip().is_loopback()
                    && origin.host().is_some_and(|h| match h {
                        Host::Ipv4(ip) => ip.is_loopback(),
                        Host::Ipv6(ip) => ip.is_loopback(),
                        _ => false,
                    }) => {}
            _ => bail!(
                "public access requires HTTPS and Secure cookies; HTTP is allowed only on loopback for development"
            ),
        }
        if !(300..=86400).contains(&settings.session_idle_seconds)
            || !(3600..=604800).contains(&settings.session_absolute_seconds)
            || settings.session_idle_seconds > settings.session_absolute_seconds
            || !(500..=5000).contains(&settings.traffic_interval_ms)
        {
            bail!("invalid session lifetime or traffic interval")
        }
        Ok(())
    }
}

pub fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_private(),
        IpAddr::V6(v) => (v.segments()[0] & 0xfe00) == 0xfc00,
    }
}

pub fn validate_controller(input: &str, trusted: &[IpNet]) -> crate::error::Result<Url> {
    let url = Url::parse(input)
        .map_err(|_| crate::error::AppError::bad("请输入合法的 Controller 地址"))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || input.len() > 512
    {
        return Err(crate::error::AppError::bad(
            "Controller 只支持 HTTP/HTTPS 根地址，禁止凭据、路径与重定向",
        ));
    }
    let ip = match url.host() {
        Some(Host::Ipv4(v)) => IpAddr::V4(v),
        Some(Host::Ipv6(v)) => IpAddr::V6(v),
        _ => {
            return Err(crate::error::AppError::bad(
                "为避免 DNS 重绑定，请使用经过授权的 IP 地址",
            ));
        }
    };
    // IPv4-mapped IPv6 is deliberately rejected rather than relying on platform-specific routing.
    if matches!(ip, IpAddr::V6(v) if v.to_ipv4_mapped().is_some()) {
        return Err(crate::error::AppError::bad("不允许 IPv4 映射地址"));
    }
    if ip == "fd00:ec2::254".parse::<IpAddr>().unwrap() {
        return Err(crate::error::AppError::bad("不允许访问云元数据地址"));
    }
    if !ip.is_loopback() && !(is_private(ip) && trusted.iter().any(|network| network.contains(&ip)))
    {
        return Err(crate::error::AppError::bad(
            "Controller 必须是本机回环或本地配置明确授权的可信私网地址",
        ));
    }
    if url.port_or_known_default() == Some(0) {
        return Err(crate::error::AppError::bad("Controller 端口无效"));
    }
    Ok(url)
}

pub fn private_dir(path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(path).context("cannot create private data directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub fn private_file(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

pub fn atomic_private_write(path: &Path, content: &[u8]) -> anyhow::Result<()> {
    let temporary = path.with_extension(format!("tmp-{}", crate::auth::random_token()));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> anyhow::Result<()> {
        let mut file = options.open(&temporary)?;
        file.write_all(content)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        #[cfg(unix)]
        {
            std::fs::File::open(path.parent().context("missing parent")?)?.sync_all()?;
        }
        private_file(path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
