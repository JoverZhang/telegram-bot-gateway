# Agent buttons and message edits

Status: interface contract; current implementation coverage is listed in the [README](../../README.md). [简体中文](buttons.zh-CN.md)

An Agent can attach buttons to a message, poll for button presses, and edit that same message to show another page. The Gateway supplies transport, ownership checks, and durable callback storage. The application owns page state, user/session authorization, and business actions. This document covers those primitives; it does not supply an application polling loop or an integration with a data source.

## Attach an inline keyboard

`send` accepts optional `--reply-markup '<JSON>'`. The value is a JSON object with an `inline_keyboard` array of button rows:

```json
{
  "inline_keyboard": [
    [
      {"text": "Next", "callback_data": "pg_7f2a"},
      {"text": "Help", "url": "https://example.com/help"}
    ]
  ]
}
```

Each button requires nonblank `text` and exactly one supported action: `callback_data` or `url`. Callback data must occupy 1–64 UTF-8 bytes; this is a byte limit, not a character limit. URL buttons accept `http://`, `https://`, or `tg://` URLs. Other Telegram button types and fields are outside this contract. Invalid markup is rejected before send acceptance. Rows in a nonempty keyboard must contain at least one button. `{"inline_keyboard":[]}` represents no buttons and removes the keyboard when used in an edit. The shape follows Telegram's [InlineKeyboardMarkup](https://core.telegram.org/bots/api#inlinekeyboardmarkup) and [InlineKeyboardButton](https://core.telegram.org/bots/api#inlinekeyboardbutton), with only the subset above exposed by the Gateway.

A send still returns `{"msg_id":"m42"}` after local durable acceptance. Its existing queue, retry, and delivery semantics are unchanged. An edit requires completed delivery and the stored Telegram message mapping; send acceptance alone is insufficient.

## Read button presses separately from conversation

```text
tbg --agent <name> callback list [--topic <topic>] [--cursor <callback_query_id>] [--limit <n>]
```

The command requires a registered Agent and returns only callbacks for messages owned by that Agent. Without `--topic`, it reads across the Agent's Topics. It does not require or create a conversation subscription. Results follow durable receive order, oldest first, with a default limit of 20:

```json
{
  "callbacks": [
    {
      "callback_query_id": "cq-73",
      "topic": "<topic>",
      "msg_id": "m42",
      "user_id": 12345678,
      "data": "pg_7f2a",
      "received_at": "2026-09-30T09:10:00Z"
    }
  ],
  "next_cursor": "cq-73",
  "remaining_count": 0
}
```

`callback_query_id` is Telegram's opaque query identifier, also used to answer the press. `msg_id` identifies the Gateway's original outgoing message; `user_id` is the actor's Telegram ID. `received_at` records Gateway receipt time, while ordering uses the stored receipt sequence.

A cursor excludes the identified callback and reads later entries. `next_cursor` is the last returned query ID; an empty result preserves the supplied cursor, or returns null without one. `remaining_count` counts matching later callbacks at query time. Each call reads current state, without a fixed snapshot. Keep the same Agent and Topic filter when continuing. Neither listing nor answering saves consumer progress: persist the cursor in the application only after processing its batch. An interruption before that save can replay callbacks, so deduplicate business effects by query ID. Duplicate Telegram updates or query IDs do not append another stored callback.

Callbacks do not become conversation messages, appear in `unread` or `history`, trigger `wait`, advance ack, or produce ❤️ receipts. An application must poll `callback list` even if it already waits for conversation.

### Admission and authorization

The Gateway accepts a callback only from an administrator or trusted User in a connected Group, for a known mapped outgoing Agent message, with `data` matching a callback button in that message's currently stored keyboard. Telegram's inaccessible-message references can still be resolved through their chat and message IDs. Inline-mode and game callbacks are unsupported. URL-button presses do not enter this callback stream.

