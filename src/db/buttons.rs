use super::Tx;
use crate::{
    contract::{CallbackQuery, InlineKeyboardMarkup, Response},
    model::{Error, Result, msg_id, now, timestamp},
    telegram::Callback,
};
use rusqlite::{OptionalExtension, params};

pub(crate) struct EditTarget {
    pub chat: i64,
    pub platform_id: i64,
    pub keyboard: Option<InlineKeyboardMarkup>,
}

impl Tx<'_> {
    pub fn keyboard(&self, message: i64) -> Result<Option<InlineKeyboardMarkup>> {
        let raw: Option<String> = self
            .c
            .query_row(
                "SELECT markup FROM message_keyboards WHERE message=?",
                [message],
                |r| r.get(0),
            )
            .optional()?;
        raw.map(|raw| {
            serde_json::from_str(&raw).map_err(|_| Error::internal("invalid stored keyboard"))
        })
        .transpose()
    }

    pub fn save_keyboard(&self, message: i64, markup: &InlineKeyboardMarkup) -> Result<()> {
        let raw = serde_json::to_string(markup).map_err(|_| Error::internal("invalid keyboard"))?;
        self.c.execute("INSERT INTO message_keyboards VALUES(?,?) ON CONFLICT(message) DO UPDATE SET markup=excluded.markup", params![message, raw])?;
        Ok(())
    }

    pub fn edit_target(&self, agent: &str, topic: &str, message: i64) -> Result<EditTarget> {
        let destination = self.topic(topic)?;
        if destination.closed || !destination.available {
            return Err(Error::conflict("Topic is closed or Group is unavailable"));
        }
        self.message_in(topic, message)?;
        let owner: Option<String> =
            self.c
                .query_row("SELECT agent FROM messages WHERE id=?", [message], |r| {
                    r.get(0)
                })?;
        if owner.as_deref() != Some(agent) {
            return Err(Error::bad("only the sending Agent can edit this message"));
        }
        let platform_id = self
            .platform_message(message, destination.chat)?
            .ok_or_else(|| Error::conflict("message has not been delivered yet"))?;
        Ok(EditTarget {
            chat: destination.chat,
            platform_id,
            keyboard: self.keyboard(message)?,
        })
    }

    pub fn edit_content(&self, message: i64, content: &str) -> Result<()> {
        self.c.execute(
            "UPDATE messages SET content=? WHERE id=?",
            params![content, message],
        )?;
        Ok(())
    }

    pub fn record_callback(&self, callback: &Callback) -> Result<bool> {
        if !self.connected(callback.chat)? {
            return Ok(false);
        }
        let target: Option<(i64, String, String)> = self.c.query_row(
            "SELECT m.id,m.agent,m.topic FROM telegram_messages tm JOIN messages m ON m.id=tm.message WHERE tm.chat=? AND tm.telegram_id=? AND m.agent IS NOT NULL",
            params![callback.chat, callback.message], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).optional()?;
        let Some((message, agent, topic)) = target else {
            return Ok(false);
        };
        if !self
            .keyboard(message)?
            .is_some_and(|markup| markup.contains_callback(&callback.data))
        {
            return Ok(false);
        }
        Ok(self.c.execute(
            "INSERT OR IGNORE INTO callbacks(query_id,agent,topic,message,user,data,received_at) VALUES(?,?,?,?,?,?,?)",
            params![callback.id, agent, topic, message, callback.user, callback.data, now()],
        )? != 0)
    }

    pub fn require_callback(&self, agent: &str, query: &str) -> Result<()> {
        if !self.c.query_row(
            "SELECT EXISTS(SELECT 1 FROM callbacks WHERE agent=? AND query_id=?)",
            params![agent, query],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(Error::bad("unknown callback for this Agent"));
        }
        Ok(())
    }

    pub fn callbacks(
        &self,
        agent: &str,
        topic: Option<&str>,
        cursor: Option<String>,
        limit: u32,
    ) -> Result<Response> {
        if let Some(topic) = topic {
            self.topic(topic)?;
        }
        let boundary = if let Some(cursor) = &cursor {
            self.c.query_row("SELECT id FROM callbacks WHERE query_id=? AND agent=? AND (? IS NULL OR topic=?)", params![cursor, agent, topic, topic], |r| r.get::<_, i64>(0)).optional()?
                .ok_or_else(|| Error::bad("callback cursor does not belong to this Agent/Topic"))?
        } else {
            0
        };
        let mut statement = self.c.prepare("SELECT query_id,topic,message,user,data,received_at FROM callbacks WHERE agent=? AND (? IS NULL OR topic=?) AND id>? ORDER BY id LIMIT ?")?;
        let callbacks = statement
            .query_map(
                params![agent, topic, topic, boundary, i64::from(limit)],
                |r| {
                    Ok(CallbackQuery {
                        callback_query_id: r.get(0)?,
                        topic: r.get(1)?,
                        msg_id: msg_id(r.get(2)?),
                        user_id: r.get(3)?,
                        data: r.get(4)?,
                        received_at: timestamp(r.get(5)?),
                    })
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let total: i64 = self.c.query_row(
            "SELECT COUNT(*) FROM callbacks WHERE agent=? AND (? IS NULL OR topic=?) AND id>?",
            params![agent, topic, topic, boundary],
            |r| r.get(0),
        )?;
        let next_cursor = callbacks
            .last()
            .map(|callback| callback.callback_query_id.clone())
            .or(cursor);
        Ok(Response::CallbackPage {
            remaining_count: total - callbacks.len() as i64,
            callbacks,
            next_cursor,
        })
    }
}
