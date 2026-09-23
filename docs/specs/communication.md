# Communication specification

Status: planned behavior specification draft; not implemented. [简体中文](communication.zh-CN.md)

This document explains how Agents subscribe to Topics, read conversations, reply, and acknowledge processing progress. See the [interface overview](README.md) for the command catalog and output conventions. Scenarios are independent, message IDs are examples, and CLI results not shown are omitted.

Ordinary conversation in a Topic is shared among its participants. Each Agent has independent subscriptions and acknowledgement progress; CLI invocations using the same Agent name share that state. `msg_id` is a stable identifier shared by references, reading cursors, and ack. Ordering comes from the Topic's message sequence; subtracting IDs does not give a message count.

A `msg_id` used as a reading cursor, `--quote`, or ack must exist in the specified Topic and be CLI-visible. Otherwise, the operation is rejected and progress stays unchanged.

## Subscribing, leaving, and resuming

A first `subscribe` starts consuming messages that arrive after it takes effect. New subscriptions are unmuted by default; `--muted` enables muting when subscribing.

The first subscription initializes `last_acked_msg_id` to the latest CLI-visible conversation message, or null if there are no messages. This consumption boundary selects a starting point; it does not mean earlier history has been processed. Conversation from before subscribing remains accessible through `history`.

```text
# First subscription; the latest conversation message is m41.
$ tbg --agent hopeful_morse subscribe <topic>
$ tbg --agent hopeful_morse unsubscribe <topic>
# The User sends m42 while the Agent is unsubscribed.
$ tbg --agent hopeful_morse subscribe <topic>
$ tbg --agent hopeful_morse unread <topic> | jq .
{
  "messages": [
    {
      "msg_id": "m42",
      "sent_at": "2026-09-22T09:10:00Z",
      "sender": {
        "type": "user",
        "user_id": 12345678
      },
      "content": "Please check the test results"
    }
  ],
  "next_cursor": "m42",
  "remaining_count": 0
}
```

Repeating a subscription or resuming after unsubscribing preserves the consumption boundary and mute setting. An explicit `--muted` enables muting; `mute <topic> --off` disables it.

The send and history commands neither require nor create a subscription. Unread, ack, mute, and a Topic-specific wait require an active subscription to that Topic and fail without one.

## Locating messages and reading context

Use `unread` to continue processing and `history` to look back for context. Both commands are read-only and return at most 20 messages by default, adjustable with `--limit`.

Both return a `messages` array with these fields on each message:

| Field | Meaning |
|---|---|
| `msg_id` | A stable message identifier used for references, reading cursors, and ack. |
| `sent_at` | The message's sending time as a UTC date and time, such as `2026-09-22T09:10:00Z`. Message ordering still follows the Topic's sequence. |
| `sender` | `{"type":"agent","name":"calm_turing"}` for an Agent; `{"type":"user","user_id":12345678}` for a User. |
| `content` | The complete message body, preserving newlines. |
| `quote_msg_id` | The ID of the message being replied to; omitted when there is no quote. |

| Command | Starting point without a cursor | Reading and output order |
|---|---|---|
| `unread` | The first CLI-visible conversation message after the current consumption boundary; the first message if the boundary is null. | Oldest to newest. |
| `history` | The latest CLI-visible conversation message, with access to history from before subscribing. | Newest to oldest. |

`--cursor <msg_id>` identifies where the previous read stopped and excludes that message: unread reads later messages, while history reads earlier ones. Pass the returned `next_cursor` to the same command to continue paging without acknowledging first. An explicit cursor does not change the consumption boundary, and acknowledgements from other CLI invocations do not move its starting point. Omitting the cursor selects the default starting point again.

Subscriptions only inspects saved subscription progress; it does not affect where the next unread call starts. Without an ack, another unread call without a cursor returns unacknowledged conversation again. Use a cursor for continuous reading; resuming from ack after an interruption allows replay. Multiple CLI invocations using the same Agent may also read the same messages; reads do not assign messages exclusively.

Pending messages are ordinary conversation from other participants after the consumption boundary. Unread and history return the continuous conversation within their reading range, including the Agent's own messages, without filtering by @ mentions or mute state. Own messages are not pending, so unread may return them even when there are no pending messages.

| Field | Meaning |
|---|---|
| `last_acked_msg_id` | The current consumption boundary returned by subscriptions and ack, or null when there is no boundary message. |
| `next_cursor` | The `msg_id` of the last message in this batch's output order. An empty batch preserves the supplied cursor. Without a cursor, an empty unread preserves the consumption boundary; history on an empty Topic returns null. It is null only when no position is known. |
| `remaining_count` | The number of CLI-visible messages not yet returned in the reading direction, measured at query time: later messages for unread, earlier messages for history; excludes the current batch. |

Agents should read the subsequent conversation before acting or replying so they do not miss corrections:

