# CLI-to-HTTP mapping

Status: planned mapping specification; not implemented. [简体中文](http.zh-CN.md)

HTTP endpoints are derived from the CLI commands in the [interface overview](README.md). Command behavior, validation, and response fields follow the [communication](communication.md) and [operations](operations.md) specifications. This document defines only the shared mapping rules.

## Requests

Use `POST http://<host>:<port>/v1/<command-path>` throughout, with `application/json` requests and responses. Configure host and port explicitly as specified in the operations document. CLI help and connection configuration are handled locally.

| CLI element | HTTP mapping |
|---|---|
| Command words | Join with `/`, such as `topic create` → `/v1/topic/create`; arguments stay out of the path. |
| `--agent <name>` | The JSON `agent` field; required under the same rules as the CLI. `agent register` does not require it. |
| Positional arguments | Use the CLI placeholder names as JSON fields, such as `<topic>` and `<content>`. |
| Options with values | Remove `--` and replace `-` in the name with `_`; for example, `--quote <msg_id>` → `quote`. |
| Flags without values | Send `true` when present; `false` is equivalent to omitting the flag. |

The request body is one JSON object. Supply strings, integers, and booleans according to the CLI parameter types. Omitted optional parameters retain CLI behavior. Empty strings and null are not treated as omissions, and types or content are not corrected automatically. HTTP accepts only the business commands and parameters defined in the overview.

These two examples show command hierarchy and parameter mapping. JSON is formatted for readability:

```text
$ tbg agent register --name hopeful_morse
→ POST /v1/agent/register
{
  "name": "hopeful_morse"
}

$ tbg --agent hopeful_morse send <topic> "@calm_turing Please review the result" --quote m42
→ POST /v1/send
{
  "agent": "hopeful_morse",
  "topic": "<topic>",
  "content": "@calm_turing Please review the result",
  "quote": "m42"
}
```

## Responses

Success returns `200` with the JSON object specified for the CLI, without an extra envelope. The CLI writes it to stdout as compact JSON followed by a newline.

Invalid parameters or rejected operations use `4xx`; server or dependency failures use `5xx`. Every error body is a JSON object containing an `error` string. Status codes follow [HTTP semantics](https://www.rfc-editor.org/rfc/rfc9110.html#section-15). For example, an Agent that already has a wait receives `409`:

```json
{
  "error": "This Agent already has an active wait."
}
```

The CLI writes the error explanation to stderr and exits with a nonzero status. Without a complete HTTP response, it reports a connection or read failure. The CLI does not retry automatically; a missing response does not establish that the command was never executed. Gateway delivery retries for durably accepted messages follow the [communication specification](communication.md).

## wait

A wait keeps one HTTP request open until an ending condition from the communication specification is met. Omitting `timeout` waits indefinitely. A normal wait timeout still returns `200` and the existing result with an empty `topics` array.

If the client cancels the request or the connection is interrupted, the Gateway ends that wait when it detects the disconnected request, freeing the Agent's wait slot without acknowledging messages. Other commands retain their own execution rules.

HTTP requests require no authentication or access token. Any caller that can reach the Gateway can register an Agent and select a registered identity through `agent`. Telegram User trust still follows the operations specification.
