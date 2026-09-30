use clap::Subcommand;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InlineKeyboardMarkup {
    pub inline_keyboard: Vec<Vec<InlineKeyboardButton>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum InlineKeyboardButton {
    Callback { text: String, callback_data: String },
    Url { text: String, url: String },
}

impl InlineKeyboardMarkup {
    pub fn validate(&self) -> Result<(), String> {
        for row in &self.inline_keyboard {
            if row.is_empty() {
                return Err("inline keyboard rows must not be empty".into());
            }
            for button in row {
                let text = match button {
                    InlineKeyboardButton::Callback {
                        text,
                        callback_data,
                    } => {
                        if !(1..=64).contains(&callback_data.len()) {
                            return Err("callback_data must contain 1–64 UTF-8 bytes".into());
                        }
                        text
                    }
                    InlineKeyboardButton::Url { text, url } => {
                        let url = reqwest::Url::parse(url).map_err(|_| "invalid button URL")?;
                        if !matches!(url.scheme(), "http" | "https" | "tg")
                            || url.host_str().is_none()
                        {
                            return Err("button URL must use http, https, or tg with a host".into());
                        }
                        text
                    }
                };
                if text.trim().is_empty() {
                    return Err("button text must not be blank".into());
                }
            }
        }
        Ok(())
    }

    pub(crate) fn contains_callback(&self, data: &str) -> bool {
        self.inline_keyboard.iter().flatten().any(|button| {
            matches!(button, InlineKeyboardButton::Callback { callback_data, .. } if callback_data == data)
        })
    }
}

impl std::str::FromStr for InlineKeyboardMarkup {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let markup: Self = serde_json::from_str(value).map_err(|error| error.to_string())?;
        markup.validate()?;
        Ok(markup)
    }
}

#[derive(Debug, Clone, Subcommand, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Edit {
    Text {
        topic: String,
        msg_id: String,
        /// New text, or - to read UTF-8 content from stdin (CLI only).
        content: String,
        #[arg(long, value_enum)]
        format: Option<super::MessageFormat>,
        #[arg(long)]
        #[serde(default, skip_serializing_if = "super::is_false")]
        no_header: bool,
        /// InlineKeyboardMarkup JSON; omit to preserve existing buttons.
        #[arg(long)]
        reply_markup: Option<InlineKeyboardMarkup>,
    },
    Markup {
        topic: String,
        msg_id: String,
        /// InlineKeyboardMarkup JSON; an empty inline_keyboard removes buttons.
        #[arg(long)]
        reply_markup: InlineKeyboardMarkup,
    },
}

#[derive(Debug, Clone, Subcommand, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Callback {
    List {
        #[arg(long)]
        topic: Option<String>,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
    Answer {
        callback_query_id: String,
        #[arg(long)]
        text: Option<String>,
        #[arg(long)]
        #[serde(default, skip_serializing_if = "super::is_false")]
        show_alert: bool,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CallbackQuery {
    pub callback_query_id: String,
    pub topic: String,
    pub msg_id: String,
    pub user_id: i64,
    pub data: String,
    pub received_at: String,
}
