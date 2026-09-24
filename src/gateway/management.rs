use super::*;
use crate::model::now;
impl Gateway {
    pub(super) async fn manage(&self, route: &str, a: String, b: Value) -> Result<Value> {
        match route {
            "agent/list" => {
                let waits = self.waits.lock().unwrap().clone();
                self.db
                    .run(false, move |tx| {
                        let mut agents = tx.agents()?;
                        for v in &mut agents {
                            let waiting = waits.contains(v["name"].as_str().unwrap());
                            v["waiting"] = json!(waiting);
                            v["active"] = json!(
                                waiting || v["last_seen"].as_i64().unwrap() > now() - 300_000
                            );
                            v.as_object_mut().unwrap().remove("last_seen");
                        }
                        Ok(json!({"agents":agents}))
                    })
                    .await
            }
            "group/list" => {
                self.db
                    .run(false, |tx| Ok(json!({"groups":tx.groups()?})))
                    .await
            }
            "topic/list" => {
                let chat = b["group"]
                    .as_str()
                    .map(|s| s.parse::<i64>().map_err(|_| Error::bad("invalid group")))
                    .transpose()?;
                self.db
                    .run(false, move |tx| Ok(json!({"topics":tx.topics(chat)?})))
                    .await
            }
            "topic/show" => {
                let id = b["topic"].as_str().unwrap().to_owned();
                self.db.run(false,move|tx|{let t=tx.topic(&id)?;Ok(json!({"topic":t.id,"name":t.name,"group":t.chat.to_string(),"status":if t.closed{"closed"}else{"active"},"subscribers":tx.subscribers(&id)?}))}).await
            }
            "topic/create" => {
                let chat = b["group"]
                    .as_str()
                    .unwrap()
                    .parse::<i64>()
                    .map_err(|_| Error::bad("invalid group"))?;
                let name = b["name"].as_str().unwrap().to_owned();
                if name.is_empty() || name.chars().count() > 128 {
                    return Err(Error::bad("Topic name must contain 1–128 characters"));
                }
                let intent = name.clone();
                let actor = a.clone();
                self.db
                    .run(true, move |tx| {
                        if !tx.connected(chat)? {
                            return Err(Error::bad("Group is unavailable"));
                        }
                        tx.audit(
                            "topic/create intent",
                            json!({"agent":actor,"group":chat,"name":intent}),
                        )
                    })
                    .await?;
                let thread = self.telegram.create_topic(chat, &name).await.map_err(|error| {
                    eprintln!("Topic creation failed or outcome unknown: {}", error.detail);
                    Error::internal("Topic creation failed or outcome unknown; inspect Telegram before retrying")
                })?;
                self.db
                    .run(true, move |tx| {
                        let id = tx.ensure_topic(chat, thread, &name, Some(&a))?;
                        tx.audit("topic/create result", json!({"topic":id}))?;
                        Ok(json!({"topic":id,"name":name,"status":"active"}))
                    })
                    .await
            }
            "topic/close" | "topic/reopen" => {
                let id = b["topic"].as_str().unwrap().to_owned();
                let requested = id.clone();
                let close = route == "topic/close";
                let t = self
                    .db
                    .run(true, move |tx| {
                        let t = tx.topic(&requested)?;
                        if t.owner.as_deref() != Some(&a) {
                            return Err(Error::bad("only the Topic creator can manage this Topic"));
                        }
                        if !t.available {
                            return Err(Error::bad("Group is unavailable"));
                        }
                        tx.audit(
                            "topic/state intent",
                            json!({"topic":requested,"closed":close}),
                        )?;
                        Ok(t)
                    })
                    .await?;
                self.telegram
                    .topic_state(t.chat, t.thread, close)
                    .await
                    .map_err(|error| Error::internal(error.detail))?;
                self.db
                    .run(true, move |tx| {
                        tx.close(&id, close)?;
                        tx.audit("topic/state result", json!({"topic":id,"closed":close}))?;
                        Ok(json!({"topic":id,"status":if close{"closed"}else{"active"}}))
                    })
                    .await
            }
            _ => Err(Error::bad("unknown management command")),
        }
    }
}
