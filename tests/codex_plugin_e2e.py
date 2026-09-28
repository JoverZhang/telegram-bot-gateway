"""Plugin subprocess → real CLI → HTTP → Gateway → Telegram test server.

Failure boundaries: unavailable Gateway, stalled HTTP response, missing config,
malformed event, concurrent Session hooks, stale CLI, and unrelated Codex hooks.
Native Codex install/remove checks run when its CLI is available (no model call).
"""

import fcntl
import hashlib
import http.server
import json
import shutil
import subprocess
import threading
import time
from pathlib import Path


def run(root, tmp, env, topic, api, telegram, eventually, report):
    plugin = root / "integrations/codex"
    script = plugin / "plugins/tbg-notify/scripts/notify.py"
    data = tmp / "plugin-data"
    state = data / "tbg/codex"
    state.mkdir(parents=True)
    config = {"topic": topic, "tbg_path": str(root / "target/debug/tbg")}
    (state / "config.json").write_text(json.dumps(config))
    isolated = dict(env, XDG_DATA_HOME=str(data), CODEX_HOME=str(tmp / "codex"))
    checks = []

    def hook(session="session-one", event="Stop", *, script_path=script, **overrides):
        payload = dict(
            hook_event_name=event,
            session_id=session,
            cwd="/projects/example",
            **overrides,
        )
        result = subprocess.run(
            ["python3", str(script_path)],
            input=json.dumps(payload),
            env=isolated,
            text=True,
            capture_output=True,
            timeout=5,
        )
        assert result.returncode == 0 and json.loads(result.stdout) == {}, result
        return result

    def binding(session):
        key = hashlib.sha256(session.encode()).hexdigest()
        return json.loads((state / f"sessions/{key}.json").read_text())["agent"]

    def logs():
        return [
            json.loads(line)
            for line in (state / "notifications.jsonl").read_text().splitlines()
        ]

    answer = "Completed **successfully**.\n\n- Added tests\n- Updated docs"
    hook(last_assistant_message=answer)
    first = logs()[-1]
    assert first["status"] == "accepted"
    agent = binding("session-one")
    messages = api("history", agent=agent, topic=topic)["messages"]
    delivered = next(m for m in messages if m["msg_id"] == first["msg_id"])
    assert delivered["content"] == answer, "Stop notification dropped the final answer"
    eventually(
        lambda: any(
            item.get("text") == f"{agent}:\n{answer}" for item in telegram.sends
        )
    )
    assert answer not in (state / "notifications.jsonl").read_text()
    bullet_answer = "- Added tests\n- Updated docs"
    hook(last_assistant_message=bullet_answer)
    bullet_receipt = logs()[-1]
    assert bullet_receipt["status"] == "accepted", (
        "Leading bullet was parsed as a CLI option"
    )
    eventually(
        lambda: any(
            item.get("text") == f"{agent}:\n{bullet_answer}" for item in telegram.sends
        )
    )

    # Astral Unicode must respect Telegram's UTF-16 budget including attribution.
    long_answer = "🚀" * 3000
    hook(last_assistant_message=long_answer)
    long_receipt = logs()[-1]
    truncated = next(
        m
        for m in api("history", agent=agent, topic=topic)["messages"]
        if m["msg_id"] == long_receipt["msg_id"]
    )["content"]
    assert truncated.endswith("\n\n… (truncated)")
    assert truncated.startswith("🚀") and "\ufffd" not in truncated
    assert len(f"{agent}:\n{truncated}".encode("utf-16-le")) // 2 <= 4096
    eventually(
        lambda: any(
            item.get("text") == f"{agent}:\n{truncated}" for item in telegram.sends
        )
    )

    hook()  # Another process, same Session.
    assert binding("session-one") == agent
    hook("session-two")
    assert binding("session-two") != agent
    before = len(logs())
    hook(event="SubagentStop")
    assert len(logs()) == before
    checks.append(
        "real CLI delivery, Session persistence/isolation, main Stop only, final answer forwarding without metadata or log disclosure"
    )

    hook(session="")
    assert logs()[-1]["status"] == "failed"
    assert hook(last_assistant_message=None).stderr == ""
    fallback = logs()[-1]
    assert (
        next(
            m
            for m in api("history", agent=agent, topic=topic)["messages"]
            if m["msg_id"] == fallback["msg_id"]
        )["content"]
        == "Codex turn finished (no final response)."
    )

    # Exercise actionable failure diagnostics without disclosing remote response text.
    malformed = subprocess.run(
        ["python3", str(script)],
        input="{",
        env=isolated,
        text=True,
        capture_output=True,
        timeout=5,
    )
    assert malformed.returncode == 0 and json.loads(malformed.stdout) == {}
    assert "parse_event" in malformed.stderr

    config_path = state / "config.json"
    config_path.unlink()
    assert "load_config" in hook().stderr
    assert "load_config" in logs()[-1]["detail"]
    config_path.write_text(json.dumps(config))

    key = hashlib.sha256(b"session-one").hexdigest()
    with (state / f"sessions/{key}.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        assert "lock_session" in hook().stderr
    assert logs()[-1]["status"] == "failed"

    config_path.write_text(json.dumps({**config, "topic": "missing-topic"}))
    assert "send_notification" in hook().stderr
    assert "HTTP" in logs()[-1]["detail"]
    config_path.write_text(json.dumps(config))

    log_path = state / "notifications.jsonl"
    saved_log = state / "saved-notifications.jsonl"
    log_path.rename(saved_log)
    log_path.mkdir()  # Prevent append while leaving Gateway delivery available.
    try:
        result = hook()
        assert "log failed" in result.stderr and "status=accepted" in result.stderr
        assert "notification failed:" not in result.stderr
    finally:
        log_path.rmdir()
        saved_log.rename(log_path)
    checks.append(
        "config/lock/send diagnostics and logging failure without false delivery failure"
    )

    # HTTP stalls after accepting a socket: request cancellation must occur in tbg itself.
    class Stalled(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            time.sleep(4)

    stalled = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Stalled)
    threading.Thread(target=stalled.serve_forever, daemon=True).start()
    cfg = Path(env["TBG_TEST_CONFIG_DIR"]) / "client.yaml"
    previous = cfg.read_text()
    try:
        cfg.write_text(f"host: 127.0.0.1\nport: {stalled.server_port}\n")
        start = time.monotonic()
        result = subprocess.run(
            [
                config["tbg_path"],
                "--request-timeout-ms",
                "150",
                "--agent",
                agent,
                "whoami",
            ],
            env=isolated,
            capture_output=True,
            timeout=2,
        )
        assert result.returncode != 0 and time.monotonic() - start < 2
        start = time.monotonic()
        hook()
        assert time.monotonic() - start < 4 and logs()[-1]["status"] == "failed"
    finally:
        cfg.write_text(previous)
        stalled.shutdown()
    checks.append("HTTP request deadline and nonblocking Hook failure")

    # Real plugin management, with unrelated config and hooks as sentinels.
    if shutil.which("codex"):
        codex_home = Path(isolated["CODEX_HOME"])
        codex_home.mkdir()
        sentinel = {
            "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "true"}]}]}
        }
        hooks_path = codex_home / "hooks.json"
        hooks_path.write_text(json.dumps(sentinel))
        (codex_home / "config.toml").write_text("[features]\nhooks = true\n")

        def install(*args):
            result = subprocess.run(
                [str(plugin / "install.sh"), *args],
                env=isolated,
                capture_output=True,
                text=True,
                timeout=60,
            )
            assert result.returncode == 0, (args, result.stdout, result.stderr)
            return result.stdout

        install("--topic", topic, "--tbg", config["tbg_path"])
        install("--topic", topic)
        assert "tbg-notify@tbg-codex" in install("--status")
        install("--test")
        assert binding("session-one") == agent
        cache = list(
            (codex_home / "plugins/cache/tbg-codex/tbg-notify").glob(
                "*/hooks/hooks.json"
            )
        )
        assert cache and json.loads(cache[0].read_text())["hooks"]["Stop"]
        # Execute the installed copy too: its sibling imports must survive caching.
        hook(script_path=cache[0].parents[1] / "scripts/notify.py")
        assert logs()[-1]["status"] == "accepted"
        assert binding("session-one") == agent
        install("--uninstall")
        result = subprocess.run(
            ["codex", "plugin", "list", "--marketplace", "tbg-codex", "--json"],
            env=isolated,
            capture_output=True,
            text=True,
            check=True,
        )
        assert json.loads(result.stdout)["installed"] == []
        assert json.loads(hooks_path.read_text()) == sentinel
        assert "hooks = true" in (codex_home / "config.toml").read_text()
        assert binding("session-one") == agent
        checks.append(
            "native Codex install/reinstall/status/test/uninstall; unrelated hooks preserved"
        )
    else:
        checks.append("native Codex installation not exercised: codex absent")
    (report / "codex-plugin.json").write_text(
        json.dumps({"passed": checks}, indent=2) + "\n"
    )
    shutil.copyfile(state / "notifications.jsonl", report / "codex-notifications.jsonl")
    return checks
