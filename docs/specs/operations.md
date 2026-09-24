# Operations specification

Status: planned behavior specification; not implemented. [简体中文](operations.zh-CN.md)

This document describes setting up the communication environment, managing participants, and diagnosing failures. See the [interface overview](README.md) for commands and the [communication specification](communication.md) for message consumption. The scenarios describe intended behavior. The CLI emits compact JSON, while examples use `jq .` for readability.

## Initial setup and administrator identity

The Client and Server communicate over HTTP and each use one global YAML configuration file per operating-system user. `~` refers to the home directory of the user running the respective program. All projects and Agents running as that user share the Client configuration; Agent identity is still selected with `--agent <name>`. Both sides require explicitly configured ports; `18473` below is only an example value.

```yaml
# ~/.config/tbg/client.yaml
host: "127.0.0.1"
port: 18473
```

The Client must explicitly supply the Gateway's IP address (`host`) and `port`. If either is missing, the CLI reports a configuration error without attempting a connection. Each CLI invocation reads the file. A running wait keeps its original connection. This example connects to `http://127.0.0.1:18473`.

```yaml
# ~/.config/tbg/server.yaml
listen: "0.0.0.0:18473"
telegram:
  bot_token: "<bot_token>"
admins:
  - 12345678
```

The Server must explicitly specify its listening IP address and port through `listen`; missing configuration prevents startup. Configuration changes take effect after manually restarting the Gateway. If the port is occupied, startup fails and reports the listening address without selecting another port. The Client's `port` identifies the port at which the Gateway is actually exposed to clients.

A User's administrator identity comes from the Server configuration. The Bot's group administrator permissions are granted through Telegram's group settings. The User first configures the Bot token, obtains their user_id through `/whoami` in a private chat with the Bot, then adds it to the administrator list and manually restarts the Gateway.

Before any administrator is configured, the Bot allows `/help` and `/whoami` and prompts for initialization. Management and Group connections are unavailable until setup is complete.

```text
# The Bot is connected to Telegram, with an empty administrator list.
User → Bot: /whoami
Bot:
  user_id: 12345678
  Identity: untrusted
  No administrator is configured. Add this user_id to the Gateway's administrator list.

# After the User adds their user_id to server.yaml and manually restarts the Gateway.
User → Bot: /whoami
Bot:
  user_id: 12345678
  Identity: administrator

User → Bot: /manage
Bot:
  Current location: private chat with the Bot
  Action scope: global
  [Status overview] [Agent] [Group] [Topic]
  [User trust] [Doctor] [Personal settings]
```

## Agent identity and CLI state

An Agent registers with `agent register [--name <name>]`. A name is generated automatically only when `--name` is omitted. Every Agent name must match this regular expression in full:

```regex
^[A-Za-z][A-Za-z0-9_]*$
```

Names start with an ASCII letter; subsequent characters may be ASCII letters, digits, or underscores. Explicit values are validated as supplied. A format mismatch or name conflict causes an error without automatic trimming, substitution, or renaming. Failed registration leaves existing Agent state unchanged.

After registration, the Agent uses the returned unique name. The Gateway stores the subscriptions, mute settings, and acknowledgement progress associated with that name; the CLI does not keep a separate consumption boundary.

```text
# Choose a name.
$ tbg agent register --name hopeful_morse | jq .
{
  "name": "hopeful_morse"
}

# Generate a name only when --name is omitted.
$ tbg agent register | jq .
{
  "name": "calm_turing"
}

$ tbg agent register --name hopeful_morse
# Registration fails: the name is already in use; the existing Agent's state is unchanged.

$ tbg --agent hopeful_morse whoami | jq .
{
  "name": "hopeful_morse"
}

# Two CLI invocations share the Client configuration; using the same identity also shares consumption state.
CLI A: tbg --agent hopeful_morse subscribe <topic>
CLI B: tbg --agent hopeful_morse subscriptions | jq .
       {
         "subscriptions": [
           {
             "topic": "<topic>",
             "last_acked_msg_id": "m41"
           }
         ]
       }
```

This example assumes m41 was the latest message when the subscription began. Identity, subscriptions, and consumption progress remain in the Gateway after a CLI process exits. A new process can continue using the same name without registering again. Agents that need independent consumption progress register separately. Reading, acknowledgement, and recovery follow the communication specification.

