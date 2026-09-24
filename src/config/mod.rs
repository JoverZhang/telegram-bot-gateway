use serde::Deserialize;
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientConfig {
    pub host: IpAddr,
    pub port: u16,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelegramConfig {
    pub bot_token: String,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub listen: SocketAddr,
    pub telegram: TelegramConfig,
    #[serde(default)]
    pub admins: Vec<i64>,
    pub data_dir: Option<PathBuf>,
}
fn home() -> std::result::Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("HOME is not set".into())
}
fn root() -> std::result::Result<PathBuf, String> {
    #[cfg(feature = "test-support")]
    if let Some(p) = std::env::var_os("TBG_TEST_CONFIG_DIR") {
        return Ok(p.into());
    }
    Ok(home()?.join(".config/tbg"))
}
fn read<T: serde::de::DeserializeOwned>(name: &str) -> std::result::Result<T, String> {
    let path = root()?.join(name);
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    // Do not echo parser source snippets: the Server file contains a Bot token.
    serde_yaml_ng::from_slice(&bytes).map_err(|_| {
        format!(
            "{}: invalid or missing configuration fields",
            path.display()
        )
    })
}
impl ClientConfig {
    pub fn load() -> std::result::Result<Self, String> {
        let c: Self = read("client.yaml")?;
        if c.port == 0 {
            return Err("port must be nonzero".into());
        }
        Ok(c)
    }
    pub fn endpoint(&self) -> String {
        format!("http://{}", SocketAddr::new(self.host, self.port))
    }
}
impl ServerConfig {
    pub fn load() -> std::result::Result<Self, String> {
        let c: Self = read("server.yaml")?;
        if c.listen.port() == 0 || c.telegram.bot_token.trim().is_empty() {
            return Err("listen requires a nonzero port and bot_token must not be empty".into());
        }
        Ok(c)
    }
    pub fn data_path(&self) -> std::result::Result<PathBuf, String> {
        self.data_dir
            .clone()
            .map(Ok)
            .unwrap_or_else(|| Ok(home()?.join(".local/share/tbg")))
    }
}
