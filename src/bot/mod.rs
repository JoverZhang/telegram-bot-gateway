pub(crate) fn conversation(agent: &str, content: &str) -> String {
    format!("{agent}:\n{content}")
}
pub(crate) fn command(text: &str) -> &str {
    text.split_whitespace()
        .next()
        .unwrap_or("")
        .split('@')
        .next()
        .unwrap_or("")
}
pub(crate) struct Context {
    pub user: i64,
    pub admin: bool,
    pub trusted: bool,
    pub initialized: bool,
    pub group: bool,
    pub topic: Option<i64>,
    pub zh: bool,
}
pub(crate) fn reply(command: &str, context: Context) -> String {
    let Context {
        user,
        admin,
        trusted,
        initialized,
        group,
        topic,
        zh,
    } = context;
    match command {
        "/whoami" => format!(
            "user_id: {user}\n{}: {}{}",
            if zh { "身份" } else { "Identity" },
            if admin {
                "administrator"
            } else if trusted {
                "trusted"
            } else {
                "untrusted"
            },
            if initialized {
                ""
            } else if zh {
                "\n请将 user_id 写入管理员配置后重启 Gateway。"
            } else {
                "\nAdd your user_id to the administrator configuration and restart the Gateway."
            }
        ),
        "/help" => "/help\n/whoami\n/manage".into(),
        "/manage" if trusted && initialized => format!(
            "{}: {}\n{}: {}",
            if zh {
                "当前位置"
            } else {
                "Current location"
            },
            if group {
                if topic.is_some() { "Topic" } else { "Group" }
            } else {
                "private chat"
            },
            if zh { "操作范围" } else { "Action scope" },
            if group {
                if topic.is_some() {
                    "current Topic"
                } else {
                    "current Group"
                }
            } else if admin {
                "global"
            } else {
                "personal"
            }
        ),
        _ => if zh {
            "请先通过 /whoami 获取身份并完成管理员配置。"
        } else {
            "Use /whoami to obtain your identity and complete administrator setup."
        }
        .into(),
    }
}
