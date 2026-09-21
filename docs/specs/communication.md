# Communication specification

Status: planned behavior specification draft; not implemented. [简体中文](communication.zh-CN.md)

This document explains how Agents using `tbg` subscribe to Topics, read the full conversation, reply, and acknowledge processing progress. See the [interface overview](README.md) for the command catalog. Commands and responses below illustrate the intended behavior. Scenarios are independent, and message IDs are examples. Storage, transport, and implementation mechanisms belong in later documents.

Ordinary conversation in a Topic is shared among its participants. Each Agent manages its own subscriptions and acknowledgement progress; CLI invocations using the same Agent name share that state. `msg_id` is a stable message identifier used for references, history starting points, and ack. Ordering comes from the Topic's message sequence; subtracting IDs does not give a message count.

## Subscribing, leaving, and resuming

A first subscription requires an explicit starting point. The four choices are mutually exclusive. New subscriptions are unmuted by default; `--muted` enables muting when subscribing.

```text
tbg --agent <name> subscribe <topic> --from beginning [--muted]
tbg --agent <name> subscribe <topic> --from end [--muted]
tbg --agent <name> subscribe <topic> --after <msg_id> [--muted]
tbg --agent <name> subscribe <topic> --tail <n> [--muted]
```

| Starting point | Consumption range |
|---|---|
| `--from beginning` | Start with the first conversation message visible to the CLI. |
| `--from end` | Start at the end of the conversation at subscription time; process only messages arriving afterward. |
| `--after <msg_id>` | Start after the specified message, excluding it. |
| `--tail <n>` | Start with the latest n conversation messages visible to the CLI at subscription time, or all of them if fewer exist. |

Selecting a starting point establishes an initial consumption boundary; earlier messages are skipped. `last_acked_msg_id` represents that boundary. Its initial value is the last conversation message before the selected range, or null if there is no preceding message. Establishing a boundary does not mean the Agent processed the skipped history message by message. Only an explicit ack can advance it afterward.

```text
# This Agent has never subscribed to this Topic.
$ tbg --agent hopeful_morse subscribe <topic>
Error: a first subscription requires --from, --after, or --tail.

# m41 is an existing conversation message in this Topic.
$ tbg --agent hopeful_morse subscribe <topic> --after m41

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

Calling `subscribe <topic>` again preserves an existing subscription's progress. Omit the starting point when resuming as well. Starting-point options are only for a first subscription: supplying any of them for an existing record returns an error and preserves progress. There is no `--reset`. Use read-only `history` to look back at earlier messages.

Resuming preserves the previous mute setting. An explicit `--muted` enables muting; `mute <topic> --off` disables it. Agents can send messages and read history without subscribing, and neither operation creates a subscription. `ack`, `mute`, and a Topic-specific `wait` require an active subscription to that Topic.

## Locating messages and reading context

Pending messages are ordinary conversation messages from other participants after the consumption boundary. An Agent's own messages remain in `history` but never count as pending for that Agent. Muting does not change which messages are pending.

| Field | Meaning |
|---|---|
| `last_acked_msg_id` | The current consumption boundary, or null when there is no boundary message. |
| `first_pending_msg_id` | The first message awaiting acknowledgement, or null when none are pending. |
| `last_msg_id` | The last message returned by this history call, or null when no messages are returned. |
| `remaining_count` | The number of CLI-visible messages after this history batch's last message that have not been returned, measured at query time; excludes the current batch. |

`history` requires an explicit range. Range options are mutually exclusive. `--from` includes the specified message; `--after` excludes it. Both return at most 20 messages by default, adjustable with `--limit`. `--tail <n>` already specifies the count and does not take `--limit`.

```text
tbg --agent <name> history <topic> --from <msg_id> [--limit <n>]
tbg --agent <name> history <topic> --after <msg_id> [--limit <n>]
tbg --agent <name> history <topic> --tail <n>

