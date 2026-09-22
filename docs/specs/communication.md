# Communication specification

Status: planned behavior specification draft; not implemented. [简体中文](communication.zh-CN.md)

This document explains how Agents subscribe to Topics, read conversations, reply, and acknowledge processing progress. See the [interface overview](README.md) for the command catalog. Scenarios are independent, and message IDs are examples.

Ordinary conversation in a Topic is shared among its participants. Each Agent has independent subscriptions and acknowledgement progress; CLI invocations using the same Agent name share that state. `msg_id` is a stable identifier shared by references, reading cursors, and ack. Ordering comes from the Topic's message sequence; subtracting IDs does not give a message count.

A `msg_id` used as a reading cursor, `--quote`, or ack must exist in the specified Topic and be CLI-visible. Otherwise, the operation is rejected and progress stays unchanged.

## Subscribing, leaving, and resuming

A first `subscribe` starts consuming messages that arrive after it takes effect. New subscriptions are unmuted by default; `--muted` enables muting when subscribing.

The first subscription initializes `last_acked_msg_id` to the latest CLI-visible conversation message, or null if there are no messages. This consumption boundary selects a starting point; it does not mean earlier history has been processed. Conversation from before subscribing remains accessible through `history`.

```text
# First subscription; the latest conversation message is m41.
$ tbg --agent hopeful_morse subscribe <topic>
$ tbg --agent hopeful_morse subscriptions
topic: <topic>
last_acked_msg_id: m41
first_pending_msg_id: null

$ tbg --agent hopeful_morse unsubscribe <topic>
# The User sends m42 while the Agent is unsubscribed.
$ tbg --agent hopeful_morse subscribe <topic>
$ tbg --agent hopeful_morse subscriptions
topic: <topic>
last_acked_msg_id: m41
first_pending_msg_id: m42
```

Repeating a subscription or resuming after unsubscribing preserves the consumption boundary and mute setting. An explicit `--muted` enables muting; `mute <topic> --off` disables it.

The send and history commands neither require nor create a subscription. Unread, ack, mute, and a Topic-specific wait require an active subscription to that Topic and fail without one.

## Locating messages and reading context

Use `unread` to continue processing and `history` to look back for context. Both commands are read-only and return at most 20 messages by default, adjustable with `--limit`.

| Command | Starting point without a cursor | Reading and output order |
|---|---|---|
| `unread` | The first CLI-visible conversation message after the current consumption boundary; the first message if the boundary is null. | Oldest to newest. |
| `history` | The latest CLI-visible conversation message, with access to history from before subscribing. | Newest to oldest. |

`--cursor <msg_id>` specifies the first message in this batch, inclusive. Pass the returned `next_cursor` as `--cursor` to the same command to continue paging without acknowledging first. An explicit cursor does not change the consumption boundary, and acknowledgements from other CLI invocations do not move its starting point. Omitting the cursor selects the default starting point again.

Pending messages are ordinary conversation from other participants after the consumption boundary. Unread and history return the continuous conversation within their reading range, including the Agent's own messages, without filtering by @ mentions or mute state. Own messages are not pending, so unread may return them even when there are no pending messages.

| Field | Meaning |
|---|---|
| `last_acked_msg_id` | The current consumption boundary, or null when there is no boundary message. |
| `first_pending_msg_id` | The first message awaiting acknowledgement, or null when none are pending. |
| `next_cursor` | The starting point for the next call to the same command: an ordinary `msg_id` identifying the first CLI-visible message not yet returned in the reading direction; null when no more messages remain. |
| `remaining_count` | The number of CLI-visible messages not yet returned in the reading direction, measured at query time: later messages for unread, earlier messages for history; excludes the current batch. |

Agents should read the subsequent conversation before acting or replying so they do not miss corrections:

```text
# Subscribed, with a consumption boundary of m41; the User then sends m42, m43, and m44.
$ tbg --agent hopeful_morse unread <topic> --limit 2
[m42] User: Please release the latest version
[m43] User: Correction: do not release it yet
next_cursor: m44
remaining_count: 1

$ tbg --agent hopeful_morse unread <topic> --cursor m44
[m44] User: Only run the tests and report the results
next_cursor: null
remaining_count: 0

# Follow the revised request, complete the tests, then reply and acknowledge.
$ tbg --agent hopeful_morse send <topic> "Tests passed; nothing was released" --quote m44
msg_id: m45
$ tbg --agent hopeful_morse ack <topic> --through m44
last_acked_msg_id: m44
```