These checks do not authorize a business action. Treat callback data as opaque, untrusted input; the application must validate the actor, Topic, current page/session, and requested action before using it. A stored callback can become stale after an edit. Use short tokens that refer to application state, rather than embedding private records or personal ledger data in buttons. Telegram itself warns that callback data may not match the current message; see [CallbackQuery](https://core.telegram.org/bots/api#callbackquery).

HTTP remains unauthenticated. Agent ownership prevents accidental cross-Agent routing; a caller that can reach the Gateway can still select a registered Agent name. Keep the endpoint local or on a private, controlled network.

## Answer a press promptly

```text
tbg --agent <name> callback answer <callback_query_id> [--text "<text>"] [--show-alert]
```

Only the message's owner Agent can answer its stored callback. Optional text is limited to 200 characters; `--show-alert` requests an alert instead of a notification. Success returns `{"callback_query_id":"cq-73"}`. The Gateway calls Telegram's [answerCallbackQuery](https://core.telegram.org/bots/api#answercallbackquery) synchronously. Call it promptly, even without text, to end the Telegram client's loading indicator. An answer does not consume the callback, edit the message, or acknowledge conversation. Telegram can reject an expired query.

## Update the existing message

```text
tbg --agent <name> edit text <topic> <msg_id> "<content>" \
  [--format plain|markdown] [--no-header] [--reply-markup '<JSON>']
tbg --agent <name> edit markup <topic> <msg_id> --reply-markup '<JSON>'
```

Edits and callback answers require the Gateway to have verified the configured Bot identity after startup; they fail while that readiness check is incomplete. `callback list` can read stored callbacks while Telegram is unavailable.

Both commands return `{"msg_id":"m42"}`. They require the Agent's own already-delivered message in an active, available Topic. Text editing uses the same rendering and length rules as `send`: plain text and the Agent header are the defaults. Repeat `--format markdown` or `--no-header` when needed; the original rendering options are not inferred. Omitting markup on a text edit preserves the stored keyboard. Supplying markup replaces it; an empty `inline_keyboard` clears it. Markup-only editing leaves the body unchanged.

The Gateway uses Telegram's [editMessageText](https://core.telegram.org/bots/api#editmessagetext) or [editMessageReplyMarkup](https://core.telegram.org/bots/api#editmessagereplymarkup). After Telegram success, it updates the stored source text and/or keyboard. The original `msg_id` and conversation position remain unchanged; no new message or ack is created. Agents that already acknowledged the message receive no new pending message. An unacknowledged message remains subject to the normal pending and mention rules, using its updated content.

Edits and callback answers are synchronous and have no durable retry queue. Telegram errors are returned to the caller. Telegram's “message is not modified” response counts as successful editing, allowing an exact retry to reconcile local state. A lost response or crash can leave Telegram updated before local history is saved. Retry the same desired edit only while it is still current; a stale retry can overwrite a newer page. The Gateway serializes edits to keep Telegram updates and stored history in the same order. The application must still serialize its page logic, reject stale sessions, and handle ambiguous outcomes. If a previous send produced external duplicates, editing targets only the Telegram message in the stored mapping.

## Example: show the next page

The page body is shared with everyone who can read the Topic. The opaque tokens below refer to application-owned page/session state; they contain no personal records.

```sh
# Store the returned msg_id; wait for delivery before editing it.
tbg --agent hopeful_morse send <topic> "Page 1 of 2: A, B" \
  --reply-markup '{"inline_keyboard":[[{"text":"Next","callback_data":"pg_7f2a"}]]}'

# Poll independently of conversation wait; this example returns cq-73 for m42.
tbg --agent hopeful_morse callback list --topic <topic> --limit 20
# Validate cq-73's user_id, Topic, token, and session in the application.
tbg --agent hopeful_morse callback answer cq-73

tbg --agent hopeful_morse edit text <topic> m42 "Page 2 of 2: C, D" \
  --reply-markup '{"inline_keyboard":[[{"text":"Previous","callback_data":"pg_91bc"}]]}'
# After processing the batch, save its next_cursor in the application.
tbg --agent hopeful_morse callback list --topic <topic> --cursor cq-73

# Remove the controls when the application session ends.
tbg --agent hopeful_morse edit markup <topic> m42 \
  --reply-markup '{"inline_keyboard":[]}'
```

This example illustrates the CLI contract, not a live Telegram result. Application deployment, authentication, and data-source access remain the caller's responsibility.
