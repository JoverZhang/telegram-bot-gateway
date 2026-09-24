mod updates;
use serde_json::{Value, json};
pub(crate) use updates::{Event, TopicEvent, Update};
#[derive(Clone)]
pub(crate) struct TelegramClient {
    http: reqwest::Client,
    base: String,
    token: String,
}
#[derive(Debug, Clone)]
pub(crate) struct Failure {
    pub code: u16,
    pub retry_after: Option<i64>,
    pub detail: String,
}
impl TelegramClient {
    pub fn new(token: String) -> std::result::Result<Self, String> {
        #[allow(unused_mut)]
        let mut base = "https://api.telegram.org".to_string();
        #[cfg(feature = "test-support")]
        if let Ok(s) = std::env::var("TBG_TEST_TELEGRAM_URL") {
            base = s;
        }
        Ok(Self {
            token,
            base,
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(5))
                .timeout(std::time::Duration::from_secs(35))
                .build()
                .map_err(|_| "cannot create Telegram client")?,
        })
    }
    async fn call(&self, method: &str, payload: Value) -> std::result::Result<Value, Failure> {
        let r = self
            .http
            .post(format!("{}/bot{}/{method}", self.base, self.token))
            .json(&payload)
            .send()
            .await
            .map_err(|_| Failure {
                code: 0,
                retry_after: None,
                detail: "Telegram connection failed or send outcome unknown".into(),
            })?;
        let status = r.status().as_u16();
        let v: Value = r.json().await.map_err(|_| Failure {
            code: 0,
            retry_after: None,
            detail: "Telegram response incomplete; outcome unknown".into(),
        })?;
        if status >= 400 || v["ok"] != true {
            return Err(Failure {
                code: v["error_code"].as_u64().map(|n| n as u16).unwrap_or(status),
                retry_after: v["parameters"]["retry_after"].as_i64(),
                detail: v["description"]
                    .as_str()
                    .unwrap_or("Telegram rejected request")
                    .replace(&self.token, "[redacted]"),
            });
        }
        Ok(v["result"].clone())
    }
}

impl TelegramClient {
    pub async fn identity(&self) -> Result<i64, Failure> {
        let me = self.call("getMe", json!({})).await?;
        let webhook = self.call("getWebhookInfo", json!({})).await?;
        if webhook["url"].as_str().is_some_and(|url| !url.is_empty()) {
            return Err(Failure {
                code: 409,
                retry_after: None,
                detail: "existing webhook prevents polling; configuration was not changed".into(),
            });
        }
        me["id"]
            .as_i64()
            .ok_or_else(|| Failure::invalid("missing Bot identity"))
    }
    pub async fn create_topic(&self, chat: i64, name: &str) -> Result<i64, Failure> {
        let value = self
            .call("createForumTopic", json!({"chat_id": chat, "name": name}))
            .await?;
        value["message_thread_id"]
            .as_i64()
            .ok_or_else(|| Failure::invalid("missing Topic identifier"))
    }
    pub async fn topic_state(&self, chat: i64, thread: i64, closed: bool) -> Result<(), Failure> {
        self.call(
            if closed {
                "closeForumTopic"
            } else {
                "reopenForumTopic"
            },
            json!({"chat_id":chat,"message_thread_id":thread}),
        )
        .await?;
        Ok(())
    }
    pub async fn deliver(
        &self,
        chat: i64,
        delivery: &crate::model::Delivery,
    ) -> Result<Option<i64>, Failure> {
        use crate::model::Delivery;
        match delivery {
            Delivery::Send {
                text,
                thread,
                quote,
            } => {
                let mut payload = json!({"chat_id":chat,"text":text});
                if let Some(thread) = thread {
                    payload["message_thread_id"] = json!(thread);
                }
                if let Some(quote) = quote {
                    payload["reply_parameters"] = json!({"message_id":quote});
                }
                let result = self.call("sendMessage", payload).await?;
                result["message_id"].as_i64().map(Some).ok_or_else(|| {
                    Failure::invalid("missing message identifier; delivery outcome unknown")
                })
            }
            Delivery::Heart { target } => {
                let target = target.ok_or_else(|| Failure::invalid("missing receipt target"))?;
                self.call("setMessageReaction", json!({"chat_id":chat,"message_id":target,"reaction":[{"type":"emoji","emoji":"❤"}]})).await?;
                Ok(None)
            }
        }
    }
}
impl Failure {
    fn invalid(detail: &str) -> Self {
        Self {
            code: 0,
            retry_after: None,
            detail: detail.into(),
        }
    }
}
