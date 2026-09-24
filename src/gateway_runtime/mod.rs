mod delivery;
mod receiver;
use crate::{config::ServerConfig, db::Database, gateway::Gateway, telegram::TelegramClient};
use std::fs::OpenOptions;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

pub async fn run() -> std::result::Result<(), String> {
    let cfg = ServerConfig::load()?;
    let data = cfg.data_path()?;
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(data.join("gateway.lock"))
        .map_err(|e| e.to_string())?;
    lock.try_lock()
        .map_err(|_| "another Gateway owns the data directory")?;
    let db = Database::open(&data.join("gateway.sqlite")).map_err(|e| e.to_string())?;
    let telegram = TelegramClient::new(cfg.telegram.bot_token)?;
    let gateway = Gateway::new(db, telegram, cfg.admins);
    let listener = tokio::net::TcpListener::bind(cfg.listen)
        .await
        .map_err(|e| format!("cannot listen on {}: {e}", cfg.listen))?;
    eprintln!("Gateway listening on {}", cfg.listen);
    let stop = CancellationToken::new();
    let mut tasks = JoinSet::new();
    let token = stop.clone();
    let app = crate::http::router(gateway.clone());
    tasks.spawn(async move {
        crate::http::serve(listener, app, token)
            .await
            .map_err(|e| e.to_string())
    });
    let (ready_sender, ready_receiver) = tokio::sync::watch::channel(false);
    let g = gateway.clone();
    let token = stop.clone();
    tasks.spawn(async move { receiver::run(g, token, ready_sender).await });
    let g = gateway.clone();
    let token = stop.clone();
    tasks.spawn(async move { delivery::run(g, token, ready_receiver).await });
    let failure = tokio::select! {
       _=shutdown_signal()=>None,
       result=tasks.join_next()=>Some(format!("Gateway task stopped: {result:?}")),
    };
    stop.cancel();
    // Workers await in-flight DB transactions; a cancelled network attempt stays in the outbox.
    while let Some(result) = tasks.join_next().await {
        if let Err(e) = result {
            eprintln!("shutdown task error: {e}")
        }
    }
    gateway
        .db
        .run(false, |_| Ok(()))
        .await
        .map_err(|e| e.to_string())?;
    drop(gateway);
    drop(lock);
    if let Some(e) = failure {
        Err(e)
    } else {
        Ok(())
    }
}
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}}
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}
