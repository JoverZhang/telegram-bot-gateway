use clap::Subcommand;
use serde_json::{Value, json};

// One command catalogue supplies clap syntax and the server's allowed routes.
#[derive(Debug, Clone, Subcommand)]
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
        muted: bool,
    },
    Subscriptions,
    Unsubscribe {
        topic: String,
    },
    Mute {
        topic: String,
        #[arg(long)]
        off: bool,
    },
    Wait {
        #[arg(long)]
        topic: Option<String>,
        #[arg(long)]
        timeout: Option<u64>,
    },
}
#[derive(Debug, Clone, Subcommand)]
pub enum Agent {
    Register {
        #[arg(long)]
        name: Option<String>,
    },
    List,
}
#[derive(Debug, Clone, Subcommand)]
pub enum Group {
    List,
}
#[derive(Debug, Clone, Subcommand)]
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
impl Command {
    pub fn wire(&self, agent: Option<&str>) -> (&'static str, Value) {
        let (route, mut b) = match self {
            Self::Agent(Agent::Register { name }) => ("agent/register", json!({"name":name})),
            Self::Agent(Agent::List) => ("agent/list", json!({})),
            Self::Whoami => ("whoami", json!({})),
            Self::Group(Group::List) => ("group/list", json!({})),
            Self::Topic(Topic::List { group }) => ("topic/list", json!({"group":group})),
            Self::Topic(Topic::Show { topic }) => ("topic/show", json!({"topic":topic})),
            Self::Topic(Topic::Create { group, name }) => {
                ("topic/create", json!({"group":group,"name":name}))
            }
            Self::Topic(Topic::Close { topic }) => ("topic/close", json!({"topic":topic})),
            Self::Topic(Topic::Reopen { topic }) => ("topic/reopen", json!({"topic":topic})),
            Self::Send {
                topic,
                content,
                quote,
            } => (
                "send",
                json!({"topic":topic,"content":content,"quote":quote}),
            ),
            Self::Unread {
                topic,
                cursor,
                limit,
            } => (
                "unread",
                json!({"topic":topic,"cursor":cursor,"limit":limit}),
            ),
            Self::History {
                topic,
                cursor,
                limit,
            } => (
                "history",
                json!({"topic":topic,"cursor":cursor,"limit":limit}),
            ),
            Self::Ack { topic, through } => ("ack", json!({"topic":topic,"through":through})),
            Self::Subscribe { topic, muted } => ("subscribe", json!({"topic":topic,"muted":muted})),
            Self::Subscriptions => ("subscriptions", json!({})),
            Self::Unsubscribe { topic } => ("unsubscribe", json!({"topic":topic})),
            Self::Mute { topic, off } => ("mute", json!({"topic":topic,"off":off})),
            Self::Wait { topic, timeout } => ("wait", json!({"topic":topic,"timeout":timeout})),
        };
        b.as_object_mut().unwrap().retain(|_, v| !v.is_null());
        if let Some(a) = agent {
            b["agent"] = json!(a)
        }
        (route, b)
    }
}
pub fn validate(route: &str, b: &Value) -> std::result::Result<(), String> {
    let fields: &[(&str, char, bool)] = match route {
        "agent/register" => &[("name", 's', false)],
        "agent/list" | "whoami" | "group/list" | "subscriptions" => &[],
        "topic/list" => &[("group", 's', false)],
        "topic/create" => &[("group", 's', true), ("name", 's', true)],
        "topic/show" | "topic/close" | "topic/reopen" | "unsubscribe" => &[("topic", 's', true)],
        "send" => &[
            ("topic", 's', true),
            ("content", 's', true),
            ("quote", 's', false),
        ],
        "unread" | "history" => &[
            ("topic", 's', true),
            ("cursor", 's', false),
            ("limit", 'n', false),
        ],
        "ack" => &[("topic", 's', true), ("through", 's', true)],
        "subscribe" => &[("topic", 's', true), ("muted", 'b', false)],
        "mute" => &[("topic", 's', true), ("off", 'b', false)],
        "wait" => &[("topic", 's', false), ("timeout", 'n', false)],
        _ => return Err("unknown command".into()),
    };
    let obj = b.as_object().ok_or("request must be an object")?;
    for (k, v) in obj {
        let kind = if k == "agent" {
            's'
        } else {
            fields
                .iter()
                .find(|(n, _, _)| n == k)
                .map(|(_, t, _)| *t)
                .ok_or_else(|| format!("unknown parameter: {k}"))?
        };
        if !match kind {
            's' => v.is_string(),
            'n' => v.as_u64().is_some(),
            'b' => v.is_boolean(),
            _ => false,
        } {
            return Err(format!("invalid type: {k}"));
        }
    }
    for (key, _, required) in fields {
        if *required && !obj.contains_key(*key) {
            return Err(format!("missing parameter: {key}"));
        }
    }
    if route != "agent/register" && !obj.contains_key("agent") {
        return Err("agent is required".into());
    }
    if obj.get("limit").is_some_and(|n| n.as_u64() == Some(0)) {
        return Err("limit must be positive".into());
    }
    if obj
        .get("limit")
        .is_some_and(|n| n.as_u64().unwrap() > i64::MAX as u64)
        || obj
            .get("timeout")
            .is_some_and(|n| n.as_u64().unwrap() > i64::MAX as u64 / 1000)
    {
        return Err("numeric parameter is too large".into());
    }
    Ok(())
}