## User trust

| Identity | Capabilities |
|---|---|
| Administrator | Manage User trust, connect Groups, manage all Topics, and view global status in a private chat. |
| Regular trusted User | Participate in conversations and view relevant status. |
| Untrusted User | Use `/help` and `/whoami`; ordinary messages are ignored, not stored in the DB, excluded from unread/history, and never trigger wait. |

Administrators grant or revoke trust for regular Users directly through the menu, identifying the target by user_id.

```text
# An administrator's private chat with the Bot.
/manage → [User trust] → [Grant]
Bot: Enter the Telegram user_id.
Administrator: 87654321
Bot: Trust granted to 87654321.

/manage → [User trust] → [87654321] → [Revoke]
Bot: Trust revoked for 87654321.
```

Revocation affects only new messages and requests received after it takes effect: ordinary messages are ignored and restricted operations are rejected. Previously received messages remain stored and available to read and process. Operations already accepted continue, and consumption progress is unchanged.

```text
Before revocation: trusted User message m42 was received; the Agent has not acknowledged it.
An administrator revokes that User's trust.
After revocation: the User's new messages are ignored; m42 can still be read and acknowledged.
```

## Connecting Groups and managing Topics

The Gateway registers a Group automatically when a Gateway administrator adds the Bot. An invitation from another User does not connect the Group's conversation. A Gateway administrator completes the connection by entering `/manage` in that Group, which also opens the menu for the current location. Ordinary conversation before connection is ignored and is not imported later.

Doctor in that Group's menu reports communication requirements and missing permissions. Authority to connect a Group comes from Gateway administrator status, separately from Telegram group administrator status.

```text
# An administrator adds the Bot to the Topic-enabled Group "Project collaboration".
Administrator in the Group: /manage
Bot:
  Current location: Group "Project collaboration"
  Action scope: current Group
  [Status overview] [Agent] [Topic] [Doctor] [Personal settings]

Administrator selects: [Doctor]
# Review permissions and suggestions; see the example below.

# Someone who is not a Gateway administrator invites the Bot to another Group.
User: Please start the discussion.
# The Group is not connected; ordinary messages are ignored.

Gateway administrator in that Group: /manage
# Connect the Group and open the menu; subsequent conversation follows User trust rules.
```

Agents can create Topics automatically in connected Groups and close or reopen Topics they created. Administrators can manage all Topics through the menu.

```text
$ tbg --agent hopeful_morse topic create --group <group> --name "Design discussion" | jq .
{
  "topic": "<topic>",
  "name": "Design discussion",
  "status": "active"
}

$ tbg --agent hopeful_morse topic close <topic> | jq .
{
  "topic": "<topic>",
  "status": "closed"
}

$ tbg --agent hopeful_morse topic reopen <topic>
# The Topic is active again.
```

Topics use `active` and `closed` for open and closed states. Closure only restricts sending; other operations follow the communication specification.

| Operation or state | Behavior after closure |
|---|---|
| Agent sending a message | Reject the send and explain that the Topic is closed. |
| history, unread, ack | Continue to allow viewing and processing existing conversation. |
| subscribe | Remains available; a first subscription starts at the current latest position, while resuming uses saved progress. |
| Subscriptions, mute settings, and consumption boundaries | Preserve them throughout. |
| wait | Qualifying pending messages still cause it to return; otherwise it keeps waiting. Closing and reopening do not themselves trigger or end wait. |

