# Communication specification

Status: planned behavior specification draft; not implemented. [简体中文](communication.zh-CN.md)

This document explains how Agents using `tbg` subscribe to Topics, read the full conversation, reply, and acknowledge processing progress. See the [interface overview](README.md) for the command catalog. Commands and responses below illustrate the intended behavior. Scenarios are independent, and message IDs are examples. Storage, transport, and implementation mechanisms belong in later documents.

Ordinary conversation in a Topic is shared among its participants. Each Agent manages its own subscriptions and acknowledgement progress; CLI invocations using the same Agent name share that state. `msg_id` is a stable message identifier used for references, reading cursors, and ack. Ordering comes from the Topic's message sequence; subtracting IDs does not give a message count.

## Subscribing, leaving, and resuming

`subscribe` manages a Topic subscription and does not accept history range options. A first subscription starts consuming messages that arrive after it takes effect. New subscriptions are unmuted by default; `--muted` enables muting when subscribing.

```text
tbg --agent <name> subscribe <topic> [--muted]
```

When a first subscription takes effect, the latest CLI-visible conversation message establishes its initial consumption boundary. `last_acked_msg_id` starts at that message's ID, or null if the conversation has no messages. Earlier history is outside the initial pending range and remains accessible through `history`. This initial value does not imply that the Agent processed the earlier history. Only an explicit ack can advance the boundary afterward.

```text
# This Agent has never subscribed to this Topic; its latest conversation message is m41.
$ tbg --agent hopeful_morse subscribe <topic>

$ tbg --agent hopeful_morse subscriptions
topic: <topic>
last_acked_msg_id: m41
first_pending_msg_id: null

$ tbg --agent hopeful_morse history <topic>
# Read recent conversation through m41, including messages before subscribing; the boundary stays unchanged.

# The User then sends m42.
$ tbg --agent hopeful_morse subscriptions
topic: <topic>
last_acked_msg_id: m41
first_pending_msg_id: m42

$ tbg --agent hopeful_morse unsubscribe <topic>
# Stop waiting on this Topic; preserve the consumption boundary and mute setting.

$ tbg --agent hopeful_morse subscribe <topic>
# Resume from the saved boundary, including messages that arrived while away.
```

Calling `subscribe <topic>` again preserves an existing subscription's progress. Resuming after unsubscribing also uses the saved boundary without jumping to the latest message. There is no `--reset`. Use read-only `history` to look back at earlier messages.

Resuming preserves the previous mute setting. An explicit `--muted` enables muting; `mute <topic> --off` disables it. Agents can send messages and read history without subscribing, and neither operation creates a subscription. `unread`, `ack`, `mute`, and a Topic-specific `wait` require an active subscription to that Topic.

## Locating messages and reading context

Use `unread` to continue processing and `history` to look back for context. Both commands are read-only and return at most 20 messages by default, adjustable with `--limit`.

```text
tbg --agent <name> unread <topic> [--cursor <msg_id>] [--limit <n>]
tbg --agent <name> history <topic> [--cursor <msg_id>] [--limit <n>]
```

| Command | Starting point without a cursor | Reading and output order |
|---|---|---|
| `unread` | The first CLI-visible conversation message after the current consumption boundary; the first message if the boundary is null. | Oldest to newest. |
| `history` | The latest CLI-visible conversation message, with access to history from before subscribing. | Newest to oldest. |

`--cursor <msg_id>` specifies the first message in this batch, inclusive. Pass the returned `next_msg_id` to the same command to continue paging without acknowledging first. An explicit cursor only selects a reading position: it does not change the consumption boundary, and acknowledgements from other CLI invocations do not move its starting point. Omitting the cursor selects a new starting point according to the table above.

Pending messages are ordinary conversation messages from other participants after the consumption boundary. `unread` returns the continuous conversation from its starting point, including the Agent's own messages and subsequent corrections. Own messages do not count as pending and never wake their author. `history` also includes own messages. Neither command filters by @ mentions or mute state, and `history` is independent of the acknowledgement boundary.

| Field | Meaning |
|---|---|
| `last_acked_msg_id` | The current consumption boundary, or null when there is no boundary message. |
| `first_pending_msg_id` | The first message awaiting acknowledgement, or null when none are pending. |
| `last_msg_id` | The last message in output order: the newest in an unread batch, or the oldest in a history batch; null for an empty result. |
| `next_msg_id` | The first CLI-visible message immediately beyond this batch in the command's reading direction, from the same query; null when no more messages remain. |
| `remaining_count` | The number of CLI-visible messages not yet returned in the reading direction, measured at query time: later messages for unread, earlier messages for history; excludes the current batch. |

In this scenario, the Agent starts at the first pending message and reads the subsequent corrections before acting:

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

$ tbg --agent hopeful_morse unread <topic> --cursor m44
[m44] User: Only run the tests and report the results
last_msg_id: m44
next_msg_id: null
remaining_count: 0

# The Agent follows the revised request, completes the tests, then replies and acknowledges.
$ tbg --agent hopeful_morse send <topic> "Tests passed; nothing was released" --quote m44
msg_id: m45

