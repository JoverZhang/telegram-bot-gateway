#!/usr/bin/env python3
"""Real processes/HTTP/SQLite; run after cargo build --features test-support."""

import collections, http.server, json, os, pathlib, socket, sqlite3, subprocess, tempfile, threading, time, urllib.request, urllib.error

ROOT = pathlib.Path(__file__).resolve().parents[1]
REPORT = ROOT / "target/e2e"
REPORT.mkdir(parents=True, exist_ok=True)


class Telegram(http.server.BaseHTTPRequestHandler):
    lock = threading.Lock()
    updates = []
    calls = []
    sends = []
    reactions = []
    faults = collections.deque()
    mid = 100
    tid = 10

    def log_message(self, *args):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))))
        method = self.path.rsplit("/", 1)[-1]
        with self.lock:
            self.calls.append((method, body))
            result = True
            status = 200
            envelope = None
            if method == "getMe":
                result = {"id": 999, "is_bot": True, "username": "gateway_test_bot"}
            elif method == "getUpdates":
                Telegram.updates = [
                    u for u in self.updates if u["update_id"] >= body.get("offset", 0)
                ]
                result = list(Telegram.updates)
            elif method == "getWebhookInfo":
                result = {"url": ""}
            elif method == "getChat":
                result = {
                    "id": body["chat_id"],
                    "title": "Test group",
                    "type": "supergroup",
                    "is_forum": True,
                }
            elif method == "getChatMember":
                result = {"status": "administrator", "can_manage_topics": True}
            elif method == "createForumTopic":
                Telegram.tid += 1
                result = {"message_thread_id": self.tid, "name": body["name"]}
                if getattr(Telegram, "lose_create_response", False):
                    self.close_connection = True
                    return
            elif method == "sendMessage":
                mode = self.faults.popleft() if self.faults else None
                if mode == "retry":
                    status = 429
                    envelope = {
                        "ok": False,
                        "error_code": 429,
                        "parameters": {"retry_after": 1},
                        "description": "rate limited",
                    }
                elif mode == "blocked":
                    status = 403
                    envelope = {
                        "ok": False,
                        "error_code": 403,
                        "description": "forbidden",
                    }
                if envelope is None:
                    Telegram.mid += 1
                    self.sends.append(body)
                    result = {
                        "message_id": self.mid,
                        "date": int(time.time()),
                        "chat": {"id": body["chat_id"]},
                    }
                    if mode == "lost":
                        self.close_connection = True
                        return
            elif method == "setMessageReaction":
                if getattr(Telegram, "reject_reactions", False):
                    status = 403
                    envelope = {
                        "ok": False,
                        "error_code": 403,
                        "description": "reactions disabled",
                    }
                else:
                    self.reactions.append(body)
            data = json.dumps(envelope or {"ok": True, "result": result}).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        try:
            self.wfile.write(data)
        except (BrokenPipeError, ConnectionResetError):
            pass


def eventually(fn, timeout=12):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        try:
            v = fn()
            if v:
                return v
        except (OSError, urllib.error.URLError, AssertionError):
            pass
        time.sleep(0.05)
    raise AssertionError("condition did not become true")


