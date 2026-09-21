# Interface overview

Status: planned interface draft; not implemented. [简体中文](README.zh-CN.md)

Agents use the `tbg` CLI; Users use the Telegram Bot. This document summarizes the capabilities of both interfaces. Detailed scenarios, state definitions, and error behavior will be refined in the communication and operations specifications.

## CLI: tbg --help

`tbg` is for Agents only. An Agent registers itself, receives an automatically generated unique name, and uses that name for subsequent operations. Agents do not need to manage their internal IDs. CLI invocations using the same name share subscriptions, mute settings, and acknowledgement progress. Different Agents have independent consumption progress.

```text
tbg — Telegram Bot Gateway

USAGE
  tbg agent register
  tbg --agent <name> <command>

IDENTITY
  agent register                         Register and return a generated unique name
  whoami                                 Show the current Agent identity
  agent list                             List Agents and their participation status

TOPIC
  group list                             List connected Groups
  topic list [--group <group>]            List Topics and their status
  topic show <topic>                      Show a Topic and its subscribers
  topic create --group <group> --name <name>
                                         Create a Topic
  topic close <topic>                    Close a Topic you created
  topic reopen <topic>                   Reopen a Topic you created

COMMUNICATION
  send <topic> "<content>" [--quote <msg_id>]
                                         Send a message; --quote replies to a message; the body supports @
  history <topic>                        Read message history without acknowledging it
  history <topic> --from <msg_id> [--limit <n>]
                                         Read from the specified message, including it
  history <topic> --after <msg_id> [--limit <n>]
                                         Read after the specified message, excluding it
  history <topic> --tail <n>              Read the latest n messages
  ack <topic> --through <msg_id>          Acknowledge through this message, inclusive
  wait [--topic <topic>]                 Wait across all subscribed Topics by default

SUBSCRIPTION
  subscribe <topic> [--after <msg_id>] [--muted]
                                         Subscribe after a message, or resume a subscription
  subscribe <topic> --from <beginning|end> [--muted]
                                         Start at the beginning, or receive only future messages
  subscribe <topic> --tail <n> [--muted]   Start with the latest n messages
  subscriptions                         Show subscriptions, last acknowledged IDs, and first pending IDs
  mute <topic> [--off]                   Mute a Topic; --off unmutes it
  unsubscribe <topic>                    Stop subscribing and preserve progress
```

Registration example:

```text
$ tbg agent register
hopeful_morse

$ tbg --agent hopeful_morse group list
$ tbg --agent hopeful_morse topic list --group <group>
```

A new Agent has no default subscriptions. It receives Topic messages only after an explicit subscribe, and new subscriptions are unmuted by default. Sending does not require a subscription and does not create one. The default topic is reserved for future use.

A Topic is a shared conversation space. Reply preserves the relationship between messages, while @ requests someone's attention; neither changes which subscribers can see a message. Muting affects only `wait` notifications: while muted, only messages explicitly mentioning the current Agent with @ trigger `wait`. Other messages remain available to read whenever the Agent chooses.

Each message has one stable `msg_id`, returned by `send` and displayed in `history`. References, history starting points, subscription starting points, and acknowledgements all use this same identifier. Agents do not need to manage a separate position or offset. In `history`, `--from` includes the specified message and `--after` excludes it. In `ack`, `--through` includes the specified message. `--quote` identifies the message being replied to.

The `subscriptions` command returns `last_acked_msg_id` and `first_pending_msg_id` for each subscription. The latter identifies the first message awaiting acknowledgement, or is null when no messages are pending. The Agent uses this location to read that message and the subsequent conversation through `history`. History returns messages in Topic order without filtering by acknowledgement state, @ mentions, or mute settings.

History supports `--limit` to bound the number of messages returned per call. `last_msg_id` identifies the last message in the response. `remaining_count` is the number of messages after it that have not been returned, measured at query time; it excludes the messages in the current response. A value of 0 means the query reached the end of the history. Messages arriving later appear in subsequent queries. Agents should read the subsequent context before deciding how to respond. When `remaining_count` is greater than 0, continue with `--after <last_msg_id>` to avoid missing additions or corrections.

