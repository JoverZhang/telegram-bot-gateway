use super::*;
use crate::{
    model::{Delivery, Job, now},
    telegram::Failure,
};
impl Gateway {
    pub async fn prepare_delivery(&self) -> Result<Option<Job>> {
        self.db
            .run(true, |tx| {
                if tx
                    .meta("telegram_blocked_until")?
                    .and_then(|s| s.parse::<i64>().ok())
                    .unwrap_or(0)
                    > now()
                {
                    return Ok(None);
                }
                for mut job in tx.due_jobs()? {
                    if let Some(message) = job.message {
                        let (topic, quote) = tx.delivery_context(message)?;
                        match &mut job.payload {
                            Delivery::Send { quote: target, .. } => {
                                let reason = if !topic.available || topic.closed {
                                    Some("Topic closed or Group unavailable")
                                } else if tx.earlier_send(&topic.id, job.id)? {
                                    Some("waiting for earlier send in Topic")
                                } else {
                                    None
                                };
                                if let Some(reason) = reason {
                                    tx.defer(job.id, reason, now() + 1_000)?;
                                    continue;
                                }
                                if let Some(quote) = quote {
                                    *target = tx.platform_message(quote, job.chat)?;
                                    if target.is_none() {
                                        tx.defer(
                                            job.id,
                                            "waiting for quoted message delivery",
                                            now() + 1_000,
                                        )?;
                                        continue;
                                    }
                                }
                            }
                            Delivery::Heart { target } => {
                                *target = tx.platform_message(message, job.chat)?;
                                if target.is_none() {
                                    tx.defer(
                                        job.id,
                                        "waiting for message delivery before receipt",
                                        now() + 1_000,
                                    )?;
                                    continue;
                                }
                            }
                        }
                    }
                    tx.claim(job.id)?;
                    return Ok(Some(job));
                }
                Ok(None)
            })
            .await
    }
    pub async fn complete_delivery(
        &self,
        job: Job,
        result: std::result::Result<Option<i64>, Failure>,
    ) -> Result<()> {
        self.db
            .run(true, move |tx| match result {
                Ok(platform_id) => {
                    if let (Some(message), Some(platform_id)) = (job.message, platform_id) {
                        tx.map(job.chat, platform_id, message)?;
                    }
                    tx.completed(&job)
                }
                Err(e) => {
                    let blocked = (400..500).contains(&e.code) && e.code != 429;
                    let delay = if blocked {
                        60
                    } else {
                        e.retry_after
                            .unwrap_or_else(|| 1_i64 << job.attempts.min(8))
                    };
                    eprintln!(
                        "delivery {} (message {:?}) failed: {}; retry in {delay}s",
                        job.id, job.message, e.detail
                    );
                    tx.failed(&job, &e.detail, delay.max(1), blocked, e.code == 429)
                }
            })
            .await
    }
}
