//! Standalone startup paths, first-run configuration and browser launch.
use crate::config::{Config, atomic_private_write, private_dir};
use anyhow::{Context, bail, ensure};
use fs2::FileExt;
use std::{
    fs::OpenOptions,
    path::{Path, PathBuf},
};

const APP: &str = "mihomo-web-panel";

pub struct RuntimePaths {
    pub config: PathBuf,
    pub default_data: PathBuf,
    pub managed: bool,
}

impl RuntimePaths {
    pub fn resolve(explicit: Option<&Path>, portable: bool) -> anyhow::Result<Self> {
        if let Some(path) = explicit {
            return Ok(Self {
                config: std::path::absolute(path)?,
                default_data: PathBuf::new(),
                managed: false,
            });
        }
        if portable {
            let executable = std::env::current_exe().context("cannot locate the executable")?;
            return Self::portable(executable.parent().context("executable has no parent")?);
        }
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
        let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
        Self::user(
            std::env::consts::OS,
            home.as_deref(),
            local.as_deref(),
            config.as_deref(),
            data.as_deref(),
        )
    }

    fn portable(directory: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            config: std::path::absolute(directory)?.join("config.toml"),
            default_data: PathBuf::from("data"),
            managed: true,
        })
    }

    fn user(
        os: &str,
        home: Option<&Path>,
        local: Option<&Path>,
        xdg_config: Option<&Path>,
        xdg_data: Option<&Path>,
    ) -> anyhow::Result<Self> {
        fn absolute(path: Option<&Path>) -> Option<&Path> {
            path.filter(|path| path.is_absolute())
        }
        let (config, data) = match os {
            "windows" => {
                let folder = absolute(local)
                    .context("LOCALAPPDATA must point to a user directory")?
                    .join(APP);
                (folder.join("config.toml"), folder.join("data"))
            }
            "macos" => {
                let folder = absolute(home)
                    .context("cannot locate the user's home directory")?
                    .join("Library/Application Support")
                    .join(APP);
                (folder.join("config.toml"), folder.join("data"))
            }
            "linux" => {
                let home = absolute(home).context("cannot locate the user's home directory")?;
                let config = absolute(xdg_config)
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| home.join(".config"));
                let data = absolute(xdg_data)
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| home.join(".local/share"));
                (config.join(APP).join("config.toml"), data.join(APP))
            }
            _ => bail!("unsupported operating system"),
        };
        Ok(Self {
            config,
            default_data: data,
            managed: true,
        })
    }

    pub fn load(&self, create: bool) -> anyhow::Result<Config> {
        if !self.managed || (!create && self.config.exists()) {
            return Config::load(&self.config);
        }
        if !create {
            let config = Config {
                data_dir: self.resolved_default_data(),
                ..Config::default()
            };
            config.validate()?;
            return Ok(config);
        }
        let directory = self
            .config
            .parent()
            .context("configuration has no parent directory")?;
        // Only automatically managed application directories have their ACLs
        // restricted here. Explicit service configuration stays administrator-owned.
        // In portable mode the executable's directory can also contain other files.
        // Keep its permissions intact; protect config and the separate data folder.
        if self.default_data.is_absolute() {
            private_dir(directory)?;
        } else {
            std::fs::create_dir_all(directory)?;
        }
        let lock_path = directory.join("config.lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;
        crate::config::private_file(&lock_path)?;
        lock.lock_exclusive()?;
        if !self.config.exists() {
            let config = Config {
                data_dir: self.default_data.clone(),
                ..Config::default()
            };
            atomic_private_write(&self.config, toml::to_string_pretty(&config)?.as_bytes())?;
        }
        let config = Config::load(&self.config)?;
        private_dir(&config.data_dir)?;
        Ok(config)
    }

    fn resolved_default_data(&self) -> PathBuf {
        if self.default_data.is_absolute() {
            self.default_data.clone()
        } else {
            self.config.parent().unwrap().join(&self.default_data)
        }
    }
}

pub fn open_browser(origin: &str) -> anyhow::Result<()> {
    let parsed = url::Url::parse(origin)?;
    ensure!(
        matches!(parsed.scheme(), "http" | "https")
            && parsed.origin().ascii_serialization() == origin,
        "invalid browser origin"
    );
    #[cfg(windows)]
    {
        crate::windows::open_browser(origin)
    }
    #[cfg(not(windows))]
    {
        let program = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let status = std::process::Command::new(program).arg(origin).status()?;
        ensure!(status.success(), "cannot open the default browser");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_paths_and_relative_xdg_fallback_are_stable() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path();
        let mac = RuntimePaths::user("macos", Some(base), None, None, None).unwrap();
        assert_eq!(
            mac.config,
            base.join("Library/Application Support/mihomo-web-panel/config.toml")
        );
        let win = RuntimePaths::user("windows", None, Some(base), None, None).unwrap();
        assert_eq!(win.default_data, base.join("mihomo-web-panel/data"));
        let linux =
            RuntimePaths::user("linux", Some(base), None, Some(Path::new("relative")), None)
                .unwrap();
        assert_eq!(
            linux.config,
            base.join(".config/mihomo-web-panel/config.toml")
        );
        assert!(RuntimePaths::user("windows", None, None, None, None).is_err());
    }

    #[test]
    fn portable_config_is_created_once_and_uses_its_own_directory() {
        let root = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::portable(root.path()).unwrap();
        assert!(!paths.config.exists());
        assert_eq!(
            paths.load(false).unwrap().data_dir,
            root.path().join("data")
        );
        assert!(!paths.config.exists());
        let first = paths.load(true).unwrap();
        assert_eq!(first.data_dir, root.path().join("data"));
        assert!(first.listen.ip().is_loopback());
        let customized = b"listen = '127.0.0.1:9876'\npublic_origin = 'http://127.0.0.1:9876'\ndata_dir = 'kept-data'\n";
        atomic_private_write(&paths.config, customized).unwrap();
        let second = paths.load(true).unwrap();
        assert_eq!(second.listen.port(), 9876);
        assert_eq!(second.data_dir, root.path().join("kept-data"));
        assert_eq!(std::fs::read(&paths.config).unwrap(), customized);
    }

    #[test]
    fn explicit_missing_or_invalid_config_never_falls_back() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("missing.toml");
        let paths = RuntimePaths::resolve(Some(&file), false).unwrap();
        assert!(paths.load(true).is_err());
        assert!(!file.exists());
        atomic_private_write(&file, b"not = 'configuration'").unwrap();
        assert!(paths.load(true).is_err());
    }
}
