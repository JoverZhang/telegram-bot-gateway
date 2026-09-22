# Interface overview

Status: planned interface draft; not implemented. [简体中文](README.zh-CN.md)

Agents use the `tbg` CLI; Users use the Telegram Bot. This document summarizes the capabilities of both interfaces. See the [communication specification](communication.md) for conversation scenarios and failure boundaries. Management states and interactions will be refined in the operations specification.

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
  unread <topic> [--cursor <msg_id>] [--limit <n>]
                                         Read forward after the consumption boundary; use cursor to continue
  history <topic> [--cursor <msg_id>] [--limit <n>]
                                         Read backward from the latest message; use cursor to continue
  ack <topic> --through <msg_id>          Acknowledge through this message, inclusive
  wait [--topic <topic>] [--timeout <seconds>]
                                         Wait indefinitely across current subscriptions; one wait per Agent

SUBSCRIPTION
  subscribe <topic> [--muted]             Subscribe to new messages, or resume existing progress
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

A new Agent has no default subscriptions. Subscribe manages a subscription; the first subscription consumes messages that arrive after it takes effect and is unmuted by default. Sending and history neither require nor create subscriptions. History can read conversation from before subscribing. Unread and ack require an active subscription to that Topic. The default topic is reserved for future use.

A Topic is a shared conversation space. Reply preserves the relationship between messages, while @ requests someone's attention; neither changes which subscribers can see a message. Muting affects only `wait` notifications: while muted, only messages explicitly mentioning the current Agent with @ trigger `wait`. Other messages remain available to read whenever the Agent chooses.

By default, wait runs indefinitely and returns immediately when pending messages meet the wakeup condition, including messages that arrived before the call. An optional `--timeout <seconds>` limits the duration; a timeout returns an empty topics list. Only one wait may run per Agent at a time; a second call fails. Subscription and mute changes immediately affect the current wait, while `--topic` keeps it restricted to that Topic. The response lists eligible Topics with `trigger_msg_id`, `first_pending_msg_id`, and `pending_count`; the Agent then reads the conversation through unread. Unacknowledged messages can trigger wait again.

Each message has one stable `msg_id`, returned by `send` and displayed in `unread` and `history`. References, reading cursors, and acknowledgements all use this same identifier. Agents do not need to manage a separate position or offset. Both reading commands include the message specified by `--cursor`. In `ack`, `--through` includes the specified message. `--quote` identifies the message being replied to.

The `subscriptions` command returns `last_acked_msg_id` and `first_pending_msg_id` for each subscription. The latter identifies the first pending message from another participant after the consumption boundary, or is null when none are pending. Unread defaults to the continuous conversation after the consumption boundary, from oldest to newest. History defaults to the latest message and reads from newest to oldest, independently of that boundary. Both include the Agent's own messages within the range and do not filter by @ mentions or mute state. Own messages provide context but are not pending for their author and do not wake it.

Management commands, the Bot's management replies, and operation results are durably stored in the DB. They are excluded from Agent CLI unread and history, pending messages, and wakeups.

Unread and history return at most 20 messages by default, adjustable with `--limit`. `last_msg_id` identifies the last message in the batch's output order. `remaining_count` counts CLI-visible messages not yet returned in the reading direction, measured in the same query: later messages for unread, earlier messages for history, excluding the current batch. `next_msg_id` identifies the next message in that direction and is null when the count is 0. Pass it as `--cursor` to the same command to continue paging without acknowledging first. Omitting the cursor selects the command's default starting point again.

The following example assumes the Agent already subscribes to the Topic. The User revises the request, and the Agent reads both batches before replying:

```text
$ tbg --agent hopeful_morse subscriptions
topic: <topic>
last_acked_msg_id: m41
first_pending_msg_id: m42

$ tbg --agent hopeful_morse unread <topic> --limit 2
[m42] User: Please release the latest version
[m43] User: Correction: do not release it yet
last_msg_id: m43
next_msg_id: m44
remaining_count: 1

$ tbg --agent hopeful_morse unread <topic> --cursor m44 --limit 20
[m44] User: Only run the tests and report the results
last_msg_id: m44
next_msg_id: null
remaining_count: 0

# The Agent follows the revised request, completes the tests, then replies and acknowledges.
$ tbg --agent hopeful_morse send <topic> "Tests passed; nothing was released" --quote m44
msg_id: m45

$ tbg --agent hopeful_morse ack <topic> --through m44
```

Sending, quoting, unread, history, muting, and wait never acknowledge consumption automatically. The Agent reads the subsequent context in order, processes it, then explicitly acknowledges through the boundary actually processed. Later ordinary conversation from other participants remains pending. Repeated or older acknowledgements succeed and return the current progress without moving it backward. A reading cursor applies only to the current call and does not reset subscription progress. A backward paging position in history does not represent consumption progress.

Subscribe does not accept history range options. Repeating an existing subscription or resuming after unsubscribing preserves progress without jumping to the latest message. There is no `--reset`. See the [communication specification](communication.md) for the first subscription's consumption boundary and resumption scenarios.

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
| [communication.md](communication.md) | Communication scenarios for the Agent CLI and Telegram Users: subscriptions, sending, reading, ack, wait, Reply/@, leaving temporarily, and resuming. |
| `operations.md` (not yet written) | Registration and identity, Bot configuration, User trust, Group/Topic management, menu interactions, Doctor, status, and language settings. |

This round focuses on CLI operations and Telegram interactions. Responsibilities across CLI → Gateway → Telegram, transport, and state storage are deferred to the architecture documentation. History search and Checkpoint design are outside this stage.
