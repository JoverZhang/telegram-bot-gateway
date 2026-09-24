use super::*;
use crate::{contract::ReadyTopic, model::msg_id};
struct Slot {
    name: String,
    slots: Arc<Mutex<HashSet<String>>>,
}
impl Drop for Slot {
    fn drop(&mut self) {
        self.slots.lock().unwrap().remove(&self.name);
    }
}
fn mentioned(content: &str, name: &str) -> bool {
    content.match_indices('@').any(|(n, _)| {
        let before = content[..n].chars().next_back();
        if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            return false;
        }
        let s = &content[n + 1..];
        s.strip_prefix(name).is_some_and(|rest| {
            !rest
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        })
    })
}
impl Gateway {
    pub(super) async fn wait(
        &self,
        a: String,
        topic: Option<String>,
        timeout: Option<u64>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<Response> {
        if !self.waits.lock().unwrap().insert(a.clone()) {
            return Err(Error::conflict("This Agent already has an active wait."));
        }
        let _slot = Slot {
            name: a.clone(),
            slots: self.waits.clone(),
        };
        let mut change = self.db.changes.subscribe();
        let deadline = timeout
            .map(|n| {
                tokio::time::Instant::now()
                    .checked_add(std::time::Duration::from_secs(n))
                    .ok_or_else(|| Error::bad("timeout is too large"))
            })
            .transpose()?;
        loop {
            change.borrow_and_update();
            let agent = a.clone();
            let topic = topic.clone();
            let topics = self
                .db
                .run(false, move |tx| {
                    let subscriptions = if let Some(topic) = topic {
                        tx.sub(&agent, &topic)?;
                        vec![topic]
                    } else {
                        tx.subs(&agent)?
                            .into_iter()
                            .map(|subscription| subscription.topic)
                            .collect()
                    };
                    if subscriptions.is_empty() {
                        return Err(Error::bad("no subscribed Topics to wait on"));
                    }
                    let mut ready = vec![];
                    for subscription in subscriptions {
                        let topic = subscription.as_str();
                        let (acked, muted) = tx.sub(&agent, topic)?;
                        let messages = tx.pending(&agent, topic, acked)?;
                        if let Some((id, _)) = messages
                            .iter()
                            .find(|(_, text)| !muted || mentioned(text, &agent))
                        {
                            ready.push(ReadyTopic {
                                topic: topic.into(),
                                trigger_msg_id: msg_id(*id),
                                pending_count: messages.len(),
                            });
                        }
                    }
                    Ok(ready)
                })
                .await?;
            if !topics.is_empty() {
                return Ok(Response::Wait { topics });
            }
            if deadline.is_some_and(|d| d <= tokio::time::Instant::now()) {
                return Ok(Response::Wait { topics: vec![] });
            }
            tokio::select! {
                _=cancel.cancelled()=>return Err(Error::bad("wait cancelled")),
                r=change.changed()=>{if r.is_err(){return Err(Error::internal("database notifications stopped"))}},
                _=async {if let Some(d)=deadline{tokio::time::sleep_until(d).await}else{std::future::pending::<()>().await}}=>return Ok(Response::Wait { topics: vec![] }),
            }
        }
    }
}
