use super::*;
use crate::model::{Delivery, msg_id, now, parse_msg};

impl Gateway {
    pub(super) async fn communication(&self, command: Command, agent: String) -> Result<Response> {
        let write = !matches!(
            command,
            Command::Whoami
                | Command::Subscriptions
                | Command::History { .. }
                | Command::Unread { .. }
        );
        self.db.run(write, move |tx| {
            let result = match command {
                Command::Whoami => Response::Identity { name: agent.clone() },
                Command::Subscriptions => Response::Subscriptions { subscriptions: tx.subs(&agent)? },
                Command::Subscribe { topic, muted } => {
                    tx.subscribe(&agent, &topic, muted)?;
                    Response::Topic { topic }
                }
                Command::Unsubscribe { topic } => {
                    tx.unsubscribe(&agent, &topic)?;
                    Response::Topic { topic }
                }
                Command::Mute { topic, off } => {
                    tx.mute(&agent, &topic, !off)?;
                    Response::Muted { topic, muted: !off }
                }
                Command::Ack { topic, through } => {
                    let through = parse_msg(&through)?;
                    let (old, _) = tx.sub(&agent, &topic)?;
                    tx.message_in(&topic, through)?;
                    let boundary = old.unwrap_or(0).max(through);
                    if old != Some(boundary) {
                        let destination = tx.topic(&topic)?;
                        for (id, _) in tx.pending(&agent, &topic, old)? {
                            if id > through { break; }
                            tx.enqueue("reaction", Some(id), destination.chat, Delivery::Heart { target: None })?;
                        }
                        tx.set_ack(&agent, &topic, boundary)?;
                    }
                    Response::Ack { last_acked_msg_id: msg_id(boundary) }
                }
                Command::Unread { topic, cursor, limit } => Self::read(tx, &agent, &topic, cursor, limit, true)?,
                Command::History { topic, cursor, limit } => Self::read(tx, &agent, &topic, cursor, limit, false)?,
                Command::Send { topic, content, quote } => {
                    let destination = tx.topic(&topic)?;
                    if destination.closed || !destination.available { return Err(Error::conflict("Topic is closed or Group is unavailable")); }
                    let text = crate::bot::conversation(&agent, &content);
                    if content.is_empty() || text.encode_utf16().count() > 4096 {
                        return Err(Error::bad("message must be nonempty and fit Telegram's 4096 UTF-16 unit limit including the Agent header"));
                    }
                    let quote = if let Some(quote) = quote {
                        let id = parse_msg(&quote)?;
                        tx.message_in(&topic, id)?;
                        Some(id)
                    } else { None };
                    let id = tx.append(&topic, Some(&agent), None, &content, now(), quote)?;
                    tx.enqueue("send", Some(id), destination.chat, Delivery::Send { text, thread: Some(destination.thread), quote: None })?;
                    Response::Sent { msg_id: msg_id(id) }
                }
                _ => return Err(Error::bad("unsupported communication command")),
            };
            if write { tx.audit("agent operation", json!({"agent":agent,"result":result}))?; }
            Ok(result)
        }).await
    }
    fn read(
        tx: &crate::db::Tx<'_>,
        agent: &str,
        topic: &str,
        cursor: Option<String>,
        limit: Option<u32>,
        forward: bool,
    ) -> Result<Response> {
        tx.topic(topic)?;
        let base = if forward {
            tx.sub(agent, topic)?.0
        } else {
            None
        };
        let cursor = if let Some(cursor) = cursor {
            let id = parse_msg(&cursor)?;
            tx.message_in(topic, id)?;
            Some(id)
        } else {
            base
        };
        let (messages, remaining) =
            tx.messages(topic, cursor, forward, i64::from(limit.unwrap_or(20)))?;
        let next = messages
            .last()
            .map(|message| message.msg_id.clone())
            .or_else(|| cursor.map(msg_id));
        Ok(Response::Page {
            messages,
            next_cursor: next,
            remaining_count: remaining,
        })
    }
}
