mod communication;
mod delivery;
mod ingest;
mod management;
mod wait;
use crate::contract::{Agent, Command, Request, Response};
use crate::{
    db::Database,
    model::{Error, Result},
    telegram::TelegramClient,
};
use serde_json::json;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};
#[derive(Clone)]
pub(crate) struct Gateway {
    pub db: Database,
    pub telegram: TelegramClient,
    pub admins: Arc<Vec<i64>>,
    waits: Arc<Mutex<HashSet<String>>>,
}
impl Gateway {
    pub fn new(db: Database, telegram: TelegramClient, admins: Vec<i64>) -> Self {
        Self {
            db,
            telegram,
            admins: Arc::new(admins),
            waits: Default::default(),
        }
    }
    pub async fn execute(
        &self,
        request: Request,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<Response> {
        if let Command::Agent(Agent::Register { name }) = request.command {
            let name = name.unwrap_or_else(|| {
                format!("agent_{}", &uuid::Uuid::new_v4().simple().to_string()[..12])
            });
            if !valid_name(&name) {
                return Err(Error::bad("name must match ^[A-Za-z][A-Za-z0-9_]*$"));
            }
            return self
                .db
                .run(true, move |tx| {
                    tx.register(&name)?;
                    tx.audit("register", json!({"agent":name}))?;
                    Ok(Response::Identity { name })
                })
                .await;
        }
        let agent = request
            .agent
            .ok_or_else(|| Error::bad("agent is required"))?;
        let a = agent.clone();
        self.db.run(false, move |tx| tx.require_agent(&a)).await?;
        let actor = agent.clone();
        let result = match request.command {
            Command::Wait { topic, timeout } => self.wait(agent, topic, timeout, cancel).await,
            command @ (Command::Topic(_) | Command::Agent(_) | Command::Group(_)) => {
                self.manage(command, agent).await
            }
            command => self.communication(command, agent).await,
        };
        if result.is_ok() {
            self.db.run(true, move |tx| tx.seen(&actor)).await?;
        }
        result
    }
}
fn valid_name(s: &str) -> bool {
    let mut c = s.bytes();
    c.next().is_some_and(|v| v.is_ascii_alphabetic())
        && c.all(|v| v.is_ascii_alphanumeric() || v == b'_')
}
