use super::*;
use crate::model::{Delivery, msg_id, now, parse_msg};

impl Gateway {
    pub(super) async fn communication(
        &self,
        route: String,
        agent: String,
        body: Value,
    ) -> Result<Value> {
        let write = !matches!(
            route.as_str(),
            "whoami" | "subscriptions" | "history" | "unread"
        );
        self.db.run(write, move |tx| {
            let topic_id = body["topic"].as_str().unwrap_or("");
            let result = match route.as_str() {
                "whoami" => json!({"name":agent}),
                "subscriptions" => json!({"subscriptions":tx.subs(&agent)?}),
                "subscribe" => {
                    tx.subscribe(&agent, topic_id, body["muted"].as_bool().unwrap_or(false))?;
                    json!({"topic":topic_id})
                }
                "unsubscribe" => {
                    tx.unsubscribe(&agent, topic_id)?;
                    json!({"topic":topic_id})
                }
                "mute" => {
                    let muted = !body["off"].as_bool().unwrap_or(false);
                    tx.mute(&agent, topic_id, muted)?;
                    json!({"topic":topic_id,"muted":muted})
                }
                "ack" => {
                    let through = parse_msg(body["through"].as_str().unwrap())?;
                    let (old, _) = tx.sub(&agent, topic_id)?;
                    tx.message_in(topic_id, through)?;
                    let boundary = old.unwrap_or(0).max(through);
                    if old != Some(boundary) {
                        let topic = tx.topic(topic_id)?;
                        for (id, _) in tx.pending(&agent, topic_id, old)? {
                            if id > through { break; }
                            tx.enqueue("reaction", Some(id), topic.chat, Delivery::Heart { target: None })?;
                        }
                        tx.set_ack(&agent, topic_id, boundary)?;
                    }
                    json!({"last_acked_msg_id":msg_id(boundary)})
                }
                "history" | "unread" => {
                    tx.topic(topic_id)?;
                    let forward = route == "unread";
                    let base = if forward { tx.sub(&agent, topic_id)?.0 } else { None };
                    let cursor = if let Some(cursor) = body["cursor"].as_str() {
                        let id = parse_msg(cursor)?;
                        tx.message_in(topic_id, id)?;
                        Some(id)
                    } else { base };
                    let limit = body["limit"].as_i64().unwrap_or(20);
                    let (messages, remaining) = tx.messages(topic_id, cursor, forward, limit)?;
                    let next = messages.last().map(|message| message.msg_id.clone()).or_else(|| cursor.map(msg_id));
                    json!({"messages":messages,"next_cursor":next,"remaining_count":remaining})
                }
                "send" => {
                    let topic = tx.topic(topic_id)?;
                    if topic.closed || !topic.available { return Err(Error::conflict("Topic is closed or Group is unavailable")); }
                    let content = body["content"].as_str().unwrap();
                    let text = crate::bot::conversation(&agent, content);
                    if content.is_empty() || text.encode_utf16().count() > 4096 {
                        return Err(Error::bad("message must be nonempty and fit Telegram's 4096 UTF-16 unit limit including the Agent header"));
                    }
                    let quote = if let Some(quote) = body["quote"].as_str() {
                        let id = parse_msg(quote)?;
                        tx.message_in(topic_id, id)?;
                        Some(id)
                    } else { None };
                    let id = tx.append(topic_id, Some(&agent), None, content, now(), quote)?;
                    tx.enqueue("send", Some(id), topic.chat, Delivery::Send { text, thread: Some(topic.thread), quote: None })?;
                    json!({"msg_id":msg_id(id)})
                }
                _ => return Err(Error::bad("unknown command")),
            };
            if write { tx.audit(&route, json!({"agent":agent,"topic":topic_id}))?; }
            Ok(result)
        }).await
    }
}
