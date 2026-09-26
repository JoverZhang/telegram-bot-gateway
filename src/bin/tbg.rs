#[tokio::main]
async fn main() {
    if let Err(e) = telegram_bot_gateway::cli::run().await {
        eprintln!("{e}");
        std::process::exit(1)
    }
}
