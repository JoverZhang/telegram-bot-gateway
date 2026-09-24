use super::{Failure, TelegramClient};
use serde_json::{Value, json};

pub(crate) struct Update {
    pub id: i64,
    pub raw: String,
    pub event: Event,
}
pub(crate) enum Event {
    Membership {
        chat: i64,
        title: String,
        actor: i64,
        present: bool,
    },
    Message(Message),
    Other,
}
pub(crate) struct Message {
    pub id: i64,
    pub chat: i64,
    pub title: String,
    pub group: bool,
    pub user: i64,
    pub language: String,
    pub thread: Option<i64>,
    pub date: i64,
    pub content: String,
    pub quote: Option<i64>,
    pub topic_event: Option<TopicEvent>,
}
pub(crate) enum TopicEvent {
    Created(String),
    Closed,
    Reopened,
}
impl Update {
    fn decode(value: Value) -> Result<Self, Failure> {
        let id = value["update_id"]
            .as_i64()
            .ok_or_else(|| Failure::invalid("missing update_id"))?;
        let event = if let Some(member) = value.get("my_chat_member") {
            Event::Membership {
                chat: required_id(&member["chat"], "id")?,
                title: member["chat"]["title"].as_str().unwrap_or("Group").into(),
                actor: required_id(&member["from"], "id")?,
                present: !matches!(
                    member["new_chat_member"]["status"].as_str(),
                    Some("left" | "kicked")
                ),
            }
        } else if let Some(message) = value.get("message") {
            // Anonymous administrator/channel messages have no attributable User actor.
            let service = [
                "forum_topic_created",
                "forum_topic_closed",
                "forum_topic_reopened",
            ]
            .iter()
            .any(|key| message.get(*key).is_some());
            if !service
                && (message.get("sender_chat").is_some() || message["from"]["is_bot"] == true)
            {
                Event::Other
            } else if let Some(user) =
                message["from"]["id"]
                    .as_i64()
                    .or(if service { Some(0) } else { None })
            {
                let topic_event = if let Some(created) = message.get("forum_topic_created") {
                    Some(TopicEvent::Created(
                        created["name"].as_str().unwrap_or("").into(),
                    ))
                } else if message.get("forum_topic_closed").is_some() {
                    Some(TopicEvent::Closed)
                } else if message.get("forum_topic_reopened").is_some() {
                    Some(TopicEvent::Reopened)
                } else {
                    None
                };
                let content = message["text"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        let kind = [
                            "photo",
                            "document",
                            "voice",
                            "video",
                            "audio",
                            "sticker",
                            "animation",
                            "contact",
                            "location",
                            "poll",
                        ]
                        .into_iter()
                        .find(|kind| message.get(*kind).is_some())
                        .unwrap_or("unsupported message");
                        let caption = message["caption"].as_str().unwrap_or("");
                        if caption.is_empty() {
                            format!("[{kind}]")
                        } else {
                            format!("[{kind}] {caption}")
                        }
                    });
                Event::Message(Message {
                    id: required_id(message, "message_id")?,
                    chat: required_id(&message["chat"], "id")?,
                    title: message["chat"]["title"].as_str().unwrap_or("Group").into(),
                    group: message["chat"]["type"].as_str() != Some("private"),
                    user,
                    language: message["from"]["language_code"]
                        .as_str()
                        .unwrap_or("")
                        .into(),
                    thread: message["message_thread_id"].as_i64(),
                    date: required_id(message, "date")?.saturating_mul(1000),
                    quote: message["reply_to_message"]["message_id"].as_i64(),
                    content,
                    topic_event,
                })
            } else {
                Event::Other
            }
        } else {
            Event::Other
        };
        Ok(Self {
            id,
            raw: value.to_string(),
            event,
        })
    }
}
fn required_id(value: &Value, key: &str) -> Result<i64, Failure> {
    value[key]
        .as_i64()
        .ok_or_else(|| Failure::invalid(&format!("missing Telegram {key}")))
}
impl TelegramClient {
    pub async fn updates(&self, offset: Option<i64>) -> Result<Vec<Update>, Failure> {
        let mut body = json!({"timeout":25,"allowed_updates":["message","my_chat_member"]});
        if let Some(offset) = offset {
            body["offset"] = json!(offset);
        }
        let response = self.call("getUpdates", body).await?;
        response
            .as_array()
            .ok_or_else(|| Failure::invalid("invalid updates response"))?
            .iter()
            .cloned()
            .map(Update::decode)
            .collect()
    }
}