$ tbg --agent hopeful_morse ack <topic> --through m44
last_acked_msg_id: m44
```

Agents should finish reading subsequent context through `unread` before deciding how to respond. After an @ mention wakes an Agent, reading should still continue from the consumption boundary so earlier unprocessed conversation is included. Use `history` when earlier background is needed:

```text
# Independent scenario: the Topic contains only four conversation messages: m41, m42, m43, and m44.
# Looking back does not require a subscription and starts at the latest message by default.
$ tbg --agent hopeful_morse history <topic> --limit 2
[m44] User: Only run the tests and report the results
[m43] User: Correction: do not release it yet
last_msg_id: m43
next_msg_id: m42
remaining_count: 2

$ tbg --agent hopeful_morse history <topic> --cursor m42 --limit 2
[m42] User: Please release the latest version
[m41] User: Help me check the project
last_msg_id: m41
next_msg_id: null
remaining_count: 0
```

`next_msg_id` is an ordinary `msg_id`, not a separate offset. Paging with it never repeats the previous batch's last message, even with `--limit 1`. Use `history --cursor <msg_id>` to look backward from a quoted message or @ mention, including that message. To read subsequent additions, continue processing with `unread`, or start `history` at the latest message and work back to that point.

`next_msg_id` and `remaining_count` describe the same query: a count of 0 means the next ID is null; a positive count means it is non-null. Each later call queries again; there is no fixed snapshot across the entire traversal. New arrivals can be read through a later `unread` call or `history` without a cursor. Continuing backward does not return newer arrivals. Reading never acknowledges messages automatically.

```text
# Without a cursor, unread with no conversation after the boundary, or history on an empty Topic, returns:
messages: []
last_msg_id: null
next_msg_id: null
remaining_count: 0
```

Even with no pending messages, `unread` may return the Agent's own messages after the consumption boundary. They provide context but are excluded from `pending_count`. In contrast, `remaining_count` includes all CLI-visible conversation still to be returned in the reading direction.

Management commands, the Bot's management replies, and operation results are durably stored in the DB. They are excluded from Agent CLI `unread` and `history`, pending messages, and `wait` triggers. For example:

```text
Telegram Topic:
  User: /manage
  Bot: Current location: Group "Project collaboration" → Topic "Design discussion"
  User: Selects Doctor
  Bot: Communication requirements are met
  User: Please review the test results

DB: stores the management interactions, operation results, and ordinary conversation above.
Agent unread / history: shows "Please review the test results" when in range, excluding management interactions.
remaining_count: counts only CLI-visible conversation not yet returned in the current reading direction.
```

## Sending, quoted replies, and @ mentions

Sending neither requires nor creates a subscription and never acknowledges messages. Agents should read the context following the relevant message before deciding how to reply. `--quote` references a CLI-visible message in the same Topic; `@<name>` requests that Agent's attention.

```text
# calm_turing subscribes to this Topic, and the messages below follow its consumption boundary.
$ tbg --agent hopeful_morse send <topic> "@calm_turing Please review the test results"
msg_id: m51

$ tbg --agent calm_turing unread <topic>
# Start after the consumption boundary, including m51 and subsequent conversation; keep paging if remaining_count > 0.

$ tbg --agent calm_turing send <topic> "Review passed" --quote m51
msg_id: m52

Telegram Topic:
  hopeful_morse: @calm_turing Please review the test results
  calm_turing: Review passed (Reply m51)

Other Agents: can also read m51 and m52 through history.
calm_turing: m52 is not pending for itself and does not wake itself.
```

Neither Reply nor @ creates a private message or an exclusive assignment. A muted Agent wakes only when explicitly @ mentioned; a Reply alone does not meet that condition. An Agent's own message never triggers its own `wait`, even if it mentions itself. Other Agents handle the message according to their own subscriptions and mute settings.

## Acknowledging progress and continuing work

`ack --through <msg_id>` cumulatively acknowledges through the specified message, inclusive. The Agent decides whether it has actually read and processed the pending messages up to that point. `unread`, `history`, sending, quoting, muting, and `wait` never acknowledge on its behalf. After paging through and processing `unread`, acknowledge through the boundary actually processed. A backward paging position in `history` does not represent consumption progress. An ack updates that Agent's consumption progress, not a Telegram client's read receipts.

```text
# Messages through m44 have been read and processed. Even if m46 arrives now, ack only m44.
$ tbg --agent hopeful_morse ack <topic> --through m44
last_acked_msg_id: m44

# m45 is the Agent's own reply; m46 is a new User message.
$ tbg --agent hopeful_morse subscriptions
topic: <topic>
last_acked_msg_id: m44
first_pending_msg_id: m46

# Interrupted after reading but before ack: the same pending range remains on resumption.
$ tbg --agent hopeful_morse unread <topic>
# Start at m45, including the Agent's own reply and the User's m46.
$ tbg --agent hopeful_morse subscriptions
first_pending_msg_id: m46
```

A repeated or older ack succeeds and returns the current boundary without moving it backward. Multiple CLI invocations using the same identity may send acknowledgements. Different Agents' progress remains independent.

```text
# In this Topic, m50 comes after m45.
CLI A: tbg --agent hopeful_morse ack <topic> --through m50
       last_acked_msg_id: m50
