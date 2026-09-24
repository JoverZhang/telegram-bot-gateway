mod connection;
use crate::{
    gateway::Gateway,
    model::{Error, ErrorKind},
};
use axum::{
    Extension, Router,
    extract::{Json, State, rejection::JsonRejection},
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
};
pub(crate) use connection::serve;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
pub(crate) fn router(gateway: Gateway) -> Router {
    Router::new()
        .fallback(axum::routing::post(handle).fallback(|| async {
            (
                StatusCode::METHOD_NOT_ALLOWED,
                Json(json!({"error":"use POST"})),
            )
        }))
        .with_state(gateway)
}
async fn handle(
    State(g): State<Gateway>,
    Extension(cancel): Extension<CancellationToken>,
    uri: Uri,
    body: std::result::Result<Json<Value>, JsonRejection>,
) -> Response {
    let result = match (uri.path().strip_prefix("/v1/"), body) {
        (Some(route), Ok(Json(b))) => g.execute(route.into(), b, cancel).await,
        (None, _) => Err(Error {
            kind: ErrorKind::NotFound,
            message: "unknown path".into(),
        }),
        (_, Err(_)) => Err(Error::bad("invalid JSON request")),
    };
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => (
            match e.kind {
                ErrorKind::Invalid => StatusCode::BAD_REQUEST,
                ErrorKind::Conflict => StatusCode::CONFLICT,
                ErrorKind::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
                ErrorKind::NotFound => StatusCode::NOT_FOUND,
            },
            Json(json!({"error":e.message})),
        )
            .into_response(),
    }
}
