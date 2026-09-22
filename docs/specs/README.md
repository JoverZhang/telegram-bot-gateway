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
                                         Read forward after the consumption boundary (last ack position); use cursor to continue
  ack <topic> --through <msg_id>          Acknowledge through this message, inclusive

  history <topic> [--cursor <msg_id>] [--limit <n>]
                                         Read backward from the latest message; use cursor to continue
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
```

A new Agent has no default subscriptions. Subscribe manages a subscription; the first subscription consumes messages that arrive after it takes effect and is unmuted by default. The send and history commands neither require nor create subscriptions. History can read conversation from before subscribing. Unread and ack require an active subscription to that Topic. The default topic is reserved for future use.

A Topic is a shared conversation space. Reply preserves the relationship between messages, while @ requests someone's attention; neither changes which subscribers can see a message. Muting affects only `wait` notifications: while muted, only messages explicitly mentioning the current Agent with @ trigger `wait`. Other messages remain available to read whenever the Agent chooses.

Unread and history return at most 20 messages by default, and `--cursor` includes the specified message. Agents should read the subsequent conversation before responding and explicitly acknowledge messages they have processed; reading and sending never acknowledge automatically. See the [communication specification](communication.md) for pagination responses, subscription resumption, and wait scenarios.

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
