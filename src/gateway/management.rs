use super::*;
use crate::{
    contract::{Group, Topic as TopicCommand},
    model::now,
};
impl Gateway {
    pub(super) async fn manage(&self, command: Command, agent: String) -> Result<Response> {
        match command {
            Command::Agent(Agent::List) => {
                let waits = self.waits.lock().unwrap().clone();
                self.db
                    .run(false, move |tx| {
                        let agents = tx
                            .agents()?
                            .into_iter()
                            .map(|(name, seen)| {
                                let waiting = waits.contains(&name);
                                crate::contract::AgentStatus {
                                    name,
                                    active: waiting || seen > now() - 300_000,
                                    waiting,
                                }
                            })
                            .collect();
                        Ok(Response::Agents { agents })
                    })
                    .await
            }
            Command::Group(Group::List) => {
                self.db
                    .run(false, |tx| {
                        Ok(Response::Groups {
                            groups: tx.groups()?,
                        })
                    })
                    .await
            }
            Command::Topic(TopicCommand::List { group }) => {
                let chat = group
                    .as_deref()
                    .map(|s| s.parse::<i64>().map_err(|_| Error::bad("invalid group")))
                    .transpose()?;
                self.db
                    .run(false, move |tx| {
                        Ok(Response::Topics {
                            topics: tx.topics(chat)?,
                        })
                    })
                    .await
            }
            Command::Topic(TopicCommand::Show { topic: id }) => {
                self.db
                    .run(false, move |tx| {
                        let t = tx.topic(&id)?;
                        Ok(Response::TopicDetail {
                            topic: t.id,
                            name: t.name,
                            group: t.chat.to_string(),
                            status: if t.closed { "closed" } else { "active" }.into(),
                            subscribers: tx.subscribers(&id)?,
                        })
                    })
                    .await
            }
            Command::Topic(TopicCommand::Create { group, name }) => {
                let chat = group
                    .parse::<i64>()
                    .map_err(|_| Error::bad("invalid group"))?;
                if name.is_empty() || name.chars().count() > 128 {
                    return Err(Error::bad("Topic name must contain 1–128 characters"));
                }
                let intent = name.clone();
                let actor = agent.clone();
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
                let thread = match self.telegram.create_topic(chat, &name).await {
                    Ok(thread) => thread,
                    Err(error) => {
                        self.management_failure(
                            "topic/create result",
                            json!({"group":chat,"name":name,"agent":agent}),
                            &error,
                        )
                        .await?;
                        return Err(Error::internal(
                            "Topic creation failed or outcome unknown; inspect Telegram before retrying",
                        ));
                    }
                };
                self.db
                    .run(true, move |tx| {
                        let id = tx.ensure_topic(chat, thread, &name, Some(&agent))?;
                        tx.audit("topic/create result", json!({"topic":id}))?;
                        Ok(Response::TopicCreated {
                            topic: id,
                            name,
                            status: "active".into(),
                        })
                    })
                    .await
            }
            Command::Topic(TopicCommand::Close { topic }) => {
                self.change_topic(agent, topic, true).await
            }
            Command::Topic(TopicCommand::Reopen { topic }) => {
                self.change_topic(agent, topic, false).await
            }
            _ => Err(Error::bad("unsupported management command")),
        }
    }
    async fn change_topic(&self, agent: String, id: String, close: bool) -> Result<Response> {
        let requested = id.clone();
        let t = self
            .db
            .run(true, move |tx| {
                let t = tx.topic(&requested)?;
                if t.owner.as_deref() != Some(&agent) {
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
        if let Err(error) = self.telegram.topic_state(t.chat, t.thread, close).await {
            self.management_failure(
                "topic/state result",
                json!({"topic":id,"closed":close}),
                &error,
            )
            .await?;
            return Err(Error::internal(
                "Topic state change failed or outcome unknown; inspect Telegram before retrying",
            ));
        }
        self.db
            .run(true, move |tx| {
                tx.close(&id, close)?;
                tx.audit("topic/state result", json!({"topic":id,"closed":close}))?;
                Ok(Response::TopicState {
                    topic: id,
                    status: if close { "closed" } else { "active" }.into(),
                })
            })
            .await
    }
    async fn management_failure(
        &self,
        operation: &'static str,
        mut context: serde_json::Value,
        error: &crate::telegram::Failure,
    ) -> Result<()> {
        eprintln!("{operation} failed: {}", error.detail);
        context["error"] = json!(error.detail);
        context["outcome_unknown"] = json!(error.code == 0 || error.code >= 500);
        self.db
            .run(true, move |tx| tx.audit(operation, context))
            .await
    }
}