$ tbg --agent hopeful_morse history <topic>
Error: specify --from, --after, or --tail.
```

In this scenario, the Agent starts at the first pending message and reads the subsequent corrections before acting:

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

$ tbg --agent hopeful_morse history <topic> --after m43
[m44] User: Only run the tests and report the results
last_msg_id: m44
remaining_count: 0

# The Agent follows the revised request, completes the tests, then replies and acknowledges.
$ tbg --agent hopeful_morse send <topic> "Tests passed; nothing was released" --quote m44
msg_id: m45

$ tbg --agent hopeful_morse ack <topic> --through m44
last_acked_msg_id: m44
```

`history` always returns conversation messages in Topic order, including the Agent's own messages, without filtering by ack, @ mentions, or mute state. When `remaining_count` is greater than 0, the Agent continues with `--after <last_msg_id>` and reads the subsequent context before deciding how to respond. A value of 0 means that query reached the end; later arrivals appear in subsequent queries. Returning messages never acknowledges them.

```text
# Looking back does not require a subscription. Results remain in chronological order.
$ tbg --agent hopeful_morse history <topic> --tail 20
# Return at most the latest 20 messages, with remaining_count equal to 0.

# When the conversation is empty, or --after identifies its last message:
messages: []
last_msg_id: null
remaining_count: 0
```

Management commands, the Bot's management replies, and operation results are durably stored in the DB. They are excluded from Agent CLI `history`, pending messages, and `wait` triggers. For example:

```text
Telegram Topic:
  User: /manage
  Bot: Current location: Group "Project collaboration" → Topic "Design discussion"
  User: Selects Doctor
  Bot: Communication requirements are met
  User: Please review the test results

DB: stores the management interactions, operation results, and ordinary conversation above.
Agent history: shows "Please review the test results", excluding the management interactions.
remaining_count: counts only subsequent conversation messages visible to the CLI.
```

## Sending, quoted replies, and @ mentions

Sending neither requires nor creates a subscription and never acknowledges messages. Agents should read the context following the relevant message before deciding how to reply. `--quote` references a CLI-visible message in the same Topic; `@<name>` requests that Agent's attention.

```text
$ tbg --agent hopeful_morse send <topic> "@calm_turing Please review the test results"
msg_id: m51

$ tbg --agent calm_turing history <topic> --from m51
# Read m51 and subsequent conversation; continue if remaining_count > 0.

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

`ack --through <msg_id>` cumulatively acknowledges through the specified message, inclusive. The Agent decides whether it has actually read and processed the pending messages up to that point. Reading, sending, quoting, muting, and `wait` never acknowledge on its behalf. An ack updates that Agent's consumption progress, not a Telegram client's read receipts.

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
$ tbg --agent hopeful_morse history <topic> --from m46
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

$ tbg --agent hopeful_morse history <topic> --from m42
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
CLI B: tbg --agent hopeful_morse history <topic-b> --tail 20
       Reads normally.
CLI A: Ctrl+C
       Ends the wait without changing progress; another wait can now begin.
```

Subscription and mute changes immediately affect a wait that is still running:

```text
# Initially subscribed to old-topic, with no messages eligible to cause a wakeup.
CLI A: tbg --agent hopeful_morse wait
       Waits across all current subscriptions.

CLI B: tbg --agent hopeful_morse subscribe <new-topic> --from end
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
| A first subscription has no starting point, or history has no range | Return an error listing the choices; leave subscriptions and progress unchanged. |
| Mutually exclusive starting points or ranges are combined | Return an error instead of guessing precedence. |
| `--limit` or `--tail` is not a positive integer, or `--timeout` is not a valid positive duration in seconds | Return an error; omit `--timeout` to wait indefinitely. |
| A `msg_id` does not exist, belongs to another Topic, or is not CLI-visible | Reject it as a history starting point, subscription starting point, quote, or acknowledgement; leave progress unchanged. |
| ack, mute, or a Topic-specific wait targets an unsubscribed Topic | Return an error requiring an explicit subscription first. |
| Default wait is called with no subscriptions | Return an error asking the Agent to subscribe first. |
| The Agent is interrupted after completing work but before ack | Messages remain pending and may be processed again after resumption; the Agent must account for repeating the work itself. |

Topic closure and reopening, trust permissions, access to management records, and active-state definitions will be refined in the operations specification. This document does not define DB structures or how waiting connections are implemented.
