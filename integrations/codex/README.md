# Codex notifications

[中文](README.zh-CN.md)

A native Codex plugin sends a short notification through `tbg` when a main-session turn stops. It does not read Telegram replies, wake an Agent, report subagents, or claim the entire task is complete. Claude integration is not included.

## Install and verify

Requires Linux/macOS, Python 3, a Codex CLI with `plugin` commands and trusted plugin Hooks (verified with 0.157.1), and the `tbg` CLI from this revision. Build the CLI with `cargo build --release --bin tbg` and install it on PATH. Configure `~/.config/tbg/client.yaml` and prepare an existing, open Topic on your running Gateway.

```sh
./integrations/codex/install.sh --topic <topic_id>
# An explicit executable is also supported:
./integrations/codex/install.sh --topic <topic_id> --tbg /absolute/path/to/tbg
./integrations/codex/install.sh --status
./integrations/codex/install.sh --test
./integrations/codex/install.sh --uninstall
```

The installer copies the bundled marketplace into a stable user data directory, then uses `codex plugin marketplace add` and `codex plugin add`. Reinstall updates the target and plugin without appending duplicate Hooks. It resolves and saves the absolute CLI path; subsequent installs retain that path unless `--tbg` changes it. Other Codex plugins and Hooks are preserved. Installation does not enable global Hook features or bypass Codex's Hook trust review: enable Hooks if disabled, review/trust this plugin in Codex, and start a new thread.

`--status` shows native plugin registration, configuration and a Topic lookup using an existing Session binding. Before the first binding exists, it reports that connectivity is untested. `--test` registers a separate test Agent and sends a test signal; successful output means Gateway acceptance, not confirmed Telegram delivery. Check Telegram yourself. Only a real Codex turn verifies event dispatch and Hook trust.

Uninstall removes the native plugin and its cache. The dedicated marketplace source, configuration, Session bindings, logs and Gateway records remain available for reinstall; it does not remove unrelated configuration. Existing threads may retain loaded Hooks: restart them after uninstall. This plugin never disables the old notification path. Keep that path during a trial and remove only its notification entry after real delivery is verified.

## Behavior and state

```text
Stop → persistent Session/Agent binding → tbg send → durable Gateway outbox → Telegram
```

Each Codex `session_id` gets an automatically registered Agent on first use. Later processes and resumed sessions reuse that binding. Sessions are keyed by a hash, so event input cannot choose filesystem paths. Two simultaneous Hooks for the same Session do not race registration: a busy lock fails visibly instead of delaying Codex. Notifications do not create subscriptions.

```text
Codex turn finished
Project: telegram-bot-gateway
Agent: calm_turing
Session: 019...
```

The template sends project, Agent and Session identifiers, not the final answer or transcript. Project display names are capped at 120 characters. One Stop invocation sends one notification; replayed events can produce duplicates. Registration with an unknown outcome may leave an unused Agent. There is no exactly-once guarantee, automatic send retry or local offline queue.

State lives in `${XDG_DATA_HOME:-~/.local/share}/tbg/codex/`, outside the versioned plugin cache:

```text
config.json           # topic and absolute tbg_path
sessions/             # hashed Session keys, Agent bindings and process locks
notifications.jsonl   # time, outcome, Session, accepted msg_id or error category
marketplace/          # stable plugin installation source
```

Deleting Session state creates new identities on next use. Use separate data directories for separate Gateways; bindings belong to the Gateway where they were registered. No Bot token is stored here. Notification content is not written to the log.

Registration and sending share a three-second budget. The adapter passes the remaining budget as `tbg --request-timeout-ms <positive_integer>`; HTTP request/body reading are bounded in the client. Codex's five-second Hook timeout is a final guard. Errors are logged and the adapter exits successfully with `{}`, without blocking Stop or injecting model instructions. Gateway acceptance makes delivery durable; failure before acceptance can lose a notification. A timed-out request may already have committed, so the plugin does not blindly retry.

Failures include the workflow step and safe diagnostics such as CLI exit/HTTP status. If the log cannot be written, stderr reports that separately; an accepted send remains accepted.

## Verify changes

```sh
cargo build --locked --features test-support
python3 tests/e2e.py
```

The suite exercises the adapter, real CLI, Gateway, SQLite and a Telegram test server. With `codex` on PATH it also installs, reinstalls and removes the real plugin in an isolated Codex configuration. Without Codex, that portion is explicitly reported as untested. Artifacts: `target/e2e/codex-plugin.json` and `codex-notifications.jsonl`. These checks do not launch a model or prove actual Codex Stop dispatch.
