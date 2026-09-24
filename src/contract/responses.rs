pub use crate::model::{GroupInfo, Message, Sender, Subscription, TopicInfo};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentStatus {
    pub name: String,
    pub active: bool,
    pub waiting: bool,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ReadyTopic {
    pub topic: String,
    pub trigger_msg_id: String,
    pub pending_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Response {
    Identity {
        name: String,
    },
    Agents {
        agents: Vec<AgentStatus>,
    },
    Groups {
        groups: Vec<GroupInfo>,
    },
    Topics {
        topics: Vec<TopicInfo>,
    },
    TopicDetail {
        topic: String,
        name: String,
        group: String,
        status: String,
        subscribers: Vec<String>,
    },
    TopicCreated {
        topic: String,
        name: String,
        status: String,
    },
    TopicState {
        topic: String,
        status: String,
    },
    Topic {
        topic: String,
    },
    Subscriptions {
        subscriptions: Vec<Subscription>,
    },
    Muted {
        topic: String,
        muted: bool,
    },
    Sent {
        msg_id: String,
    },
    Page {
        messages: Vec<Message>,
        next_cursor: Option<String>,
        remaining_count: i64,
    },
    Ack {
        last_acked_msg_id: String,
    },
    Wait {
        topics: Vec<ReadyTopic>,
    },
}

impl Response {
    pub fn decode(
        command: &super::Command,
        value: serde_json::Value,
    ) -> Result<Self, serde_json::Error> {
        // Empty Topic lists and empty wait results share the wire shape; the request disambiguates.
        if matches!(command, super::Command::Wait { .. }) {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Waiting {
                topics: Vec<ReadyTopic>,
            }
            let waiting: Waiting = serde_json::from_value(value)?;
            Ok(Self::Wait {
                topics: waiting.topics,
            })
        } else {
            serde_json::from_value(value)
        }
    }
}