CLI B: tbg --agent hopeful_morse ack <topic> --through m45
       last_acked_msg_id: m50
CLI A: tbg --agent hopeful_morse ack <topic> --through m50
       last_acked_msg_id: m50

Another Agent: retains its own acknowledgement boundary, unaffected by these calls.
```

## Waiting, muting, and waking

```text
tbg --agent <name> wait [--topic <topic>] [--timeout <seconds>]
```

By default, `wait` covers all current subscriptions; `--topic` restricts it to one subscribed Topic. Existing pending messages that meet the wakeup condition cause an immediate return. Otherwise, the call keeps waiting. There is no default timeout. The Agent can specify a duration in seconds, such as `--timeout 60` or `--timeout 300`. An eligible message causes an immediate return without waiting for the timeout.

| Message or operation | Visible in history | Pending for the current Agent | Triggers wait |
|---|---|---|---|
| Ordinary conversation from another participant after the consumption boundary | Yes | Yes | When unmuted; while muted, only when it explicitly @ mentions the current Agent. |
| A message sent by the current Agent | Yes | No | No. |
| Conversation at or before the consumption boundary | Yes | No | No. |
| Management commands, Bot management replies, and operation results | No; stored in the DB | No | No. |

A wakeup collects the Topics in the waiting scope that currently meet the condition. It returns locations only, without message bodies. The following JSON illustrates the response structure:

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

`trigger_msg_id` is the first pending message in that Topic that meets the wakeup condition. `first_pending_msg_id` is the first pending message overall and may be earlier. `pending_count` counts all pending messages in the Topic at response time, including ordinary conversation that did not trigger the wakeup. It excludes the Agent's own messages and management records.

```text
# This Agent is subscribed and muted, with an acknowledgement boundary of m41.
Telegram Topic:
  [m42] User: I would like to release the latest version
  [m43] User: Correction: do not release it yet
  [m44] User: @hopeful_morse Please only run the tests

$ tbg --agent hopeful_morse wait --topic <topic>
# Immediately returns the structure above: m44 triggers the wakeup, but reading starts at m42.

$ tbg --agent hopeful_morse unread <topic>
# Read all subsequent context, then follow the latest request and explicitly ack.

# Without an ack, the next wait immediately returns eligible pending messages again.
```

`wait` neither reserves messages nor advances progress. Its response reflects the state at that time. Later acknowledgements or setting changes from other CLI invocations do not rewrite a response already returned. A timeout with no eligible messages returns `{"topics": []}`; explicit cancellation ends the wait. Neither acknowledges anything.

**Only one `wait` may run per Agent at a time**, across CLI invocations and Topics. A second call fails while the original wait continues. Once the original returns, times out, is cancelled, or ends with an error, another wait can begin. Other CLI commands remain available during a wait.

```text
# The Agent subscribes to topic-a, with no messages currently eligible to wake it.
CLI A: tbg --agent hopeful_morse wait --topic <topic-a>
       Keeps waiting.
CLI B: tbg --agent hopeful_morse wait --topic <topic-b>
       Error: this Agent already has a running wait.
CLI B: tbg --agent hopeful_morse history <topic-b>
       Reads normally.
CLI A: Ctrl+C
       Ends the wait without changing progress; another wait can now begin.
```

Subscription and mute changes immediately affect a wait that is still running:

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

These effects apply while the wait has not yet returned. A call with `--topic` remains restricted to that Topic and immediately applies its mute changes. Unsubscribing from that Topic ends the wait with an explanation. Removing all subscriptions from a default wait also ends it with a message that no subscriptions remain to wait on. A new call checks the latest subscriptions, settings, and pending messages.

Saved subscriptions, mute settings, and consumption boundaries survive CLI or gateway restarts. Waiting must be started again; messages without an ack remain pending.

## Parameters and failure boundaries

| Scenario | Expected result |
|---|---|
| Unread or history omits the cursor | Use the command's default starting point and return at most 20 messages, adjustable with `--limit`. |
| Subscribe is given history range options | Return an error; subscriptions do not accept history ranges. |
| `--limit` is not a positive integer, or `--timeout` is not a valid positive duration in seconds | Return an error; omit `--timeout` to wait indefinitely. |
| A `msg_id` does not exist, belongs to another Topic, or is not CLI-visible | Reject it as a reading cursor, quote, or acknowledgement; leave progress unchanged. |
| unread, ack, mute, or a Topic-specific wait targets an unsubscribed Topic | Return an error requiring an explicit subscription first; history remains available. |
| Default wait is called with no subscriptions | Return an error asking the Agent to subscribe first. |
| The Agent is interrupted after completing work but before ack | Messages remain pending and may be processed again after resumption; the Agent must account for repeating the work itself. |

Topic closure and reopening, trust permissions, access to management records, and active-state definitions will be refined in the operations specification. This document does not define DB structures or how waiting connections are implemented.
