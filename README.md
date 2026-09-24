# telegram-bot-gateway

[简体中文](README.zh-CN.md) · [Intent](docs/intent/README.md) · [Specifications](docs/specs/README.md)

A persistent Telegram Topic gateway for Agents. `tbg` sends and reads through HTTP; the Gateway retains conversation, independent subscriptions and acknowledgements in SQLite. A User participates through Telegram.

The first end-to-end flow is implemented: administrator initialization, Group connection, Agent registration, Topic creation/closure/reopening, send, subscriptions, unread/history, wait, ack and Bot ❤️ receipts. `/manage` currently connects a Group and reports its scope. Full button menus, ordinary User trust management, Doctor views and saved language preferences remain planned. For now, configure participating Users as administrators; do not edit the database to grant trust.

## Run on a Linux PC

Requires Docker with Compose. Build/install the CLI with Rust 1.95 or later:

```sh
cargo install --path . --locked --bin tbg
mkdir -p ~/.config/tbg
cp config/client.example.yaml ~/.config/tbg/client.yaml
cp config/server.example.yaml ~/.config/tbg/server.yaml
```

Edit both files. Set the Bot token in `server.yaml` and choose an explicit listening address and port. Set the same reachable IP and port in `client.yaml`. `18473` is an example, never a default. The Compose service uses Linux host networking, so `listen` is the host address; it mounts the Server file read-only and the complete `/var/lib/tbg` data directory as a named volume. The example binds localhost. HTTP has no authentication; any caller that can reach it can select a registered Agent identity.

```sh
docker compose up -d --build
docker compose logs -f gateway
```

In a private chat with the Bot, send `/whoami`. Add the returned numeric `user_id` to `admins` in `server.yaml`, then restart:

```sh
docker compose restart gateway
```

Create a Telegram Group with Topics enabled. Add the Bot as an administrator with Manage Topics permission, then enter `/manage` in the Group as a configured Gateway administrator. Existing webhooks are not deleted automatically: this Gateway requires polling access and logs a conflict if a webhook is configured. Only one running Gateway may own a data directory, and that directory is pinned to one Bot identity.

## Use the CLI

Success is one compact JSON object plus a newline; use `jq .` for display. Errors go to stderr with a nonzero exit status. Register once; subsequent processes reuse the same Agent name.

```sh
tbg agent register --name hopeful_morse | jq .
tbg --agent hopeful_morse group list | jq .
tbg --agent hopeful_morse topic create --group <group> --name "Discussion" | jq .
tbg --agent hopeful_morse subscribe <topic>
tbg --agent hopeful_morse send <topic> "Ready to collaborate"
tbg --agent hopeful_morse wait | jq .
tbg --agent hopeful_morse unread <topic> --limit 20 | jq .
# Read every remaining page with --cursor <next_cursor> before responding.
tbg --agent hopeful_morse send <topic> "Completed" --quote <msg_id>
tbg --agent hopeful_morse ack <topic> --through <msg_id>
tbg --agent hopeful_morse history <topic> --limit 20 | jq .
```

`unread` moves forward from the saved ack; `history` moves backward from the latest message. Both return `next_cursor` and `remaining_count`; a cursor is exclusive. Reading does not save progress. Only ack advances it. One wait may run per Agent; no timeout means continuous waiting. A muted subscription wakes only on an explicit `@AgentName`, while all its conversation remains readable.

Send success means the message and delivery task are committed locally. Delivery retries survive restart, including Telegram rate limits. A lost Telegram response can cause an external duplicate; the local `msg_id` stays the same. A second CLI send is a new message, not a retry of the first. Ack and its pending heart receipt commit together; receipt failure never rolls back consumption progress. Delivery failures and retries appear in container logs. Stop the container before copying its data volume for backup; retain the complete directory, including SQLite sidecar files.

Outbound text that exceeds 4096 UTF-16 units including the Agent header is rejected before acceptance. Non-text Telegram messages retain a `[type]` marker, any caption and the accepted raw update; attachment download is not implemented. The general/default Topic is reserved. Other Users' ordinary messages are ignored unless trusted; permitted management exchanges are retained separately from conversation.

## Task-completion notifications

The hook is an external CLI caller. Supply a registered Agent, an existing Topic and the notification JSON; put `tbg` on its PATH. It needs Python 3 and forwards `last-assistant-message` for `agent-turn-complete` events. It does not retry ambiguous sends. Notification errors are reported to stderr but do not fail the originating task.

```sh
./integrations/codex-notify.sh hopeful_morse <topic> \
  '{"type":"agent-turn-complete","last-assistant-message":"Task completed."}'
```

Use the script and its first two arguments as the existing hook command; the caller supplies the JSON argument. Oversized notifications fail visibly rather than being truncated.

## Develop and verify

```sh
cargo fmt --check
cargo clippy --locked --all-targets --features test-support -- -D warnings
cargo build --locked --features test-support
python3 tests/e2e.py
docker build -t telegram-bot-gateway .
```

The E2E suite runs actual CLI/Gateway processes, HTTP and SQLite against a controllable Telegram HTTP server. It covers independent consumption, cursors, mention/mute rules, wait cancellation, admission, restart recovery, 429 responses, lost send responses and rejected receipts. `target/e2e/` contains the report, logs and test database. These are simulated integration results, not a live Telegram check. The `test-support` feature enables isolated configuration/API overrides only for tests; container builds omit it.

Native `tbg-gateway` reads `~/.config/tbg/server.yaml`. Without `data_dir`, it stores data in `~/.local/share/tbg`. Server configuration changes require restart; each CLI invocation reloads Client configuration. Implementation plans and capability boundaries are tracked in [Issue #7](https://github.com/JoverZhang/telegram-bot-gateway/issues/7).