def main():
    assert (ROOT / "target/debug/tbg-gateway").exists(), (
        "build the implementation first"
    )
    results = []
    with tempfile.TemporaryDirectory(prefix="tbg-e2e-") as tmp:
        tmp = pathlib.Path(tmp)
        data = tmp / "data"
        cfg = tmp / "config"
        cfg.mkdir()
        stub = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Telegram)
        threading.Thread(target=stub.serve_forever, daemon=True).start()
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        (cfg / "client.yaml").write_text(f'host: "127.0.0.1"\nport: {port}\n')
        (cfg / "server.yaml").write_text(
            f'listen: "127.0.0.1:{port}"\ndata_dir: "{data}"\ntelegram:\n  bot_token: "TEST_SECRET"\nadmins: []\n'
        )
        env = dict(
            os.environ,
            TBG_TEST_CONFIG_DIR=str(cfg),
            TBG_TEST_TELEGRAM_URL=f"http://127.0.0.1:{stub.server_port}",
        )
        logs = open(REPORT / "gateway.log", "w")
        proc = None

        def api(command, **params):
            req = urllib.request.Request(
                f"http://127.0.0.1:{port}/v1/{command}",
                json.dumps(params).encode(),
                {"Content-Type": "application/json"},
            )
            with urllib.request.urlopen(req, timeout=6) as r:
                return json.load(r)

        def cli(*args, ok=True):
            p = subprocess.run(
                [str(ROOT / "target/debug/tbg"), *args],
                env=env,
                text=True,
                capture_output=True,
                timeout=8,
            )
            assert (p.returncode == 0) == ok, (args, p.stdout, p.stderr)
            if ok:
                assert len(p.stdout.splitlines()) == 1 and p.stdout.endswith("\n"), (
                    p.stdout
                )
                return json.loads(p.stdout)
            assert not p.stdout

        def start():
            nonlocal proc
            proc = subprocess.Popen(
                [str(ROOT / "target/debug/tbg-gateway")],
                env=env,
                stdout=logs,
                stderr=logs,
            )
            eventually(socket_ready)

        def socket_ready():
            with socket.create_connection(("127.0.0.1", port), 0.1):
                return True

        def stop():
            if proc and proc.poll() is None:
                proc.terminate()
                proc.wait(timeout=8)

        def update(uid, mid, text, thread=None, kind="message"):
            msg = {
                "message_id": mid,
                "date": int(time.time()),
                "from": {"id": 123},
                "chat": {
                    "id": -100,
                    "type": "supergroup",
                    "is_forum": True,
                    "title": "Test group",
                },
                "text": text,
            }
            if thread:
                msg["message_thread_id"] = thread
            with Telegram.lock:
                Telegram.updates.append({"update_id": uid, kind: msg})

        try:
            start()
            update(0, 1000, "/whoami")
            eventually(
                lambda: any(
                    "user_id: 123" in item.get("text", "") for item in Telegram.sends
                )
            )
            stop()
            server_config = cfg / "server.yaml"
            server_config.write_text(
                server_config.read_text().replace("admins: []", "admins: [123]")
            )
            start()
            duplicate = subprocess.run(
                [str(ROOT / "target/debug/tbg-gateway")],
                env=env,
                capture_output=True,
                text=True,
                timeout=5,
            )
            assert duplicate.returncode and "another Gateway" in duplicate.stderr
            cli("agent", "register", "--name", "alpha")
            cli("agent", "register", "--name", "beta")
            cli("agent", "register", "--name", "", ok=False)
            cli("agent", "register", "--name", "alpha", ok=False)
            update(1, 1, "/manage")
            eventually(lambda: api("group/list", agent="alpha")["groups"])
            topic = cli(
                "--agent",
                "alpha",
                "topic",
                "create",
                "--group",
                "-100",
                "--name",
                "Discussion",
            )["topic"]
            thread = Telegram.tid
            cli("--agent", "alpha", "subscribe", topic)
            cli("--agent", "beta", "subscribe", topic, "--muted")
            update(2, 2, "Please publish", thread)
            update(3, 3, "Correction: only test @beta", thread)
            first = eventually(
                lambda: api("unread", agent="alpha", topic=topic)["messages"]
            )
            eventually(
                lambda: len(api("unread", agent="alpha", topic=topic)["messages"]) == 2
            )
            page = api("unread", agent="alpha", topic=topic, limit=1)
            assert page["remaining_count"] == 1
            later = api(
                "unread", agent="alpha", topic=topic, cursor=page["next_cursor"]
            )
            assert len(later["messages"]) == 1
            end = later["next_cursor"]
            assert (
                api("wait", agent="beta", timeout=1)["topics"][0]["trigger_msg_id"]
                == end
            )
            cli("--agent", "alpha", "ack", topic, "--through", end)
            eventually(lambda: len(Telegram.reactions) >= 2)
            assert len(api("unread", agent="beta", topic=topic)["messages"]) == 2
            assert not api("unread", agent="alpha", topic=topic)["messages"]
            cli("--agent", "beta", "ack", topic, "--through", end)
            hist = cli("--agent", "alpha", "history", topic, "--limit", "1")
            assert hist["messages"][0]["msg_id"] == end
            assert (
                api("history", agent="alpha", topic=topic, cursor=hist["next_cursor"])[
                    "messages"
                ][0]["msg_id"]
                == page["next_cursor"]
            )
            results.append(
                "independent progress, complete context, pagination, mention wake, Bot receipts"
            )
            # Explicitly redeliver an already committed update across a restart.
            stop()
            update(3, 3, "Correction: only test @beta", thread)
            start()
            assert len(api("history", agent="alpha", topic=topic)["messages"]) == 2
            # A long wait holds one Agent slot; aborting the TCP client releases it.
            waiter = subprocess.Popen(
                [str(ROOT / "target/debug/tbg"), "--agent", "alpha", "wait"],
                env=env,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            eventually(
                lambda: any(
                    a["name"] == "alpha" and a["waiting"]
                    for a in api("agent/list", agent="beta")["agents"]
                )
            )
            cli("--agent", "alpha", "wait", "--timeout", "1", ok=False)
            waiter.terminate()
            waiter.wait(timeout=3)
            eventually(
                lambda: (
                    not next(
                        a
                        for a in api("agent/list", agent="beta")["agents"]
                        if a["name"] == "alpha"
                    )["waiting"]
                )
            )
            assert api("wait", agent="alpha", timeout=1) == {"topics": []}
            results.append("single wait, real HTTP disconnect cleanup, timeout")
            # Save a send, receive 429, restart before retry, then lose a successful response.
            with Telegram.lock:
                Telegram.faults.extend(["retry", "lost"])
            sent = cli("--agent", "alpha", "send", topic, "done after restart")[
                "msg_id"
            ]
            eventually(
                lambda: any(
                    m == "sendMessage"
                    and b.get("text", "").endswith("done after restart")
                    for m, b in Telegram.calls
                )
            )
            stop()
            start()
            eventually(
                lambda: (
                    sum(
                        s.get("text", "").endswith("done after restart")
                        for s in Telegram.sends
                    )
                    >= 2
                )
            )
            assert (
                sum(
                    m["msg_id"] == sent
                    for m in api("history", agent="alpha", topic=topic)["messages"]
                )
                == 1
            )
            results.append(
                "durable outbox, 429 retry, restart, uncertain send duplicates with stable local ID"
            )
            Telegram.reject_reactions = True
            update(4, 4, "receipt recovery", thread)
            eventually(
                lambda: any(
                    m["content"] == "receipt recovery"
                    for m in api("unread", agent="alpha", topic=topic)["messages"]
                )
            )
            last = api("history", agent="alpha", topic=topic)["messages"][0]["msg_id"]
            cli("--agent", "alpha", "ack", topic, "--through", last)
            eventually(
                lambda: any(
                    m == "setMessageReaction" and b["message_id"] == 4
                    for m, b in Telegram.calls
                )
            )
            stop()
            Telegram.reject_reactions = False
            start()
            eventually(
                lambda: any(r["message_id"] == 4 for r in Telegram.reactions),
                timeout=20,
            )
            assert not api("unread", agent="alpha", topic=topic)["messages"]
            assert len(api("history", agent="alpha", topic=topic)["messages"]) == 4
            results.append(
                "durable ack with receipt failure/restart recovery; management hidden"
            )
            cli("--agent", "beta", "topic", "close", topic, ok=False)
            cli("--agent", "alpha", "topic", "close", topic)
            cli("--agent", "alpha", "send", topic, "closed", ok=False)
            cli("--agent", "alpha", "topic", "reopen", topic)
            cli("--agent", "alpha", "unsubscribe", topic)
            update(5, 5, "while away", thread)
            eventually(
                lambda: len(api("history", agent="beta", topic=topic)["messages"]) == 5
            )
            cli("--agent", "alpha", "subscribe", topic)
            assert (
                api("unread", agent="alpha", topic=topic)["messages"][0]["content"]
                == "while away"
            )
            results.append("Topic ownership, close/reopen, resume subscription")
            # Untrusted content must not survive even in raw update storage.
            with Telegram.lock:
                Telegram.updates.append(
                    {
                        "update_id": 6,
                        "message": {
                            "message_id": 6,
                            "date": int(time.time()),
                            "from": {"id": 456},
                            "chat": {"id": -100, "type": "supergroup"},
                            "message_thread_id": thread,
                            "text": "UNTRUSTED_SECRET",
                        },
                    }
                )

            def received(update_id):
                with sqlite3.connect(data / "gateway.sqlite") as database:
                    return database.execute(
                        "SELECT COUNT(*) FROM updates WHERE id=?", (update_id,)
                    ).fetchone()[0]

            eventually(lambda: received(6))
            assert "UNTRUSTED_SECRET" not in json.dumps(
                api("history", agent="alpha", topic=topic)
            )
            with sqlite3.connect(data / "gateway.sqlite") as database:
                assert (
                    database.execute("SELECT raw FROM updates WHERE id=6").fetchone()[0]
                    is None
                )
            # A reply without @ does not wake a muted Agent, even with pending messages.
            with Telegram.lock:
                Telegram.updates.append(
                    {
                        "update_id": 7,
                        "message": {
                            "message_id": 7,
                            "date": int(time.time()),
                            "from": {"id": 123},
                            "chat": {"id": -100, "type": "supergroup"},
                            "message_thread_id": thread,
                            "text": "reply without mention",
                            "quote": {
                                "text": "UNTRUSTED_SECRET",
                                "position": 0,
                                "is_manual": False,
                            },
                            "reply_to_message": {
                                "message_id": 6,
                                "from": {"id": 456},
                                "text": "UNTRUSTED_SECRET",
                            },
                        },
                    }
                )
            eventually(lambda: received(7))
            with sqlite3.connect(data / "gateway.sqlite") as database:
                raw = database.execute("SELECT raw FROM updates WHERE id=7").fetchone()[
                    0
                ]
                assert "UNTRUSTED_SECRET" not in raw
                assert json.loads(raw)["message"]["reply_to_message"]["message_id"] == 6

            assert api("wait", agent="beta", topic=topic, timeout=1) == {"topics": []}
            waiter = subprocess.Popen(
                [
                    str(ROOT / "target/debug/tbg"),
                    "--agent",
                    "beta",
                    "wait",
                    "--topic",
                    topic,
                ],
                env=env,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            eventually(
                lambda: next(
                    a
                    for a in api("agent/list", agent="alpha")["agents"]
                    if a["name"] == "beta"
                )["waiting"]
            )
            cli("--agent", "beta", "unsubscribe", topic)
            output, error = waiter.communicate(timeout=5)
            assert (
                waiter.returncode and not output and "not currently subscribed" in error
            )
            cli("--agent", "beta", "subscribe", topic)
            waiter = subprocess.Popen(
                [str(ROOT / "target/debug/tbg"), "--agent", "beta", "wait"],
                env=env,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            eventually(
                lambda: next(
                    a
                    for a in api("agent/list", agent="alpha")["agents"]
                    if a["name"] == "beta"
                )["waiting"]
            )
            cli("--agent", "beta", "mute", topic, "--off")
            output, error = waiter.communicate(timeout=5)
            assert not waiter.returncode and json.loads(output)["topics"]
            # Non-text content remains recognizable and raw accepted input is retained.
            with Telegram.lock:
                Telegram.updates.append(
                    {
                        "update_id": 8,
                        "message": {
                            "message_id": 8,
                            "date": int(time.time()),
                            "from": {"id": 123},
                            "chat": {"id": -100, "type": "supergroup"},
                            "message_thread_id": thread,
                            "photo": [{"file_id": "test-photo"}],
                            "caption": "test image",
                        },
                    }
                )
            eventually(lambda: received(8))
            assert (
                api("history", agent="alpha", topic=topic)["messages"][0]["content"]
                == "[photo] test image"
            )
            before = len(api("history", agent="alpha", topic=topic)["messages"])
            cli("--agent", "alpha", "send", topic, "x" * 4096, ok=False)
            assert len(api("history", agent="alpha", topic=topic)["messages"]) == before
            # The hook uses the public CLI and leaves the caller successful on notification errors.
            hook_env = dict(
                env, PATH=str(ROOT / "target/debug") + os.pathsep + env["PATH"]
            )
            hook = subprocess.run(
                [
                    str(ROOT / "integrations/codex-notify.sh"),
                    "alpha",
                    topic,
                    json.dumps(
                        {
                            "type": "agent-turn-complete",
                            "last-assistant-message": "hook complete",
                        }
                    ),
                ],
                env=hook_env,
                capture_output=True,
                text=True,
                timeout=10,
            )
            assert hook.returncode == 0
            assert (
                api("history", agent="alpha", topic=topic)["messages"][0]["content"]
                == "hook complete"
            )
            results.append(
                "initialization, instance lock, trust isolation, reply-only mute, live wait changes, media retention, hook"
            )

            # A blocked send stalls its Topic, not another Topic; quotes wait for mapping.
            eventually(
                lambda: any(
                    item.get("text", "").endswith("hook complete")
                    for item in Telegram.sends
                )
            )
            with Telegram.lock:
                Telegram.faults.append("blocked")
            blocked = cli("--agent", "alpha", "send", topic, "blocked first")["msg_id"]

            def blocked_saved():
                with sqlite3.connect(data / "gateway.sqlite") as database:
                    return (
                        database.execute(
                            "SELECT status FROM outbox WHERE kind='send' AND message=?",
                            (int(blocked[1:]),),
                        ).fetchone()[0]
                        == "blocked"
                    )

            eventually(blocked_saved)
            cli("--agent", "alpha", "send", topic, "queued quote", "--quote", blocked)
            other = cli(
                "--agent",
                "alpha",
                "topic",
                "create",
                "--group",
                "-100",
                "--name",
                "Independent",
            )["topic"]
            cli("--agent", "alpha", "send", other, "independent Topic")
            eventually(
                lambda: any(
                    item.get("text", "").endswith("independent Topic")
                    for item in Telegram.sends
                )
            )
            assert not any(
                item.get("text", "").endswith("queued quote") for item in Telegram.sends
            )
            stop()
            start()
            quoted = eventually(
                lambda: next(
                    (
                        item
                        for item in Telegram.sends
                        if item.get("text", "").endswith("queued quote")
                    ),
                    None,
                )
            )
            assert quoted["reply_parameters"]["message_id"] > 0
            results.append(
                "per-Topic send ordering, cross-Topic progress, durable quote mapping"
            )
            Telegram.lose_create_response = True
            cli(
                "--agent",
                "alpha",
                "topic",
                "create",
                "--group",
                "-100",
                "--name",
                "Lost create",
                ok=False,
            )
            Telegram.lose_create_response = False
            with sqlite3.connect(data / "gateway.sqlite") as database:
                result = database.execute(
                    "SELECT detail FROM management WHERE kind='topic/create result' ORDER BY id DESC LIMIT 1"
                ).fetchone()[0]
                assert json.loads(result)["outcome_unknown"] is True
            # After a long offline interval Telegram may choose a lower random update ID.
            stop()
            with sqlite3.connect(data / "gateway.sqlite") as database:
                database.execute("UPDATE meta SET value='10001' WHERE key='offset'")
            with Telegram.lock:
                Telegram.updates.clear()
                call_start = len(Telegram.calls)
            start()
            eventually(
                lambda: (
                    len(
                        [
                            body
                            for method, body in Telegram.calls[call_start:]
                            if method == "getUpdates"
                        ]
                    )
                    >= 3
                )
            )
            assert all(
                body.get("offset") is None
                for method, body in Telegram.calls[call_start:]
                if method == "getUpdates"
            )
            update(50, 50, "/home/mira/project is the workspace", thread)
            eventually(lambda: received(50))
            assert (
                api("history", agent="alpha", topic=topic)["messages"][0]["content"]
                == "/home/mira/project is the workspace"
            )
            assert (
                sum(
                    method == "createForumTopic" and body.get("name") == "Lost create"
                    for method, body in Telegram.calls
                )
                == 1
            )
            results.append(
                "empty startup polls preserve reset offset; slash-prefixed conversation is retained"
            )
        finally:
            stop()
            stub.shutdown()
            logs.close()
            if (data / "gateway.sqlite").exists():
                with (
                    sqlite3.connect(data / "gateway.sqlite") as source,
                    sqlite3.connect(REPORT / "gateway.sqlite") as destination,
                ):
                    source.backup(destination)
            assert "TEST_SECRET" not in (REPORT / "gateway.log").read_text()
    (REPORT / "report.json").write_text(
        json.dumps({"passed": results}, indent=2) + "\n"
    )
    print(json.dumps({"passed": len(results), "report": str(REPORT / "report.json")}))


if __name__ == "__main__":
    main()
