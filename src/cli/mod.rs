use clap::Parser;
#[derive(Parser)]
#[command(name = "tbg", version, about = "Telegram Bot Gateway — Agent CLI")]
struct Args {
    #[arg(long, global = true)]
    agent: Option<String>,
    #[command(subcommand)]
    command: crate::contract::Command,
}
pub async fn run() -> std::result::Result<(), String> {
    let a = Args::parse();
    let request = crate::contract::Request::new(a.agent, a.command)?;
    let c = crate::config::ClientConfig::load()?;
    let client = crate::client::GatewayClient::new(c.endpoint())?;
    let v = client
        .execute(&request)
        .await
        .map_err(|error| error.to_string())?;
    println!("{}", serde_json::to_string(&v).map_err(|e| e.to_string())?);
    Ok(())
}
