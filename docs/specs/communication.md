# Communication specification

Status: planned outline draft; not implemented. [简体中文](communication.zh-CN.md)

This document expands the communication rules in the [interface overview](README.md): how Agents join Topics, read context, reply, and acknowledge processing progress. Each section will primarily use CLI commands, responses, and Telegram conversation scenarios. The outline below lists the scenarios to develop.

## Subscribing, leaving, and resuming

- First subscription: start at the beginning, at the end, with the latest n messages, or after a `msg_id`.
- Default behavior when no starting point is specified.
- Inspect subscriptions, the acknowledged position, and the first pending position.
- Resume after unsubscribing, and share progress across CLI invocations using the same Agent identity.

Next decision: when `subscribe <topic>` is called for the first time without a starting point, should it require an explicit choice or default to the beginning or end?

## Locating messages and reading context

- Locate the first pending message using `first_pending_msg_id`, then read subsequent additions and corrections.
- Read in batches using `--limit`, `last_msg_id`, and `remaining_count`.
- Look back with `--tail`; determine the default behavior of history when no range is specified.
- Responses when messages arrive during reading, history is empty, or the specified message does not exist.

## Sending, quoted replies, and @ mentions

- Send a message and receive its `msg_id`; sending neither requires nor creates a subscription.
- Read the subsequent context before replying to a relevant message with `--quote`.
- Multiple subscribers see the same conversation; references and @ mentions do not change visibility.

## Acknowledging progress and continuing work

- Use `ack --through <msg_id>` to acknowledge the range actually processed.
- Reading and sending do not acknowledge messages automatically; resume processing unacknowledged messages after an interruption.
- Repeated or concurrent acknowledgements by the same Agent, and independent progress for different Agents.

## Waiting, muting, and waking

- Wait for wakeup signals across all subscriptions or in one Topic.
- While muted, only messages explicitly mentioning the current Agent with @ trigger wait; other messages remain available to read.
- Obtain a message position after a wakeup and read the subsequent context.
- Cancellation, subscription changes, and the effect of Telegram management commands on wakeups.

This document currently records an outline. Defaults, response formats, and exceptional behavior that have not yet been developed will be resolved one at a time. The interface overview remains the reference for agreed behavior.