If a Topic is closed or reopened directly in Telegram, the Gateway updates its state when it receives the notification and applies the same rules. State notifications are stored as management records and excluded from conversation. Telegram provides these [Topic state notifications](https://core.telegram.org/bots/api#message).

When the Bot is removed from a Group, the global menu and `group list` mark that Group as unavailable while preserving its history, subscriptions, and consumption progress. Telegram operations that require access to that Group fail. Local reads, ack, and wait retain their existing rules. Rejoining follows the connection procedure above and resumes the saved state once connected.

## Menu scope and participation status

`/manage` follows the current location and always displays the action scope. The Agent page inside a Topic shows that Topic's subscribers. Administrators can view global information in a private chat with the Bot. The interface overview contains the full menu catalog.

```text
# Open the menu inside the "Design discussion" Topic.
/manage → [Agent]
Current location: Group "Project collaboration" → Topic "Design discussion"
Action scope: current Topic
[All] [Active] [Waiting]

hopeful_morse   Active, waiting on the current Topic
calm_turing     Active, not waiting
[Back]
```

`active` means the Agent has successfully called the Gateway within the last 5 minutes or is currently waiting. It describes communication activity, not whether the Agent is executing a task. `waiting` means the Agent has a wait call that has not ended. Only one may run per Agent, and the Agent remains active throughout the wait.

A Topic's Agent list contains its current subscribers. A Group's list is the union of subscribers to its Topics; the global list contains all registered Agents. On Topic/Group pages, Waiting counts only Agents whose wait currently covers at least one Topic in that scope. Global Waiting counts all Agents currently waiting; each Agent is counted once. A default wait follows current subscriptions; on Topic/Group pages, a wait for one specified Topic counts only toward the corresponding scope.

```text
# An administrator views global status in a private chat; counts are illustrative.
/manage → [Status overview]
Bot: connected to Telegram
Agent: 4 total, 2 active, including 1 waiting
Group: 2 connected
Topic: 3 active, 1 closed
[Agent] [Group] [Topic] [Doctor] [Back]

/manage → [Group]
Project collaboration
Daily tasks
[Back]

/manage → [Topic]
[active] [closed]
```

Only the User who opened a menu can operate it, and each click checks that User's current permissions. A page may display earlier state, but actions use the permissions and state at the time of the click. Another User who clicks is prompted to open their own menu with `/manage`; the original menu's scope and language stay unchanged.

```text
User A opens /manage inside a Topic.
User B clicks a button in that menu.
Bot prompts User B: Open your own menu with /manage first.
# User A's menu stays unchanged.
```

## Doctor and language settings

Doctor is available through `/manage` and explains the check's scope, findings, and next steps. Creating a Topic in a Group requires the Bot to be an administrator with `can_manage_topics`. Visibility of ordinary group messages also depends on administrator status and Privacy Mode. Sources: [Topic creation requirements](https://core.telegram.org/bots/api#createforumtopic), [group message delivery rules](https://core.telegram.org/bots/faq#what-messages-will-my-bot-get).

```text
/manage → [Doctor]
Scope: Group "Project collaboration"

Topics: enabled
Bot administrator: no
Create Topic: unavailable

Suggestion: make the Bot an administrator and grant Manage Topics permission.
[Check again] [Back]

# After granting the permissions, select "Check again".
Bot administrator: yes
Manage Topics permission: granted
Create Topic: permission requirements met
```

Diagnostics distinguish satisfied permission requirements from successful communication. Doctor shows connection status and permission checks. Send/receive results come from normal communication; Doctor does not send probe messages automatically. When Telegram is unavailable, local diagnostics come from the Gateway's startup and runtime logs. With Docker, these are available through the container logs.

| Scenario | Feedback |
|---|---|
| Missing or invalid Bot token | Gateway logs explain the configuration problem for local diagnosis. |
| CLI cannot connect to the Gateway | The CLI reports the actual HTTP address and connection failure reason to stderr and exits with a nonzero status. |
| Bot lacks permissions in a Group | `/manage → Doctor` identifies the missing permissions and how to grant them. |

Diagnostic output does not contain the Bot token.

Language settings are saved per User and follow Telegram by default, with a manual override. Menus use the opener's language. A missing language field retains the last recorded value; if a supported language cannot be selected, the interface falls back to English. Names and conversation bodies retain their original text.

```text
/manage → [个人设置] → [语言]
当前：跟随 Telegram（简体中文）
[跟随 Telegram] [简体中文] [English]

User selects: [English]
Language: English
[Follow Telegram] [简体中文] [English]
```

Management commands, menu operations, the Bot's management replies, and their results are stored in the DB. They are excluded from Agent unread/history and do not trigger wait.

This document defines observable behavior. HTTP API request and response contracts still need to be specified. Deployment commands, configuration mounts, internal transport implementation, and DB structures belong in later docs/how documentation.
