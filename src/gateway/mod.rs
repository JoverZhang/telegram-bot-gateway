mod communication;
mod delivery;
mod ingest;
mod management;
mod wait;
use crate::{
    db::Database,
    model::{Error, Result},
    telegram::TelegramClient,
};
use serde_json::{Value, json};
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
        route: String,
        b: Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<Value> {
        crate::contract::validate(&route, &b).map_err(Error::bad)?;
        if route == "agent/register" {
            let name = b["name"].as_str().map(str::to_owned).unwrap_or_else(|| {
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
                    Ok(json!({"name":name}))
                })
                .await;
        }
        let agent = b["agent"].as_str().unwrap().to_owned();
        let a = agent.clone();
        self.db.run(false, move |tx| tx.require_agent(&a)).await?;
        let actor = agent.clone();
        let result = if route == "wait" {
            self.wait(agent, b, cancel).await
        } else if route.starts_with("topic/") || route == "agent/list" || route == "group/list" {
            self.manage(&route, agent, b).await
        } else {
            self.communication(route, agent, b).await
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
