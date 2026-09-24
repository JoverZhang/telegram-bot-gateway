use super::*;
use crate::{
    bot,
    model::Delivery,
    telegram::{Event, TopicEvent, Update},
};

impl Gateway {
    pub async fn ingest(&self, update: Update) -> Result<()> {
        let admins = self.admins.clone();
        self.db.run(true, move |tx| {
            // Ignored ordinary messages retain receipt metadata, never their bodies.
            if !tx.record_update(update.id, None)? {
                tx.set_meta("offset", &update.id.saturating_add(1).to_string())?;
                return Ok(());
            }
            match update.event {
                Event::Membership { chat, title, actor, present } => {
                    if present { tx.group(chat, &title, admins.contains(&actor), true)?; }
                    else { tx.disconnect(chat)?; }
                    tx.audit("membership", json!({"chat":chat,"actor":actor,"present":present}))?;
                }
                Event::Message(message) => {
                    let admin = admins.contains(&message.user);
                    let trusted = admin || tx.trusted(message.user)?;
                    if message.content.starts_with('/') {
                        let command = bot::command(&message.content);
                        if command == "/manage" && admin && message.group {
                            tx.group(message.chat, &message.title, true, true)?;
                        }
                        if matches!(command, "/help" | "/whoami") || trusted {
                            tx.audit("command", json!({"user":message.user,"chat":message.chat,"command":message.content}))?;
                            let text = bot::reply(command, bot::Context {
                                user: message.user, admin, trusted, initialized: !admins.is_empty(),
                                group: message.group, topic: message.thread, zh: message.language.starts_with("zh"),
                            });
                            tx.enqueue("management", None, message.chat, Delivery::Send { text, thread: message.thread, quote: None })?;
                        }
                    } else if tx.connected(message.chat)? && let Some(thread) = message.thread.filter(|thread| *thread != 1) {
                        if let Some(event) = message.topic_event {
                            let name = if let TopicEvent::Created(ref name) = event { name.as_str() } else { "" };
                            let topic = tx.ensure_topic(message.chat, thread, name, None)?;
                            match event {
                                TopicEvent::Closed => tx.close(&topic, true)?,
                                TopicEvent::Reopened => tx.close(&topic, false)?,
                                TopicEvent::Created(_) => (),
                            }
                            tx.audit("topic state", json!({"topic":topic}))?;
                        } else if trusted {
                            let topic = tx.ensure_topic(message.chat, thread, "", None)?;
                            let quote = message.quote.map(|id| tx.mapped(message.chat, id)).transpose()?.flatten()
                                .filter(|id| tx.message_in(&topic, *id).is_ok());
                            let id = tx.append(&topic, None, Some(message.user), &message.content, message.date, quote)?;
                            tx.map(message.chat, message.id, id)?;
                            tx.save_raw_update(update.id, &update.raw)?;
                        }
                    }
                }
                Event::Other => (),
            }
            tx.set_meta("offset", &update.id.saturating_add(1).to_string())?;
            Ok(())
        }).await
    }
}
