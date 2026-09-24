use crate::gateway::Gateway;
use tokio_util::sync::CancellationToken;
pub(super) async fn run(
    g: Gateway,
    stop: CancellationToken,
    ready: tokio::sync::watch::Sender<bool>,
) -> Result<(), String> {
    let mut initialized = false;
    let mut offset = None;
    let mut last_update = std::time::Instant::now();
    loop {
        if !g.db.alive() {
            return Err("database worker stopped".into());
        }
        if !initialized {
            let result = tokio::select! { _=stop.cancelled()=>return Ok(()), result=g.telegram.identity()=>result };
            match result {
                Ok(id) => {
                    let id = id.to_string();
                    g.db.run(true, move |tx| {
                        if tx.meta("bot_id")?.is_some_and(|old| old != id) {
                            return Err(crate::model::Error::conflict(
                                "data directory belongs to another Bot",
                            ));
                        }
                        tx.set_meta("bot_id", &id)
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                    initialized = true;
                    let _ = ready.send(true);
                }
                Err(e) => eprintln!("Telegram initialization unavailable: {}", e.detail),
            }
        } else {
            // Telegram may choose a new random update_id after a week without updates.
            if last_update.elapsed() >= std::time::Duration::from_secs(7 * 24 * 3600) {
                offset = None;
            }
            let batch =
                tokio::select! {_=stop.cancelled()=>return Ok(()),v=g.telegram.updates(offset)=>v};
            match batch {
                Ok(updates) => {
                    if !updates.is_empty() {
                        last_update = std::time::Instant::now();
                    }
                    for update in updates {
                        if stop.is_cancelled() {
                            return Ok(());
                        }
                        g.ingest(update).await.map_err(|e| e.to_string())?;
                    }
                    offset =
                        g.db.run(false, |tx| {
                            Ok(tx.meta("offset")?.and_then(|s| s.parse::<i64>().ok()))
                        })
                        .await
                        .map_err(|e| e.to_string())?;
                }
                Err(e) => {
                    eprintln!("Telegram polling unavailable: {}", e.detail);
                    let delay = e.retry_after.unwrap_or(2).max(1) as u64;
                    tokio::select! {_=stop.cancelled()=>return Ok(()),_=tokio::time::sleep(std::time::Duration::from_secs(delay))=>{}}
                }
            }
        }
        tokio::select! {_=stop.cancelled()=>return Ok(()),_=tokio::time::sleep(std::time::Duration::from_millis(if initialized{100}else{1000}))=>{}}
    }
}