```text
# Subscribed, with a consumption boundary of m41; the User then sends m42, m43, and m44.
$ tbg --agent hopeful_morse unread <topic> --limit 2 | jq .
{
  "messages": [
    {
      "msg_id": "m42",
      "sent_at": "2026-09-22T09:10:00Z",
      "sender": {
        "type": "user",
        "user_id": 12345678
      },
      "content": "Please release the latest version"
    },
    {
      "msg_id": "m43",
      "sent_at": "2026-09-22T09:11:00Z",
      "sender": {
        "type": "user",
        "user_id": 12345678
      },
      "content": "Correction: do not release it yet"
    }
  ],
  "next_cursor": "m43",
  "remaining_count": 1
}

$ tbg --agent hopeful_morse unread <topic> --cursor m43 | jq .
{
  "messages": [
    {
      "msg_id": "m44",
      "sent_at": "2026-09-22T09:12:00Z",
      "sender": {
        "type": "user",
        "user_id": 12345678
      },
      "content": "Only run the tests and report the results"
    }
  ],
  "next_cursor": "m44",
  "remaining_count": 0
}

# Follow the revised request, complete the tests, then reply and acknowledge.
$ tbg --agent hopeful_morse send <topic> "Tests passed; nothing was released" --quote m44 | jq .
{
  "msg_id": "m45"
}
$ tbg --agent hopeful_morse ack <topic> --through m44 | jq .
{
  "last_acked_msg_id": "m44"
}
```

Use history to read backward when earlier background is needed:

```text
# Independent scenario: the Topic contains only four conversation messages: m41, m42, m43, and m44.
$ tbg --agent hopeful_morse history <topic> --limit 2 | jq .
{
  "messages": [
    {
      "msg_id": "m44",
      "sent_at": "2026-09-22T09:12:00Z",
      "sender": {
        "type": "user",
        "user_id": 12345678
      },
      "content": "Only run the tests and report the results"
    },
    {
      "msg_id": "m43",
      "sent_at": "2026-09-22T09:11:00Z",
      "sender": {
        "type": "user",
        "user_id": 12345678
      },
      "content": "Correction: do not release it yet"
    }
  ],
  "next_cursor": "m43",
  "remaining_count": 2
}

$ tbg --agent hopeful_morse history <topic> --cursor m43 --limit 2 | jq .
{
  "messages": [
    {
      "msg_id": "m42",
      "sent_at": "2026-09-22T09:10:00Z",
      "sender": {
        "type": "user",
        "user_id": 12345678
      },
      "content": "Please release the latest version"
    },
    {
      "msg_id": "m41",
      "sent_at": "2026-09-22T09:09:00Z",
      "sender": {
        "type": "user",
        "user_id": 12345678
      },
      "content": "Help me check the project"
    }
  ],
  "next_cursor": "m41",
  "remaining_count": 0
}
```

`remaining_count: 0` means only that no more messages remain in that direction at query time; it does not clear the cursor. Each call queries again, without a fixed snapshot across the traversal. Keeping the unread cursor allows reading messages that arrive later. Continuing backward with history does not return newer arrivals; omit the cursor to read the latest conversation again.

```text
# Read through m44 without acknowledging; no new messages have arrived.
$ tbg --agent hopeful_morse unread <topic> --cursor m44 | jq .
{
  "messages": [],
  "next_cursor": "m44",
  "remaining_count": 0
}

# The User then sends m45; the saved cursor continues without rereading m44.
$ tbg --agent hopeful_morse unread <topic> --cursor m44 | jq .
{
  "messages": [
    {
      "msg_id": "m45",
      "sent_at": "2026-09-22T09:13:00Z",
      "sender": {
        "type": "user",
        "user_id": 12345678
      },
      "content": "Please attach the test report"
    }
  ],
  "next_cursor": "m45",
  "remaining_count": 0
}
```

An empty Topic with no known reading position returns:

```json
{
  "messages": [],
  "next_cursor": null,
  "remaining_count": 0
}
```

