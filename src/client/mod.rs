use serde_json::Value;
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
    pub async fn execute(&self, route: &str, body: Value) -> std::result::Result<Value, String> {
        let url = format!("{}/v1/{route}", self.endpoint);
        let mut req = self.http.post(&url).json(&body);
        if route != "wait" {
            req = req.timeout(std::time::Duration::from_secs(40))
        }
        let r = req.send().await.map_err(|e| format!("{url}: {e}"))?;
        let status = r.status();
        let v: Value = r
            .json()
            .await
            .map_err(|e| format!("{url}: incomplete or invalid response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "{url}: {}",
                v["error"].as_str().unwrap_or("Gateway rejected request")
            ));
        }
        Ok(v)
    }
}
