use crate::gateway::Gateway;
use tokio_util::sync::CancellationToken;
pub(super) async fn run(
    g: Gateway,
    stop: CancellationToken,
    mut ready: tokio::sync::watch::Receiver<bool>,
) -> Result<(), String> {
    // Never replay persisted deliveries through an unverified, possibly different Bot.
    while !*ready.borrow_and_update() {
        tokio::select! {
            _ = stop.cancelled() => return Ok(()),
            changed = ready.changed() => changed.map_err(|_| "Bot initialization stopped")?,
        }
    }
    loop {
        if stop.is_cancelled() {
            return Ok(());
        }
        if let Some(job) = g.prepare_delivery().await.map_err(|e| e.to_string())? {
            let result = tokio::select! {_=stop.cancelled()=>return Ok(()),r=g.telegram.deliver(job.chat, &job.payload)=>r};
            g.complete_delivery(job, result)
                .await
                .map_err(|e| e.to_string())?;
        } else {
            tokio::select! {_=stop.cancelled()=>return Ok(()),_=tokio::time::sleep(std::time::Duration::from_millis(100))=>{}}
        }
    }
}
