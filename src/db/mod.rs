mod queries;
use crate::model::{Error, Result};
pub(crate) use queries::Tx;
use rusqlite::Connection;
use tokio::sync::{mpsc, oneshot, watch};
type Work = Box<dyn FnOnce(&mut Connection) + Send>;
#[derive(Clone)]
pub(crate) struct Database {
    sender: mpsc::Sender<Work>,
    pub changes: watch::Sender<u64>,
}
impl Database {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;",
        )?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 1 {
            return Err(Error::internal(
                "database schema is newer than this Gateway",
            ));
        }
        if version == 0 {
            let t = conn.transaction()?;
            t.execute_batch(include_str!("../../migrations/0001_initial.sql"))?;
            t.commit()?;
        }
        conn.execute(
            "UPDATE outbox SET status='pending' WHERE status='inflight'",
            [],
        )?;
        conn.execute(
            "UPDATE outbox SET next_attempt=0 WHERE status='blocked'",
            [],
        )?;
        let (sender, mut receiver) = mpsc::channel::<Work>(128);
        let (changes, _) = watch::channel(0);
        std::thread::Builder::new()
            .name("tbg-db".into())
            .spawn(move || {
                while let Some(f) = receiver.blocking_recv() {
                    f(&mut conn)
                }
            })
            .map_err(|_| Error::internal("cannot start database thread"))?;
        Ok(Self { sender, changes })
    }
    pub fn alive(&self) -> bool {
        !self.sender.is_closed()
    }
    pub async fn run<R, F>(&self, write: bool, f: F) -> Result<R>
    where
        R: Send + 'static,
        F: FnOnce(&Tx<'_>) -> Result<R> + Send + 'static,
    {
        let (sender, receiver) = oneshot::channel();
        let change = self.changes.clone();
        self.sender
            .send(Box::new(move |c| {
                let before = c.total_changes();
                let result = (|| {
                    let t = c.transaction_with_behavior(if write {
                        rusqlite::TransactionBehavior::Immediate
                    } else {
                        rusqlite::TransactionBehavior::Deferred
                    })?;
                    let v = f(&Tx::new(&t))?;
                    t.commit()?;
                    Ok(v)
                })();
                if write && result.is_ok() && c.total_changes() != before {
                    change.send_modify(|v| *v = v.wrapping_add(1));
                }
                let _ = sender.send(result);
            }))
            .await
            .map_err(|_| Error::internal("database worker unavailable"))?;
        receiver
            .await
            .map_err(|_| Error::internal("database worker unavailable"))?
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        if matches!(&e, rusqlite::Error::SqliteFailure(err,_) if err.code==rusqlite::ErrorCode::ConstraintViolation)
        {
            Self::conflict("conflicting name or record")
        } else {
            eprintln!("database operation failed: {e}");
            Self::internal("database operation failed")
        }
    }
}
