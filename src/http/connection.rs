use axum::{Router, body::Body};
use hyper::{server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use tokio::{
    net::{TcpListener, TcpStream},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

pub(crate) async fn serve(
    listener: TcpListener,
    app: Router,
    stop: CancellationToken,
) -> std::io::Result<()> {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
           _=stop.cancelled()=>break,
           result=listener.accept()=>{
               let(stream,_)=result?;let socket=stream.into_std()?;let peek=TcpStream::from_std(socket.try_clone()?)?;let stream=TcpStream::from_std(socket)?;
               let router=app.clone();let cancelled=stop.child_token();
               connections.spawn(async move{
                   let monitor_token=cancelled.clone();
                   let monitor=tokio::spawn(async move{
                       // Readiness alone does not identify EOF while a request is pending.
                       // Peek never consumes HTTP bytes; the duplicated descriptor shares the socket.
                       let mut byte=[0];loop{tokio::select!{
                           _=monitor_token.cancelled()=>break,
                           r=peek.peek(&mut byte)=>match r{Ok(0)|Err(_)=>{monitor_token.cancel();break},Ok(_)=>tokio::time::sleep(std::time::Duration::from_millis(25)).await}
                       }}
                   });
                   let service_token=cancelled.clone();
                   let service=service_fn(move|req:hyper::Request<hyper::body::Incoming>|{
                       let router=router.clone();let mut req=req.map(Body::new);req.extensions_mut().insert(service_token.clone());
                       async move{router.oneshot(req).await}
                   });
                   tokio::select!{_ = cancelled.cancelled()=>{},_ = http1::Builder::new().serve_connection(TokioIo::new(stream),service)=>{}}
                   cancelled.cancel();let _=monitor.await;
               });
           },
           Some(_)=connections.join_next(),if !connections.is_empty()=>{},
        }
    }
    connections.abort_all();
    while connections.join_next().await.is_some() {}
    Ok(())
}