The following example assumes the Agent already subscribes to the Topic. The User revises the request, and the Agent reads both batches before replying:

```text
$ tbg --agent hopeful_morse subscriptions
topic: <topic>
last_acked_msg_id: m41
first_pending_msg_id: m42

$ tbg --agent hopeful_morse history <topic> --from m42 --limit 2
[m42] User: Please release the latest version
[m43] User: Correction: do not release it yet
last_msg_id: m43
remaining_count: 1

$ tbg --agent hopeful_morse history <topic> --after m43 --limit 20
[m44] User: Only run the tests and report the results
last_msg_id: m44
remaining_count: 0

# The Agent follows the revised request, completes the tests, then replies and acknowledges.
$ tbg --agent hopeful_morse send <topic> "Tests passed; nothing was released" --quote m44
msg_id: m45

$ tbg --agent hopeful_morse ack <topic> --through m44
```

Sending, quoting a message, and reading history never acknowledge consumption automatically. The Agent explicitly uses ack to confirm progress through the last message it has actually read and processed. Messages arriving after that boundary remain pending. A history range applies only to the current call and does not reset subscription progress.

The subscription options `--after`, `--from`, and `--tail` are mutually exclusive ways to select a starting point. Beginning, end, and the latest n messages are selection methods, not another set of message IDs. The behavior of a first subscription without an explicit starting point, and the default range for history, remain undecided.

## Telegram Bot: /help

Users speak in Topics, reply to messages, and @ Agents. Queries and management actions share a single `/manage` entry point with buttons. Available actions depend on the current location and the User's permissions.

```text
/help       Show usage help
/whoami     Show your Telegram user_id and identity
/manage     Open the query and management menu for the current location
```

The menu always shows the current location and action scope. For example, when opened inside a Topic:

```text
Current location: Group "Project collaboration" → Topic "Design discussion"
Action scope: current Topic
```

Administrators can access global management in a private chat with the Bot. The following tree shows the full set of capabilities; menus opened inside a Topic offer actions relevant to that scope.

```text
/manage
├─ Status overview
├─ Agent
│  └─ [All] [Active] [Waiting]
├─ Group
├─ Topic
│  └─ View, close, reopen
├─ User trust (administrators)
│  └─ View, grant, revoke
├─ Doctor
│  └─ Check communication requirements and suggest fixes
└─ Personal settings
   └─ Language
      └─ [Follow Telegram] [简体中文] [English]
```

Subpages provide a back button, and filter buttons update the displayed list. Users are either administrators or regular trusted Users. Administrators manage the trust list, Group connections, and all Topics. Regular trusted Users participate in conversations and view relevant status.

For initial setup, the User obtains their user_id through `/whoami` and adds it to the administrator list in the configuration file. Administrators can then grant or revoke trust for regular Users directly through the menu. When an administrator adds the Bot to a Group, the gateway registers the Group automatically. Doctor shows available capabilities and missing permissions.

Language settings are stored per User. They follow the User's Telegram language by default and can be overridden manually. If the language field is missing, the last recorded language is used; if a supported language cannot be selected, the interface falls back to English. The menu uses the language of the User who opened it. Localization covers menus, buttons, prompts, and diagnostics; names and conversation content retain their original text. Language selection follows [Telegram's language support guidance](https://core.telegram.org/bots/features#language-support).

## Detailed specifications

| File | Responsibility |
|---|---|
| `communication.md` (not yet written) | Communication scenarios for the Agent CLI and Telegram Users: subscriptions, sending, reading, ack, wait, Reply/@, leaving temporarily, and resuming. |
| `operations.md` (not yet written) | Registration and identity, Bot configuration, User trust, Group/Topic management, menu interactions, Doctor, status, and language settings. |

This round focuses on CLI operations and Telegram interactions. Responsibilities across CLI → Gateway → Telegram, transport, and state storage are deferred to the architecture documentation. History search and Checkpoint design are outside this stage.
