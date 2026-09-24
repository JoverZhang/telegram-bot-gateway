use crate::model::{Delivery, Error, Job, Message, Result, Topic, msg_id, now, timestamp};
use crate::model::{GroupInfo, Sender, Subscription, TopicInfo};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
pub(crate) struct Tx<'a> {
    c: &'a Connection,
}
impl<'a> Tx<'a> {
    pub(super) fn new(c: &'a Connection) -> Self {
        Self { c }
    }
    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .c
            .query_row("SELECT value FROM meta WHERE key=?", [key], |r| r.get(0))
            .optional()?)
    }
    pub fn set_meta(&self, k: &str, v: &str) -> Result<()> {
        self.c.execute(
            "INSERT INTO meta VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [k, v],
        )?;
        Ok(())
    }
    pub fn audit(&self, kind: &str, detail: Value) -> Result<()> {
        self.c.execute(
            "INSERT INTO management(time,kind,detail) VALUES(?,?,?)",
            params![now(), kind, detail.to_string()],
        )?;
        Ok(())
    }
    pub fn register(&self, n: &str) -> Result<()> {
        self.c
            .execute("INSERT INTO agents VALUES(?,?)", params![n, now()])?;
        Ok(())
    }
    pub fn require_agent(&self, name: &str) -> Result<()> {
        let exists: bool = self.c.query_row(
            "SELECT EXISTS(SELECT 1 FROM agents WHERE name=?)",
            [name],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(Error::bad("unknown Agent"));
        }
        Ok(())
    }
    pub fn seen(&self, n: &str) -> Result<()> {
        if self.c.execute(
            "UPDATE agents SET last_seen=? WHERE name=?",
            params![now(), n],
        )? == 0
        {
            return Err(Error::bad("unknown Agent"));
        }
        Ok(())
    }
    pub fn agents(&self) -> Result<Vec<(String, i64)>> {
        let mut q = self
            .c
            .prepare("SELECT name,last_seen FROM agents ORDER BY name")?;
        Ok(q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?)
    }
    pub fn group(&self, chat: i64, name: &str, connected: bool, available: bool) -> Result<()> {
        self.c.execute("INSERT INTO groups VALUES(?,?,?,?) ON CONFLICT(chat) DO UPDATE SET name=excluded.name,connected=MAX(groups.connected,excluded.connected),available=excluded.available",params![chat,name,connected,available])?;
        Ok(())
    }
    pub fn disconnect(&self, chat: i64) -> Result<()> {
        self.c.execute(
            "UPDATE groups SET connected=0,available=0 WHERE chat=?",
            [chat],
        )?;
        Ok(())
    }
    pub fn connected(&self, chat: i64) -> Result<bool> {
        Ok(self
            .c
            .query_row(
                "SELECT connected AND available FROM groups WHERE chat=?",
                [chat],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(false))
    }
    pub fn groups(&self) -> Result<Vec<GroupInfo>> {
        let mut q = self
            .c
            .prepare("SELECT chat,name,connected,available FROM groups ORDER BY chat")?;
        Ok(q.query_map([], |r| {
            Ok(GroupInfo {
                group: r.get::<_, i64>(0)?.to_string(),
                name: r.get(1)?,
                connected: r.get(2)?,
                available: r.get(3)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?)
    }
    pub fn topic(&self, id: &str) -> Result<Topic> {
        self.c.query_row("SELECT t.id,t.chat,t.thread,t.name,t.owner,t.closed,g.connected AND g.available FROM topics t JOIN groups g ON g.chat=t.chat WHERE t.id=?",[id],|r|Ok(Topic{id:r.get(0)?,chat:r.get(1)?,thread:r.get(2)?,name:r.get(3)?,owner:r.get(4)?,closed:r.get(5)?,available:r.get(6)?})).optional()?.ok_or_else(||Error::bad("unknown Topic"))
    }
    pub fn ensure_topic(
        &self,
        chat: i64,
        thread: i64,
        name: &str,
        owner: Option<&str>,
    ) -> Result<String> {
        let id = format!("t{}_{}", chat.unsigned_abs(), thread);
        self.c.execute("INSERT INTO topics(id,chat,thread,name,owner) VALUES(?,?,?,?,?) ON CONFLICT(chat,thread) DO UPDATE SET name=CASE WHEN excluded.name='' THEN topics.name ELSE excluded.name END,owner=COALESCE(topics.owner,excluded.owner)",params![id,chat,thread,name,owner])?;
        Ok(id)
    }
    pub fn topics(&self, chat: Option<i64>) -> Result<Vec<TopicInfo>> {
        let mut q = self.c.prepare(
            "SELECT id,name,chat,closed FROM topics WHERE (? IS NULL OR chat=?) ORDER BY id",
        )?;
        Ok(q.query_map(params![chat, chat], |r| {
            Ok(TopicInfo {
                topic: r.get(0)?,
                name: r.get(1)?,
                group: r.get::<_, i64>(2)?.to_string(),
                status: if r.get::<_, bool>(3)? {
                    "closed"
                } else {
                    "active"
                }
                .into(),
            })
        })?
        .collect::<std::result::Result<_, _>>()?)
    }
    pub fn close(&self, id: &str, closed: bool) -> Result<()> {
        self.c
            .execute("UPDATE topics SET closed=? WHERE id=?", params![closed, id])?;
        if !closed {
            self.c.execute(
                "UPDATE outbox SET next_attempt=0 WHERE status='blocked'",
                [],
            )?;
        }
        Ok(())
    }
    pub fn subscribe(&self, a: &str, t: &str, muted: bool) -> Result<()> {
        self.topic(t)?;
        self.c.execute("INSERT INTO subscriptions(agent,topic,active,muted,acked) VALUES(?,?,1,?,(SELECT MAX(id) FROM messages WHERE topic=?)) ON CONFLICT(agent,topic) DO UPDATE SET active=1,muted=CASE WHEN excluded.muted=1 THEN 1 ELSE subscriptions.muted END",params![a,t,muted,t])?;
        Ok(())
    }
    pub fn sub(&self, a: &str, t: &str) -> Result<(Option<i64>, bool)> {
        self.c
            .query_row(
                "SELECT acked,muted FROM subscriptions WHERE agent=? AND topic=? AND active=1",
                params![a, t],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| Error::bad("Topic is not currently subscribed"))
    }
    pub fn subs(&self, a: &str) -> Result<Vec<Subscription>> {
        let mut q = self.c.prepare(
            "SELECT topic,acked,muted FROM subscriptions WHERE agent=? AND active=1 ORDER BY topic",
        )?;
        Ok(q.query_map([a], |r| {
            Ok(Subscription {
                topic: r.get(0)?,
                last_acked_msg_id: r.get::<_, Option<i64>>(1)?.map(msg_id),
                muted: r.get(2)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?)
    }
    pub fn subscribers(&self, t: &str) -> Result<Vec<String>> {
        let mut q = self
            .c
            .prepare("SELECT agent FROM subscriptions WHERE topic=? AND active=1 ORDER BY agent")?;
        Ok(q.query_map([t], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?)
    }
    pub fn unsubscribe(&self, a: &str, t: &str) -> Result<()> {
        self.topic(t)?;
        self.c.execute(
            "UPDATE subscriptions SET active=0 WHERE agent=? AND topic=?",
            params![a, t],
        )?;
        Ok(())
    }
    pub fn mute(&self, a: &str, t: &str, m: bool) -> Result<()> {
        self.sub(a, t)?;
        self.c.execute(
            "UPDATE subscriptions SET muted=? WHERE agent=? AND topic=?",
            params![m, a, t],
        )?;
        Ok(())
    }
    pub fn message_in(&self, t: &str, n: i64) -> Result<()> {
        if !self.c.query_row(
            "SELECT EXISTS(SELECT 1 FROM messages WHERE id=? AND topic=?)",
            params![n, t],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(Error::bad("msg_id does not belong to this Topic"));
        }
        Ok(())
    }
    pub fn append(
        &self,
        t: &str,
        a: Option<&str>,
        u: Option<i64>,
        content: &str,
        sent: i64,
        quote: Option<i64>,
    ) -> Result<i64> {
        self.c.execute(
            "INSERT INTO messages(topic,agent,user,content,sent_at,quote) VALUES(?,?,?,?,?,?)",
            params![t, a, u, content, sent, quote],
        )?;
        Ok(self.c.last_insert_rowid())
    }
    pub fn map(&self, chat: i64, tid: i64, msg: i64) -> Result<()> {
        self.c.execute(
            "INSERT OR IGNORE INTO telegram_messages VALUES(?,?,?)",
            params![chat, tid, msg],
        )?;
        Ok(())
    }
    pub fn mapped(&self, chat: i64, tid: i64) -> Result<Option<i64>> {
        Ok(self
            .c
            .query_row(
                "SELECT message FROM telegram_messages WHERE chat=? AND telegram_id=?",
                params![chat, tid],
                |r| r.get(0),
            )
            .optional()?)
    }
    pub fn messages(
        &self,
        t: &str,
        cursor: Option<i64>,
        forward: bool,
        limit: i64,
    ) -> Result<(Vec<Message>, i64)> {
        let (cmp, order) = if forward { (">", "ASC") } else { ("<", "DESC") };
        let sql = format!(
            "SELECT id,agent,user,content,sent_at,quote FROM messages WHERE topic=? AND (? IS NULL OR id {cmp} ?) ORDER BY id {order} LIMIT ?"
        );
        let mut q = self.c.prepare(&sql)?;
        let rows = q
            .query_map(params![t, cursor, cursor, limit], |r| {
                let a: Option<String> = r.get(1)?;
                let u: Option<i64> = r.get(2)?;
                Ok(Message {
                    msg_id: msg_id(r.get(0)?),
                    sender: if let Some(a) = a {
                        Sender::Agent { name: a }
                    } else {
                        Sender::User {
                            user_id: u.unwrap_or(0),
                        }
                    },
                    content: r.get(3)?,
                    sent_at: timestamp(r.get(4)?),
                    quote_msg_id: r.get::<_, Option<i64>>(5)?.map(msg_id),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let total: i64 = self.c.query_row(
            &format!("SELECT COUNT(*) FROM messages WHERE topic=? AND (? IS NULL OR id {cmp} ?)"),
            params![t, cursor, cursor],
            |r| r.get(0),
        )?;
        let remaining = total - rows.len() as i64;
        Ok((rows, remaining))
    }
    pub fn set_ack(&self, agent: &str, topic: &str, through: i64) -> Result<()> {
        self.c.execute(
            "UPDATE subscriptions SET acked=? WHERE agent=? AND topic=?",
            params![through, agent, topic],
        )?;
        Ok(())
    }
    pub fn pending(&self, a: &str, t: &str, acked: Option<i64>) -> Result<Vec<(i64, String)>> {
        let mut q=self.c.prepare("SELECT id,content FROM messages WHERE topic=? AND id>? AND (agent IS NULL OR agent<>?) ORDER BY id")?;
        Ok(q.query_map(params![t, acked.unwrap_or(0), a], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?
        .collect::<std::result::Result<_, _>>()?)
    }
    pub fn trusted(&self, id: i64) -> Result<bool> {
        Ok(self
            .c
            .query_row("SELECT trusted FROM users WHERE id=?", [id], |r| r.get(0))
            .optional()?
            .unwrap_or(false))
    }
    pub fn record_update(&self, id: i64, raw: Option<&str>) -> Result<bool> {
        Ok(self.c.execute(
            "INSERT OR IGNORE INTO updates VALUES(?,?)",
            params![id, raw],
        )? != 0)
    }
    pub fn enqueue(&self, kind: &str, msg: Option<i64>, chat: i64, p: Delivery) -> Result<()> {
        self.c.execute(
            "INSERT OR IGNORE INTO outbox(kind,message,chat,payload) VALUES(?,?,?,?)",
            params![
                kind,
                msg,
                chat,
                serde_json::to_string(&p).map_err(|_| Error::internal("invalid delivery"))?
            ],
        )?;
        Ok(())
    }
    pub fn due_jobs(&self) -> Result<Vec<Job>> {
        let mut query = self.c.prepare("SELECT id,kind,message,chat,payload,attempts FROM outbox WHERE status IN ('pending','blocked') AND next_attempt<=? ORDER BY id")?;
        let rows = query.query_map([now()], |row| {
            let raw: String = row.get(4)?;
            let payload = serde_json::from_str(&raw).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    4,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok(Job {
                id: row.get(0)?,
                message: row.get(2)?,
                chat: row.get(3)?,
                payload,
                attempts: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
    pub fn delivery_context(&self, message: i64) -> Result<(Topic, Option<i64>)> {
        let (topic, quote): (String, Option<i64>) = self.c.query_row(
            "SELECT topic,quote FROM messages WHERE id=?",
            [message],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok((self.topic(&topic)?, quote))
    }
    pub fn earlier_send(&self, topic: &str, job: i64) -> Result<bool> {
        Ok(self.c.query_row("SELECT EXISTS(SELECT 1 FROM outbox o JOIN messages m ON m.id=o.message WHERE o.kind='send' AND o.status<>'done' AND m.topic=? AND o.id<?)", params![topic, job], |row| row.get(0))?)
    }
    pub fn platform_message(&self, message: i64, chat: i64) -> Result<Option<i64>> {
        Ok(self.c.query_row("SELECT telegram_id FROM telegram_messages WHERE message=? AND chat=? ORDER BY telegram_id LIMIT 1", params![message, chat], |row| row.get(0)).optional()?)
    }
    pub fn claim(&self, job: i64) -> Result<()> {
        self.c.execute(
            "UPDATE outbox SET status='inflight',attempts=attempts+1,error=NULL WHERE id=?",
            [job],
        )?;
        Ok(())
    }
    pub fn defer(&self, job: i64, reason: &str, next: i64) -> Result<bool> {
        let changed: bool = self.c.query_row(
            "SELECT error IS NULL OR error<>? FROM outbox WHERE id=?",
            params![reason, job],
            |row| row.get(0),
        )?;
        self.c.execute(
            "UPDATE outbox SET status='blocked',error=?,next_attempt=? WHERE id=?",
            params![reason, next, job],
        )?;
        Ok(changed)
    }
    pub fn completed(&self, job: &Job) -> Result<()> {
        self.c.execute(
            "UPDATE outbox SET status='done',error=NULL WHERE id=?",
            [job.id],
        )?;
        self.audit("delivery", json!({"job":job.id,"status":"done"}))
    }
    pub fn failed(
        &self,
        job: &Job,
        detail: &str,
        delay: i64,
        blocked: bool,
        rate: bool,
    ) -> Result<()> {
        let next = now().saturating_add(delay.saturating_mul(1000));
        self.c.execute(
            "UPDATE outbox SET status=?,next_attempt=?,error=? WHERE id=?",
            params![
                if blocked { "blocked" } else { "pending" },
                next,
                detail,
                job.id
            ],
        )?;
        if rate {
            self.set_meta("telegram_blocked_until", &next.to_string())?;
        }
        self.audit(
            "delivery",
            json!({"job":job.id,"status":"retry","error":detail}),
        )
    }
}
impl Tx<'_> {
    pub fn save_raw_update(&self, id: i64, raw: &str) -> Result<()> {
        self.c
            .execute("UPDATE updates SET raw=? WHERE id=?", params![raw, id])?;
        Ok(())
    }
}
