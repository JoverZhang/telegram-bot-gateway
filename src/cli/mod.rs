use clap::Parser;
#[derive(Parser)]
#[command(name = "tbg", version, about = "Telegram Bot Gateway — Agent CLI")]
struct Args {
    #[arg(long, global = true)]
    agent: Option<String>,
    /// Bound the complete HTTP request, including wait, in milliseconds.
    #[arg(long, global = true)]
    request_timeout_ms: Option<std::num::NonZeroU64>,
    #[command(subcommand)]
    command: crate::contract::Command,
}
pub async fn run() -> std::result::Result<(), String> {
    let mut a = Args::parse();
    if let crate::contract::Command::Send { content, .. }
    | crate::contract::Command::Edit(crate::contract::Edit::Text { content, .. }) =
        &mut a.command
        && content == "-"
    {
        use std::io::Read;
        let mut input = String::new();
        std::io::stdin()
            .read_to_string(&mut input)
            .map_err(|error| format!("cannot read message from stdin: {error}"))?;
        *content = input;
    }
    let request = crate::contract::Request::new(a.agent, a.command)?;
    let c = crate::config::ClientConfig::load()?;
    let client = crate::client::GatewayClient::new(c.endpoint())?.with_request_timeout(
        a.request_timeout_ms
            .map(|n| std::time::Duration::from_millis(n.get())),
    );
    let v = client
        .execute(&request)
        .await
        .map_err(|error| error.to_string())?;
    println!("{}", serde_json::to_string(&v).map_err(|e| e.to_string())?);
    Ok(())
}
