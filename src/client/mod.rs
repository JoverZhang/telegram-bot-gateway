use crate::contract::{Request, Response};
use serde_json::Value;
#[derive(Debug)]
pub enum ClientError {
    Transport {
        endpoint: String,
        detail: String,
    },
    Rejected {
        endpoint: String,
        status: u16,
        message: String,
    },
}
impl std::fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport { endpoint, detail } => write!(formatter, "{endpoint}: {detail}"),
            Self::Rejected {
                endpoint,
                status,
                message,
            } => write!(formatter, "{endpoint}: HTTP {status}: {message}"),
        }
    }
}
impl std::error::Error for ClientError {}
pub struct GatewayClient {
    http: reqwest::Client,
    endpoint: String,
}
impl GatewayClient {
    pub fn new(endpoint: String) -> std::result::Result<Self, String> {
        Ok(Self {
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(5))
                .build()
                .map_err(|e| e.to_string())?,
            endpoint,
        })
    }
    pub async fn execute(&self, request: &Request) -> std::result::Result<Response, ClientError> {
        let (route, body) = request.wire();
        let url = format!("{}/v1/{route}", self.endpoint);
        let mut req = self.http.post(&url).json(&body);
        if route != "wait" {
            req = req.timeout(std::time::Duration::from_secs(40))
        }
        let r = req.send().await.map_err(|error| ClientError::Transport {
            endpoint: url.clone(),
            detail: error.to_string(),
        })?;
        let status = r.status();
        let v: Value = r.json().await.map_err(|error| ClientError::Transport {
            endpoint: url.clone(),
            detail: format!("incomplete or invalid response: {error}"),
        })?;
        if !status.is_success() {
            return Err(ClientError::Rejected {
                endpoint: url,
                status: status.as_u16(),
                message: v["error"]
                    .as_str()
                    .unwrap_or("Gateway rejected request")
                    .into(),
            });
        }
        Response::decode(&request.command, v).map_err(|error| ClientError::Transport {
            endpoint: url,
            detail: format!("invalid response fields: {error}"),
        })
    }
}
