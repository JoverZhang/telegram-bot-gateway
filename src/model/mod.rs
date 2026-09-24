use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone)]
pub enum ErrorKind {
    Invalid,
    Conflict,
    Unavailable,
    NotFound,
}
#[derive(Debug, Clone)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn bad(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Invalid,
            message: message.into(),
        }
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Conflict,
            message: message.into(),
        }
    }
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Unavailable,
            message: message.into(),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for Error {}
pub fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
pub fn timestamp(t: i64) -> String {
    chrono::DateTime::from_timestamp_millis(t)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
pub fn msg_id(n: i64) -> String {
    format!("m{n}")
}
pub fn parse_msg(s: &str) -> Result<i64> {
    s.strip_prefix('m')
        .and_then(|x| x.parse().ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| Error::bad("invalid msg_id"))
}
#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub msg_id: String,
    pub sent_at: String,
    pub sender: Value,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote_msg_id: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Topic {
    pub id: String,
    pub chat: i64,
    pub thread: i64,
    pub name: String,
    pub owner: Option<String>,
    pub closed: bool,
    pub available: bool,
}
#[derive(Clone, Debug)]
pub struct Job {
    pub id: i64,
    pub message: Option<i64>,
    pub chat: i64,
    pub payload: Delivery,
    pub attempts: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum Delivery {
    Send {
        text: String,
        thread: Option<i64>,
        quote: Option<i64>,
    },
    Heart {
        target: Option<i64>,
    },
}
