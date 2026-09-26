mod responses;
use clap::Subcommand;
pub use responses::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

// One command catalogue supplies clap syntax and the server's allowed routes.
#[derive(Debug, Clone, Subcommand, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    #[command(subcommand)]
    Agent(Agent),
    Whoami,
    #[command(subcommand)]
    Group(Group),
    #[command(subcommand)]
    Topic(Topic),
    Send {
        topic: String,
        content: String,
        #[arg(long)]
        quote: Option<String>,
    },
    Unread {
        topic: String,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
    History {
        topic: String,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
    Ack {
        topic: String,
        #[arg(long)]
        through: String,
    },
    Subscribe {
        topic: String,
        #[arg(long)]
        #[serde(default)]
        muted: bool,
    },
    Subscriptions,
    Unsubscribe {
        topic: String,
    },
    Mute {
        topic: String,
        #[arg(long)]
        #[serde(default)]
        off: bool,
    },
    Wait {
        #[arg(long)]
        topic: Option<String>,
        #[arg(long)]
        timeout: Option<u64>,
    },
}
#[derive(Debug, Clone, Subcommand, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Agent {
    Register {
        #[arg(long)]
        name: Option<String>,
    },
    List,
}
#[derive(Debug, Clone, Subcommand, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Group {
    List,
}
#[derive(Debug, Clone, Subcommand, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Topic {
    List {
        #[arg(long, allow_hyphen_values = true)]
        group: Option<String>,
    },
    Show {
        topic: String,
    },
    Create {
        #[arg(long, allow_hyphen_values = true)]
        group: String,
        #[arg(long)]
        name: String,
    },
    Close {
        topic: String,
    },
    Reopen {
        topic: String,
    },
}

/// The HTTP adapter validates wire data before calling Gateway.
#[derive(Debug, Clone)]
pub struct Request {
    pub agent: Option<String>,
    pub command: Command,
}
impl Request {
    pub fn new(agent: Option<String>, command: Command) -> Result<Self, String> {
        let request = Self { agent, command };
        if !matches!(request.command, Command::Agent(Agent::Register { .. }))
            && request.agent.is_none()
        {
            return Err("agent is required".into());
        }
        match &request.command {
            Command::Unread { limit: Some(0), .. } | Command::History { limit: Some(0), .. } => {
                return Err("limit must be positive".into());
            }
            Command::Wait {
                timeout: Some(timeout),
                ..
            } if *timeout > i64::MAX as u64 / 1000 => return Err("timeout is too large".into()),
            _ => (),
        }
        Ok(request)
    }
    pub fn decode(route: &str, body: Value) -> Result<Self, String> {
        let mut body = body
            .as_object()
            .cloned()
            .ok_or("request must be an object")?;
        if body.values().any(Value::is_null) {
            return Err("parameters must not be null".into());
        }
        let agent = body
            .remove("agent")
            .map(|value| {
                serde_json::from_value::<String>(value)
                    .map_err(|_| "agent must be a string".to_string())
            })
            .transpose()?;
        // Unit variants serialize as strings; struct variants as one-key objects.
        let mut parts = route.split('/').rev();
        let leaf = parts.next().ok_or("missing command")?;
        let payload =
            if body.is_empty() && !matches!(route, "agent/register" | "topic/list" | "wait") {
                Value::String(leaf.into())
            } else {
                json!({leaf: body})
            };
        let value = parts.fold(payload, |value, parent| json!({parent:value}));
        let command =
            serde_json::from_value(value).map_err(|error| format!("invalid command: {error}"))?;
        Self::new(agent, command)
    }
    pub fn wire(&self) -> (String, Value) {
        let value = serde_json::to_value(&self.command).expect("command serialization");
        let (first, value) = untag(value);
        let (route, mut body) = if matches!(first.as_str(), "agent" | "topic" | "group") {
            let (second, body) = untag(value);
            (format!("{first}/{second}"), body)
        } else {
            (first, value)
        };
        body.as_object_mut()
            .expect("command fields")
            .retain(|_, value| !value.is_null());
        if let Some(agent) = &self.agent {
            body["agent"] = json!(agent);
        }
        (route, body)
    }
}
fn untag(value: Value) -> (String, Value) {
    match value {
        Value::String(name) => (name, json!({})),
        Value::Object(map) => map.into_iter().next().expect("tagged command"),
        _ => unreachable!("command representation"),
    }
}