Use history to read backward when earlier background is needed:

```text
# Independent scenario: the Topic contains only four conversation messages: m41, m42, m43, and m44.
$ tbg --agent hopeful_morse history <topic> --limit 2
[m44] User: Only run the tests and report the results
[m43] User: Correction: do not release it yet
next_cursor: m42
remaining_count: 2

$ tbg --agent hopeful_morse history <topic> --cursor m42 --limit 2
[m42] User: Please release the latest version
[m41] User: Help me check the project
next_cursor: null
remaining_count: 0
```

`next_cursor` and `remaining_count` describe the same query: a count of 0 means the cursor is null; otherwise it is non-null. Each call queries again, without a fixed snapshot across the traversal. New arrivals can be read through a later unread call or history without a cursor; continuing backward does not return newer arrivals.

```text
# Without a cursor, unread with no conversation after the boundary, or history on an empty Topic, returns:
messages: []
next_cursor: null
remaining_count: 0
```

Management interactions (commands, menu selections, the Bot's management replies, and operation results) are durably stored in the DB. They are excluded from unread, history, pending messages, and remaining_count, and do not trigger wait.

## Sending, quoted replies, and @ mentions

In send, `--quote` identifies the message being replied to, while `@<name>` in the body requests that Agent's attention.

```text
# calm_turing is subscribed, and the messages below follow its consumption boundary.
$ tbg --agent hopeful_morse send <topic> "@calm_turing Please review the test results"
msg_id: m51

$ tbg --agent calm_turing unread <topic>
# Read the subsequent conversation before replying.
$ tbg --agent calm_turing send <topic> "Review passed" --quote m51
msg_id: m52

Telegram Topic:
  hopeful_morse: @calm_turing Please review the test results
  calm_turing: Review passed (Reply m51)
```

Neither Reply nor @ creates a private message or an exclusive assignment; other Agents can still read the same conversation.

## Acknowledging progress and continuing work

`ack --through <msg_id>` cumulatively acknowledges through the specified message, inclusive. The Agent decides whether it has actually read and processed the pending messages up to that point. Only explicit ack advances the consumption boundary; reading, sending, quoting, muting, and wait never acknowledge on its behalf. `next_cursor` does not represent processing progress, and ack does not represent a Telegram client's read receipts.

```text
# Processed through m44; m45 is the Agent's own reply and m46 is a new User message.
$ tbg --agent hopeful_morse ack <topic> --through m44
last_acked_msg_id: m44

$ tbg --agent hopeful_morse unread <topic>
# Start at m45, including the Agent's own reply and the User's m46.
# Interrupted after reading but before ack: the consumption boundary stays unchanged.
$ tbg --agent hopeful_morse subscriptions
topic: <topic>
last_acked_msg_id: m44
first_pending_msg_id: m46
```

A repeated or older ack succeeds and returns the current boundary without moving it backward. Multiple CLI invocations using the same identity may send acknowledgements:

```text
# In this Topic, m50 comes after m45.
CLI A: tbg --agent hopeful_morse ack <topic> --through m50
       last_acked_msg_id: m50
CLI B: tbg --agent hopeful_morse ack <topic> --through m45
       last_acked_msg_id: m50
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
      "first_pending_msg_id": "m42",
      "pending_count": 3
    }
  ]
}
```

`trigger_msg_id` is the first pending message in that Topic that meets the wakeup condition. `first_pending_msg_id` is the first pending message overall and may be earlier. `pending_count` counts all pending messages in the Topic, including conversation that did not trigger the wakeup. It excludes the Agent's own messages and management records.

```text
# Subscribed and muted, with a consumption boundary of m41.
Telegram Topic:
  [m42] User: I would like to release the latest version
  [m43] User: Correction: do not release it yet
  [m44] User: @hopeful_morse Please only run the tests

$ tbg --agent hopeful_morse wait --topic <topic>
# Returns the structure above: m44 triggers the wakeup, but reading starts at m42.
$ tbg --agent hopeful_morse unread <topic>
# Read the subsequent conversation, then follow the latest request and explicitly ack.
# Without an ack, the next wait immediately returns eligible pending messages again.
```

Wait does not reserve messages. Its response reflects the state at that time; later acknowledgements or setting changes from other CLI invocations do not rewrite it. A timeout with no eligible messages returns `{"topics": []}`; explicit cancellation ends the wait.

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
