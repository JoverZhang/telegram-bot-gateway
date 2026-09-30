use super::*;
use crate::{
    contract::{Callback, Edit},
    model::{msg_id, parse_msg},
};
use std::sync::atomic::Ordering;

impl Gateway {
    pub fn mark_telegram_ready(&self) {
        self.telegram_ready.store(true, Ordering::Release);
    }

    pub(super) async fn buttons(&self, command: Command, agent: String) -> Result<Response> {
        if let Command::Callback(Callback::List {
            topic,
            cursor,
            limit,
        }) = command
        {
            return self
                .db
                .run(false, move |tx| {
                    tx.callbacks(&agent, topic.as_deref(), cursor, limit.unwrap_or(20))
                })
                .await;
        }
        if !self.telegram_ready.load(Ordering::Acquire) {
            return Err(Error::internal(
                "Telegram Bot identity has not been verified yet",
            ));
        }
        match command {
            Command::Callback(Callback::Answer {
                callback_query_id,
                text,
                show_alert,
            }) => {
                let query = callback_query_id.clone();
                self.db
                    .run(false, move |tx| tx.require_callback(&agent, &query))
                    .await?;
                self.telegram
                    .answer_callback(&callback_query_id, text, show_alert)
                    .await
                    .map_err(|error| {
                        Error::internal(format!(
                            "callback answer failed or outcome unknown: {}",
                            error.detail
                        ))
                    })?;
                Ok(Response::CallbackAnswered { callback_query_id })
            }
            Command::Edit(edit) => self.edit_message(agent, edit).await,
            _ => Err(Error::bad("unsupported button command")),
        }
    }

    async fn edit_message(&self, agent: String, edit: Edit) -> Result<Response> {
        // Serialize the Telegram call and its local commit. Without this boundary,
        // concurrent responses could reverse history relative to the visible page.
        let _edit = self.edits.lock().await;
        let (topic, requested) = match &edit {
            Edit::Text { topic, msg_id, .. } | Edit::Markup { topic, msg_id, .. } => {
                (topic.clone(), msg_id)
            }
        };
        let message = parse_msg(requested)?;
        let actor = agent.clone();
        let target = self
            .db
            .run(false, move |tx| tx.edit_target(&actor, &topic, message))
            .await?;
        let mut payload = json!({"chat_id":target.chat,"message_id":target.platform_id});
        let (method, content, markup) = match edit {
            Edit::Text {
                content,
                format,
                no_header,
                reply_markup,
                ..
            } => {
                let (text, parse_mode) =
                    crate::telegram::formatting::message(&agent, &content, format, no_header)?;
                payload["text"] = json!(text);
                if let Some(mode) = parse_mode {
                    payload["parse_mode"] = json!(mode);
                }
                (
                    "editMessageText",
                    Some(content),
                    reply_markup.or(target.keyboard),
                )
            }
            Edit::Markup { reply_markup, .. } => {
                ("editMessageReplyMarkup", None, Some(reply_markup))
            }
        };
        if let Some(markup) = &markup {
            payload["reply_markup"] = json!(markup);
        }
        self.telegram.edit(method, payload).await.map_err(|error| {
            Error::internal(format!(
                "message edit failed or outcome unknown: {}",
                error.detail
            ))
        })?;
        self.db
            .run(true, move |tx| {
                if let Some(content) = content {
                    tx.edit_content(message, &content)?;
                }
                if let Some(markup) = markup {
                    tx.save_keyboard(message, &markup)?;
                }
                tx.audit(
                    "message edit",
                    json!({"agent":agent,"msg_id":msg_id(message),"method":method}),
                )?;
                Ok(Response::Sent {
                    msg_id: msg_id(message),
                })
            })
            .await
    }
}
