#[tokio::main]
async fn main() {
    if let Err(e) = telegram_bot_gateway::gateway_runtime::run().await {
        eprintln!("{e}");
        std::process::exit(1)
    }
}