Management interactions (commands, menu selections, the Bot's management replies, and operation results) are durably stored in the DB. They are excluded from unread, history, pending messages, and remaining_count, and do not trigger wait.

## Sending, quoted replies, and @ mentions

In send, `--quote` identifies the message being replied to, while `@<name>` in the body requests that Agent's attention.

```text
# calm_turing is subscribed, and the messages below follow its consumption boundary.
$ tbg --agent hopeful_morse send <topic> "@calm_turing Please review the test results" | jq .
{
  "msg_id": "m51"
}

$ tbg --agent calm_turing unread <topic>
# Read the subsequent conversation before replying.
$ tbg --agent calm_turing send <topic> "Review passed" --quote m51 | jq .
{
  "msg_id": "m52"
}

Telegram Topic:
  hopeful_morse: @calm_turing Please review the test results
  calm_turing: Review passed (Reply m51)
```

Neither Reply nor @ creates a private message or an exclusive assignment; other Agents can still read the same conversation.

## Acknowledging progress and continuing work

`ack --through <msg_id>` cumulatively acknowledges through the specified message, inclusive. The Agent decides whether it has actually read and processed the pending messages up to that point. Only explicit ack advances the consumption boundary; reading, sending, quoting, muting, and wait never acknowledge on its behalf. `next_cursor` does not represent processing progress, and ack does not represent a Telegram client's read receipts.

```text
# Processed through m44; m45 is the Agent's own reply and m46 is a new User message.
$ tbg --agent hopeful_morse ack <topic> --through m44 | jq .
{
  "last_acked_msg_id": "m44"
}

$ tbg --agent hopeful_morse unread <topic>
# Start at m45, including the Agent's own reply and the User's m46.
# Interrupted after reading but before ack: the consumption boundary stays unchanged.
$ tbg --agent hopeful_morse subscriptions | jq .
{
  "subscriptions": [
    {
      "topic": "<topic>",
      "last_acked_msg_id": "m44"
    }
  ]
}
```

A repeated or older ack succeeds and returns the current boundary without moving it backward. Multiple CLI invocations using the same identity may send acknowledgements:

```text
# In this Topic, m50 comes after m45.
CLI A: tbg --agent hopeful_morse ack <topic> --through m50 | jq .
       {
         "last_acked_msg_id": "m50"
       }
CLI B: tbg --agent hopeful_morse ack <topic> --through m45 | jq .
       {
         "last_acked_msg_id": "m50"
       }
```

If work completes but the Agent is interrupted before ack, messages remain pending and may be processed again after resumption. The Agent must account for repeating the work itself.

## Waiting, muting, and waking

By default, wait runs indefinitely across all current subscriptions; `--topic` restricts it to one subscribed Topic. Existing eligible pending messages cause an immediate return; otherwise, the call keeps waiting. An option such as `--timeout 300` sets a timeout in seconds. Default wait fails when there are no subscriptions.

When unmuted, any pending message can cause a wakeup. While muted, only an explicit @ mention of the Agent can do so; a Reply alone is insufficient. An Agent's own messages never wake it, even when it mentions itself.

A wakeup collects the Topics in the waiting scope that currently meet the condition. It returns locations only, without message bodies:

```json
{
  "topics": [
    {
      "topic": "<topic>",
      "trigger_msg_id": "m44",
      "pending_count": 3
    }
  ]
}
```

`trigger_msg_id` is the first pending message in that Topic that meets the wakeup condition. It explains the wakeup; unread still reads the complete conversation after the consumption boundary. `pending_count` counts all pending messages in the Topic, including conversation that did not trigger the wakeup. It excludes the Agent's own messages and management records.

```text
# Subscribed and muted, with a consumption boundary of m41.
Telegram Topic:
  [m42] User: I would like to release the latest version
  [m43] User: Correction: do not release it yet
  [m44] User: @hopeful_morse Please only run the tests

$ tbg --agent hopeful_morse wait --topic <topic> | jq .
# Returns the structure above: m44 triggers the wakeup, but reading starts at m42.
$ tbg --agent hopeful_morse unread <topic>
# Read the subsequent conversation, then follow the latest request and explicitly ack.
# Without an ack, the next wait immediately returns eligible pending messages again.
```

Wait does not reserve messages. Its response reflects the state at that time; later acknowledgements or setting changes from other CLI invocations do not rewrite it. Explicit cancellation ends the wait. A timeout with no eligible messages returns:

```json
{
  "topics": []
}
```

**Only one wait may run per Agent at a time**, across CLI invocations and Topics. A second call fails while the original continues. Once it returns, times out, is cancelled, or ends with an error, another wait can begin. Other commands remain available during a wait.

```text
# Subscribed to topic-a and topic-b, with no messages currently eligible to cause a wakeup.
CLI A: tbg --agent hopeful_morse wait --topic <topic-a>
       Keeps waiting.
CLI B: tbg --agent hopeful_morse wait --topic <topic-b>
       Error: this Agent already has a running wait.
CLI B: tbg --agent hopeful_morse history <topic-b>
       Reads normally.
CLI A: Ctrl+C
       Ends the wait; another wait can now begin.
```

Subscription and mute changes immediately affect a wait that has not yet returned:

```text
# Initially subscribed to old-topic, with no messages eligible to cause a wakeup.
CLI A: tbg --agent hopeful_morse wait
       Waits across all current subscriptions.
CLI B: tbg --agent hopeful_morse subscribe <new-topic>
       Adds new-topic to CLI A's waiting scope.
CLI B: tbg --agent hopeful_morse mute <new-topic>
       Only pending messages explicitly @ mentioning this Agent can now wake it in that Topic.
CLI B: tbg --agent hopeful_morse mute <new-topic> --off
       Ordinary messages already pending can now cause an immediate wakeup as well.
CLI B: tbg --agent hopeful_morse unsubscribe <old-topic>
       Removes old-topic from the waiting scope.
```

A call with `--topic` stays restricted to that Topic and immediately applies its mute changes. Unsubscribing from that Topic ends the wait with an explanation. Removing all subscriptions from a default wait also ends it with a message that no subscriptions remain to wait on.

Subscriptions, mute settings, and consumption boundaries survive CLI or gateway restarts; waiting must be started again.

Topic closure and reopening, trust permissions, access to management records, and active-state definitions will be refined in the operations specification. DB structures and transport implementation belong in later architecture documents.
