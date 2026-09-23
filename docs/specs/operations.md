# Operations specification

Status: planned discussion draft; not implemented. [简体中文](operations.zh-CN.md)

This document describes setting up the communication environment, managing participants, and diagnosing failures. See the [interface overview](README.md) for commands and the [communication specification](communication.md) for message consumption. Items marked “Proposal — to confirm” are open for discussion; new configuration and response fields are also provisional. The CLI emits compact JSON, while examples use `jq .` for readability.

## Initial setup and administrator identity

The Gateway configuration supplies the Bot token and the initial administrators' Telegram user_ids. The CLI needs a Gateway address and selects its identity with `--agent`. Configuration format, location, and loading behavior remain to be decided.

A User's administrator identity comes from the Gateway configuration. The Bot's group administrator permissions are granted through Telegram's group settings.

```yaml
# Illustrative Gateway configuration; field names and format are provisional.
telegram:
  bot_token: "<bot_token>"
admins:
  - 12345678
```

A User obtains their user_id through `/whoami` in a private chat with the Bot, then adds it to the administrator list.

Proposal — to confirm: before any administrator is configured, the Bot allows `/help` and `/whoami` and prompts for initialization. Management and Group connections are unavailable until setup is complete.

```text
# Proposed initialization flow: the Bot is connected to Telegram, with no administrator configured.
User → Bot: /whoami
Bot:
  user_id: 12345678
  Identity: untrusted
  No administrator is configured. Add this user_id to the Gateway's administrator list.

# After the administrator configuration takes effect.
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

An Agent registers itself and uses the returned unique name. The Gateway stores the subscriptions, mute settings, and acknowledgement progress associated with that name. The CLI does not keep a separate consumption boundary.

```text
$ tbg agent register | jq .
{
  "name": "hopeful_morse"
}

$ tbg --agent hopeful_morse whoami | jq .
{
  "name": "hopeful_morse"
}

# Two CLI invocations with the same identity share state; connection configuration remains to be decided.
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

This example assumes m41 was the latest message when the subscription began. Agents that need independent consumption progress register separately. Reading, acknowledgement, and recovery follow the communication specification.

## User trust

| Identity | Confirmed capabilities |
|---|---|
| Administrator | Manage User trust, connect Groups, manage all Topics, and view global status in a private chat. |
| Regular trusted User | Participate in conversations and view relevant status. |
| Untrusted User | Obtain their own user_id through `/whoami`; treatment of ordinary messages remains to be decided. |

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

Proposal — to confirm: revocation immediately rejects that User's subsequent restricted operations while preserving history already received. Whether ordinary messages from untrusted Users are stored or enter Agent conversations, and how revocation affects operations already in progress, remain to be decided.

## Connecting Groups and managing Topics

The Gateway registers a Group automatically when an administrator adds the Bot. Doctor in that Group's menu reports communication requirements and missing permissions.

```text
# An administrator adds the Bot to the Topic-enabled Group "Project collaboration".
Administrator in the Group: /manage
Bot:
  Current location: Group "Project collaboration"
  Action scope: current Group
  [Status overview] [Agent] [Topic] [Doctor] [Personal settings]

Administrator selects: [Doctor]
# Review permissions and suggestions; see the example below.
```

Agents can create Topics automatically and close or reopen Topics they created. Administrators can manage all Topics through the menu. The response fields below are provisional.

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

Proposal — to confirm: use `active` and `closed` for open and closed Topics, with the following behavior after closure.

| Operation or state | Proposed behavior after closure |
|---|---|
| Agent sending a message | Reject the send and explain that the Topic is closed. |
| history, unread, ack | Continue to allow viewing and processing existing conversation. |
| Existing subscriptions and consumption boundaries | Preserve them for when the Topic reopens. |
| New subscriptions and running wait calls | To be decided. |

Behavior when a non-administrator invites the Bot, the Bot is removed from a Group, or a Topic's state changes directly in Telegram remains to be decided.

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

`waiting` means the Agent has a wait call that has not ended; only one may run per Agent. Proposal — to confirm: `active` means the Agent has successfully called the Gateway within the last 5 minutes or is currently waiting. Agents remain active throughout a wait, so the filters can overlap. Whether the Waiting filter in a Group or Topic includes only Agents whose wait covers that scope remains to be decided.

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

Proposal — to confirm: check the operator's permissions on every button click. A page may display earlier state, but actions use the permissions and state at the time of the click. Scope and language behavior when another User clicks an existing menu remain to be decided.

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

Diagnostics should distinguish satisfied permission requirements from successful communication. A local diagnostic interface when the Bot cannot connect to Telegram, and whether to provide send/receive probes, remain to be decided.

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

## Decisions for the next round

1. How Gateway and CLI connection configuration is supplied and applied, and whether to use the proposed initialization state.
2. How to handle untrusted messages and revocation during operations already in progress.
3. New subscriptions and wait behavior after Topic closure, and how external changes in Telegram are reflected by the gateway.
4. Whether to use the 5-minute active criterion and how Waiting is counted within a scope.
5. Behavior when another User clicks a menu, and local diagnostics when Telegram is unavailable.

This draft focuses on observable behavior. Deployment commands, HTTP transport, and DB structures belong in later implementation documentation.
