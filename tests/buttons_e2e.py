#!/usr/bin/env python3
"""Inline keyboard contract: real CLI/Gateway processes, HTTP, and SQLite.

Run: cargo build --locked --features test-support && python3 tests/buttons_e2e.py
Artifacts: target/buttons-e2e/{report.json,telegram-calls.json,gateway.sqlite,gateway.log}
The mock and every request bind/use localhost; no Telegram credentials are required.

Failure-mode checklist (written before implementing the feature):
- Old schema fails to upgrade or preexisting conversation is lost.
- CLI JSON is malformed, or wire types/null/unknown fields bypass validation.
- Empty labels, missing/multiple actions, or UTF-8 callback data outside 1..64 bytes.
- Markup disappears from queued sends, send retries, or restart recovery.
- Callback polling omits callback_query; callbacks leak into conversation/unread/wait.
- Untrusted users, disconnected/wrong chats, unknown or incoming message targets,
  inline-only queries, missing data, and forged/non-current button data are admitted.
- Inaccessible messages (date=0) fail to resolve by chat/message ID.
- Embedded original/replied/quoted message bodies leak into retained callback raw data.
- Redelivered update IDs or repeated callback query IDs create duplicate events.
- Callback ordering/cursors leak another owner/topic or reads consume progress.
- Pagination defaults, empty results, invalid limits, and restart durability regress.
- Answer allows another Agent, invalid text, hides Telegram failure, or retries durably.
- Synchronous mutations reach Telegram before startup verifies the Bot identity.
- Edit permits another Agent, incoming/undelivered/wrong-topic/unknown targets,
  closed Topics/unavailable Groups, or changes history before Telegram success.
- Plain/Markdown/header rendering diverges from send; omitted markup is cleared.
- Edit appends conversation, resets acknowledgements, or queues unsafe durable retries.
- Editing an unacked message fails to reevaluate mentions for a muted waiter, or
  editing an acked message incorrectly creates new pending conversation.
- Empty markup fails to remove buttons; stale callbacks remain accepted afterward.
- Lost edit/answer responses trigger implicit retries; identical caller edit retry
  cannot recover Telegram's 'message is not modified' response after restart.
- A malformed successful Telegram result (missing/wrong message ID or chat, or
  non-true answer result) is mistaken for confirmed success and commits local state.
"""

import collections
import contextlib
import copy
import http.server
import json
import os
import pathlib
import socket
import sqlite3
import subprocess
import tempfile
import threading
import time
import traceback
import urllib.error
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
REPORT = ROOT / "target/buttons-e2e"
CHAT = -100
ADMIN = 123
TRUSTED = 321
SECRET = "BUTTON_TEST_TOKEN"
KEYBOARD = {
    "inline_keyboard": [
        [{"text": "Approve", "callback_data": "approve"},
         {"text": "Reject", "callback_data": "reject"}],
        [{"text": "Details", "url": "https://example.com/details"}],
    ]
}
EMPTY = {"inline_keyboard": []}


def eventually(check, timeout=12):
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        try:
            value = check()
            if value:
                return value
        except (OSError, urllib.error.URLError, AssertionError) as error:
            last = error
        time.sleep(0.05)
    raise AssertionError(f"condition did not become true: {last}")


class TelegramState:
    def __init__(self):
        self.lock = threading.Lock()
        self.updates = []
        self.calls = []
        self.messages = {}
        self.faults = collections.defaultdict(collections.deque)
        self.mid = 100
        self.tid = 10

    def fault(self, method, *modes):
        with self.lock:
            self.faults[method].extend(modes)

    def requests(self, method):
        with self.lock:
            return copy.deepcopy([body for name, body in self.calls if name == method])

    def message(self, mid):
        with self.lock:
            return copy.deepcopy(self.messages[(CHAT, mid)])


class Telegram(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))))
        method = self.path.rsplit("/", 1)[-1]
        state = self.server.state
        if method == "getUpdates":
            time.sleep(0.03)
        with state.lock:
            state.calls.append((method, body))
            mode = state.faults[method].popleft() if state.faults[method] else None
            status = 200
            envelope = None
            result = True
            if mode in ("reject", "rate", "blocked", "expired"):
                status = {"reject": 400, "rate": 429, "blocked": 403, "expired": 400}[mode]
                envelope = {"ok": False, "error_code": status,
                            "description": "controlled Telegram rejection"}
                if mode == "expired":
                    envelope["description"] = "Bad Request: query is too old and response timeout expired"
                if mode == "rate":
                    envelope["parameters"] = {"retry_after": 1}
            elif method == "getMe":
                result = {"id": 999, "is_bot": True, "username": "button_test_bot"}
            elif method == "getWebhookInfo":
                result = {"url": ""}
            elif method == "getUpdates":
                state.updates = [u for u in state.updates
                                 if u["update_id"] >= body.get("offset", 0)]
                result = copy.deepcopy(state.updates)
            elif method == "getChat":
                result = {"id": body["chat_id"], "title": "Buttons",
                          "type": "supergroup", "is_forum": True}
            elif method == "getChatMember":
                result = {"status": "administrator", "can_manage_topics": True}
            elif method == "createForumTopic":
                state.tid += 1
                result = {"message_thread_id": state.tid, "name": body["name"]}
            elif method == "sendMessage":
                state.mid += 1
                state.messages[(body["chat_id"], state.mid)] = copy.deepcopy(body)
                result = {"message_id": state.mid, "date": int(time.time()),
                          "chat": {"id": body["chat_id"]}}
            elif method in ("editMessageText", "editMessageReplyMarkup"):
                key = (body["chat_id"], body["message_id"])
                original = state.messages[key]
                edited = copy.deepcopy(original)
                if method == "editMessageText":
                    edited["text"] = body["text"]
                    edited.pop("parse_mode", None)
                    if "parse_mode" in body:
                        edited["parse_mode"] = body["parse_mode"]
                if "reply_markup" in body:
                    edited["reply_markup"] = copy.deepcopy(body["reply_markup"])
                if edited == original:
                    status = 400
                    envelope = {"ok": False, "error_code": 400,
                                "description": "Bad Request: message is not modified"}
                else:
                    state.messages[key] = edited
                    result = {"message_id": key[1], "date": int(time.time()),
                              "chat": {"id": key[0]}}
            # 'lost' applies the request, then drops its response: outcome is ambiguous.
            if mode == "lost":
                self.close_connection = True
                return
            # The remote mutation happened, but its success response is unusable.
            if mode == "missing_message_id":
                result.pop("message_id")
            elif mode == "wrong_message_id":
                result["message_id"] += 100000
            elif mode == "wrong_chat":
                result["chat"]["id"] -= 100000
            elif mode == "false_result":
                result = False
            elif mode == "null_result":
                result = None
            encoded = json.dumps(envelope or {"ok": True, "result": result}).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        try:
            self.wfile.write(encoded)
        except (BrokenPipeError, ConnectionResetError):
            pass


class Fixture:
    def __init__(self, tmp):
        self.tmp = tmp
        self.data = tmp / "data"
        self.data.mkdir()
        self.config = tmp / "config"
        self.config.mkdir()
        self.database = self.data / "gateway.sqlite"
        # Exercise an actual v1 database upgrade, including existing retained data.
        with sqlite3.connect(self.database) as db:
            db.executescript((ROOT / "migrations/0001_initial.sql").read_text())
            db.execute("INSERT INTO agents VALUES('legacy',0)")
            db.execute("INSERT INTO groups VALUES(-200,'Legacy',1,1)")
            db.execute("INSERT INTO topics(id,chat,thread,name,owner) "
                       "VALUES('t200_50',-200,50,'Legacy','legacy')")
            db.execute("INSERT INTO messages(topic,agent,content,sent_at) "
                       "VALUES('t200_50','legacy','preserved v1 history',0)")
            db.execute("INSERT INTO users(id,trusted) VALUES(?,1)", (TRUSTED,))
            assert db.execute("PRAGMA user_version").fetchone()[0] == 1
        self.telegram = TelegramState()
        self.stub = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Telegram)
        self.stub.state = self.telegram
        threading.Thread(target=self.stub.serve_forever, daemon=True).start()
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            self.port = sock.getsockname()[1]
        (self.config / "client.yaml").write_text(
            f'host: "127.0.0.1"\nport: {self.port}\n')
        (self.config / "server.yaml").write_text(
            f'listen: "127.0.0.1:{self.port}"\ndata_dir: "{self.data}"\n'
            f'telegram:\n  bot_token: "{SECRET}"\nadmins: [{ADMIN}]\n')
        self.env = dict(os.environ, TBG_TEST_CONFIG_DIR=str(self.config),
                        TBG_TEST_TELEGRAM_URL=f"http://127.0.0.1:{self.stub.server_port}")
        self.log = (REPORT / "gateway.log").open("w")
        self.proc = None
        self.update_id = 0

    def start(self, wait_telegram=True):
        polls = len(self.telegram.requests("getUpdates"))
        self.proc = subprocess.Popen([str(ROOT / "target/debug/tbg-gateway")],
                                     env=self.env, stdout=self.log, stderr=self.log)
        def ready():
            assert self.proc.poll() is None, "Gateway exited; inspect gateway.log"
            with socket.create_connection(("127.0.0.1", self.port), 0.1):
                return True
        eventually(ready)
        if wait_telegram:
            eventually(lambda: len(self.telegram.requests("getUpdates")) > polls)

    def stop(self):
        if self.proc and self.proc.poll() is None:
            self.proc.terminate()
            self.proc.wait(timeout=8)

    def restart(self):
        self.stop()
        self.start()

    def api(self, route, ok=True, **params):
        request = urllib.request.Request(
            f"http://127.0.0.1:{self.port}/v1/{route}", json.dumps(params).encode(),
            {"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(request, timeout=8) as response:
                result = json.load(response)
                assert ok, (route, params, result)
                return result
        except urllib.error.HTTPError as error:
            result = json.load(error)
            assert not ok, (route, params, error.code, result)
            assert 400 <= error.code < 600 and isinstance(result.get("error"), str)
            return result

    def cli(self, *args, ok=True):
        result = subprocess.run([str(ROOT / "target/debug/tbg"), *args],
                                env=self.env, text=True, capture_output=True, timeout=10)
        assert (result.returncode == 0) == ok, (args, result.stdout, result.stderr)
        if not ok:
            assert not result.stdout and result.stderr
            return result.stderr
        assert len(result.stdout.splitlines()) == 1 and result.stdout.endswith("\n")
        return json.loads(result.stdout)

    def sql(self, statement, args=()):
        with sqlite3.connect(self.database) as db:
            return db.execute(statement, args).fetchall()

    def push(self, **event):
        self.update_id += 1
        update = {"update_id": self.update_id, **event}
        with self.telegram.lock:
            self.telegram.updates.append(update)
        return self.update_id

    def received(self, update_id):
        return bool(self.sql("SELECT id FROM updates WHERE id=?", (update_id,)))

    def settle(self, update_id):
        eventually(lambda: self.received(update_id))

    def incoming(self, text, thread=None, user=ADMIN, chat=CHAT, mid=None):
        message = {"message_id": mid or 10000 + self.update_id,
                   "date": int(time.time()), "from": {"id": user},
                   "chat": {"id": chat, "type": "supergroup", "is_forum": True,
                            "title": "Buttons"}, "text": text}
        if thread is not None:
            message["message_thread_id"] = thread
        uid = self.push(message=message)
        self.settle(uid)
        return message["message_id"]

    def callback(self, query, mid, data="approve", user=ADMIN, chat=CHAT,
                 inaccessible=False, inline=False, **extra):
        value = {"id": query, "from": {"id": user, "is_bot": False},
                 "chat_instance": "test-instance", "data": data}
        if inline:
            value["inline_message_id"] = "inline-only"
        else:
            value["message"] = {
                "message_id": mid, "date": 0 if inaccessible else int(time.time()),
                "chat": {"id": chat, "type": "supergroup"},
                "text": "EMBEDDED_ORIGINAL_SECRET",
                "caption": "EMBEDDED_CAPTION_SECRET",
                "reply_to_message": {"message_id": 500, "text": "EMBEDDED_REPLY_SECRET"},
                "quote": {"text": "EMBEDDED_QUOTE_SECRET", "position": 0},
                "photo": [{"file_id": "EMBEDDED_MEDIA_SECRET"}],
            }
        value.update(extra)
        return self.push(callback_query=value)

    def send(self, topic, content, agent="alpha", markup=KEYBOARD, **params):
        request = dict(agent=agent, topic=topic, content=content, **params)
        if markup is not None:
            request["reply_markup"] = markup
        result = self.api("send", **request)
        msg = result["msg_id"]
        return msg, self.wait_for_delivery(msg)

    def wait_for_delivery(self, msg, timeout=12):
        def delivered():
            rows = self.sql("SELECT telegram_id FROM telegram_messages WHERE message=?",
                            (int(msg[1:]),))
            return rows[0][0] if rows else None
        return eventually(delivered, timeout=timeout)

    def history(self, topic, agent="alpha"):
        return self.api("history", agent=agent, topic=topic, limit=200)["messages"]

    def callbacks(self, agent="alpha", **params):
        return self.api("callback/list", agent=agent, **params)

    def close(self):
        self.stop()
        self.stub.shutdown()
        self.stub.server_close()
        self.log.close()
        with sqlite3.connect(self.database) as source:
            target = REPORT / "gateway.sqlite"
            if target.exists():
                target.unlink()
            with sqlite3.connect(target) as destination:
                source.backup(destination)
        (REPORT / "telegram-calls.json").write_text(
            json.dumps(self.telegram.calls, ensure_ascii=False, indent=2) + "\n")
        assert SECRET not in (REPORT / "gateway.log").read_text()

    def delivered_all(self):
        eventually(lambda: not self.sql("SELECT id FROM outbox WHERE status <> 'done'"))


def prepare_topics(f):
    f.start()
    assert f.sql("PRAGMA user_version")[0][0] >= 2
    assert f.api("history", agent="legacy", topic="t200_50")["messages"][0][
        "content"] == "preserved v1 history"
    f.cli("agent", "register", "--name", "alpha")
    f.cli("agent", "register", "--name", "beta")
    f.incoming("/manage")
    f.topic = f.cli("--agent", "alpha", "topic", "create", "--group", str(CHAT),
                    "--name", "Buttons")["topic"]
    f.thread = f.telegram.tid
    f.other = f.cli("--agent", "alpha", "topic", "create", "--group", str(CHAT),
                    "--name", "Other buttons")["topic"]
    f.cli("--agent", "alpha", "subscribe", f.topic)
    f.cli("--agent", "beta", "subscribe", f.topic)
    return "v1 schema upgrade preserves existing history and trust fixtures"


def verify_send_and_validation(f):
    response = f.cli("--agent", "alpha", "send", f.topic, "Choose an action",
                     "--reply-markup", json.dumps(KEYBOARD))
    f.msg = response["msg_id"]
    f.mid = f.wait_for_delivery(f.msg)
    assert f.telegram.message(f.mid)["reply_markup"] == KEYBOARD
    assert f.telegram.message(f.mid)["text"] == "alpha:\nChoose an action"
    f.plain, f.plain_mid = f.send(f.topic, "No buttons", markup=None)
    assert "reply_markup" not in f.telegram.message(f.plain_mid)
    f.beta, f.beta_mid = f.send(f.topic, "Beta buttons", agent="beta")
    f.other_msg, f.other_mid = f.send(f.other, "Other topic buttons")
    f.incoming_mid = f.incoming("Incoming user message", f.thread)
    f.incoming_msg = f.history(f.topic)[0]["msg_id"]
    boundary = f.history(f.topic)[0]["msg_id"]
    for agent in ("alpha", "beta"):
        f.cli("--agent", agent, "ack", f.topic, "--through", boundary)
    before = len(f.history(f.topic))
    invalid = [None, [], "{}", {}, {"inline_keyboard": None},
               {"inline_keyboard": "bad"}, {"inline_keyboard": [1]},
               {"inline_keyboard": [[]]},
               {"inline_keyboard": [[None]]}, {"inline_keyboard": [[{}]]},
               {"inline_keyboard": [], "extra": True}]
    invalid_buttons = [
        {"text": "", "callback_data": "a"},
        {"text": " \n\t", "callback_data": "a"},
        {"text": 1, "callback_data": "a"},
        {"text": "Missing"},
        {"text": "Both", "callback_data": "a", "url": "https://example.com"},
        {"text": "Unknown", "callback_data": "a", "web_app": {}},
        {"text": "Empty", "callback_data": ""},
        {"text": "Long", "callback_data": "x" * 65},
        {"text": "UTF-8 overflow", "callback_data": "🙂" * 17},
        {"text": "Wrong type", "callback_data": 1},
        {"text": "Null", "callback_data": None},
        {"text": "Null URL", "url": None},
        {"text": "Wrong URL type", "url": 1},
        {"text": "Empty URL", "url": ""},
        {"text": "Unsupported URL", "url": "javascript:alert(1)"},
        {"text": "Missing URL host", "url": "https://"},
        {"text": "Malformed URL host", "url": "https://exa mple.com"},
    ]
    invalid += [{"inline_keyboard": [[button]]} for button in invalid_buttons]
    for markup in invalid:
        f.api("send", ok=False, agent="alpha", topic=f.topic, content="invalid",
              reply_markup=markup)
    for markup in ("{", "null", "[]"):
        f.cli("--agent", "alpha", "send", f.topic, "invalid", "--reply-markup",
              markup, ok=False)
    assert len(f.history(f.topic)) == before
    utf8 = {"inline_keyboard": [[{"text": "64 bytes", "callback_data": "🙂" * 16}]]}
    _, mid = f.send(f.other, "UTF-8 boundary", markup=utf8)
    assert f.telegram.message(mid)["reply_markup"] == utf8
    f.api("send", ok=False, agent="alpha", topic=f.topic, content="unknown field",
          reply_markup=KEYBOARD, unexpected=True)
    return "CLI/API send markup, strict nested validation, UTF-8 byte boundaries"


def verify_callback_admission(f):
    f.delivered_all()
    before = f.history(f.topic)
    subs = f.api("subscriptions", agent="alpha")
    outbox = f.sql("SELECT COUNT(*) FROM outbox")[0][0]
    hearts = len(f.telegram.requests("setMessageReaction"))
    assert f.callbacks() == {"callbacks": [], "next_cursor": None, "remaining_count": 0}
    # Intentionally nonlexical IDs and same polling batch: receive order wins.
    uid = f.callback("query-z", f.mid)
    f.first_callback_update = uid
    f.callback("query-a", f.mid, data="reject", user=TRUSTED, inaccessible=True)
    last = f.callback("query-beta", f.beta_mid)
    f.settle(last)
    alpha = f.callbacks()["callbacks"]
    assert [c["callback_query_id"] for c in alpha] == ["query-z", "query-a"]
    expected = {"callback_query_id", "topic", "msg_id", "user_id", "data", "received_at"}
    assert all(set(callback) == expected for callback in alpha)
    assert [(c["topic"], c["msg_id"], c["user_id"], c["data"]) for c in alpha] == [
        (f.topic, f.msg, ADMIN, "approve"), (f.topic, f.msg, TRUSTED, "reject")]
    assert all(isinstance(c["received_at"], str) and c["received_at"] for c in alpha)
    assert [c["callback_query_id"] for c in f.callbacks(agent="beta")["callbacks"]] == [
        "query-beta"]
    retained = f.sql("SELECT raw FROM updates WHERE id=?", (uid,))[0][0]
    assert retained is not None
    assert "EMBEDDED_" not in retained
    assert json.loads(retained)["callback_query"]["message"]["message_id"] == f.mid
    invalid = [
        ("untrusted", f.mid, {"user": 456}),
        ("wrong-chat", f.mid, {"chat": -300}),
        ("unknown-message", 987654, {}),
        ("incoming-message", f.incoming_mid, {}),
        ("without-keyboard", f.plain_mid, {}),
        ("forged-data", f.mid, {"data": "never-issued"}),
        ("empty-data", f.mid, {"data": ""}),
        ("inline-only", f.mid, {"inline": True}),
    ]
    rejected_updates = []
    for query, mid, overrides in invalid:
        rejected_updates.append(f.callback(query, mid, **overrides))
    missing = {"id": "missing-data", "from": {"id": ADMIN}, "chat_instance": "test",
               "message": {"message_id": f.mid, "date": 0, "chat": {"id": CHAT}},
               "game_short_name": "unsupported"}
    rejected_updates.append(f.push(callback_query=missing))
    f.settle(rejected_updates[-1])
    assert f.callbacks()["callbacks"] == alpha
    for rejected in rejected_updates:
        assert f.sql("SELECT raw FROM updates WHERE id=?", (rejected,))[0][0] is None
    # Duplicate callback query ID under a new Telegram update must still deduplicate.
    last = f.callback("query-z", f.mid, data="reject")
    f.settle(last)
    assert f.callbacks()["callbacks"] == alpha
    assert f.history(f.topic) == before
    assert f.api("unread", agent="alpha", topic=f.topic)["messages"] == []
    assert f.api("subscriptions", agent="alpha") == subs
    assert f.api("wait", agent="alpha", topic=f.topic, timeout=1) == {"topics": []}
    assert f.sql("SELECT COUNT(*) FROM outbox")[0][0] == outbox
    assert len(f.telegram.requests("setMessageReaction")) == hearts
    assert any("callback_query" in body.get("allowed_updates", [])
               for body in f.telegram.requests("getUpdates"))
    return "trusted owner routing, inaccessible mapping, dedup, raw sanitization, conversation isolation"


def verify_callback_pagination(f):
    first = f.cli("--agent", "alpha", "callback", "list", "--topic", f.topic,
                  "--limit", "1")
    assert len(first["callbacks"]) == 1 and first["remaining_count"] == 1
    assert first["next_cursor"] == "query-z"
    later = f.callbacks(topic=f.topic, cursor=first["next_cursor"])
    assert [c["callback_query_id"] for c in later["callbacks"]] == ["query-a"]
    assert later["remaining_count"] == 0 and later["next_cursor"] == "query-a"
    assert f.callbacks(topic=f.topic, cursor="query-a")["callbacks"] == []
    assert f.callbacks(topic=f.other)["callbacks"] == []
    for args in ({"limit": 0}, {"limit": -1}, {"limit": "1"}, {"limit": None},
                 {"cursor": "unknown"}, {"cursor": "query-beta"},
                 {"topic": f.other, "cursor": "query-a"}, {"topic": "unknown"},
                 {"unexpected": True}):
        f.api("callback/list", ok=False, agent="alpha", **args)
    f.api("callback/list", ok=False)
    f.api("callback/list", ok=False, agent="unregistered")
    assert f.callbacks(topic=f.topic, limit=1) == first  # Reading never consumes.
    for i in range(23):
        last = f.callback(f"page-{22-i:02}", f.other_mid)
    f.settle(last)
    default = f.callbacks(topic=f.other)
    assert len(default["callbacks"]) == 20 and default["remaining_count"] == 3
    assert [c["callback_query_id"] for c in default["callbacks"]] == [
        f"page-{22-i:02}" for i in range(20)]
    tail = f.callbacks(topic=f.other, cursor=default["next_cursor"], limit=10)
    assert len(tail["callbacks"]) == 3 and tail["remaining_count"] == 0
    saved = f.callbacks(limit=100)
    f.stop()
    # Startup intentionally polls without an offset. Redelivery of the original
    # Telegram update must be harmless even if its payload is inconsistent.
    with f.telegram.lock:
        f.telegram.updates.append({"update_id": f.first_callback_update,
            "callback_query": {"id": "duplicate-update-new-query", "from": {"id": ADMIN},
                "chat_instance": "test", "data": "approve", "message": {
                    "message_id": f.mid, "date": 0, "chat": {"id": CHAT}}}})
    f.start()
    # A fresh update is a processing barrier for the redelivered startup batch.
    barrier = f.callback("query-z", f.mid)
    f.settle(barrier)
    assert f.callbacks(limit=100) == saved
    last = f.callback("query-z", f.mid)
    f.settle(last)
    assert f.callbacks(limit=100) == saved
    return "exclusive scoped cursors, default pagination, read-only callbacks, restart durability"


def verify_answers(f):
    before = len(f.telegram.requests("answerCallbackQuery"))
    result = f.cli("--agent", "alpha", "callback", "answer", "query-z", "--text",
                   "Approved", "--show-alert")
    assert result == {"callback_query_id": "query-z"}
    sent = f.telegram.requests("answerCallbackQuery")[-1]
    assert sent == {"callback_query_id": "query-z", "text": "Approved", "show_alert": True}
    assert len(f.telegram.requests("answerCallbackQuery")) == before + 1
    f.api("callback/answer", agent="alpha", callback_query_id="query-a", text="🙂" * 200)
    assert f.telegram.requests("answerCallbackQuery")[-1]["text"] == "🙂" * 200
    assert f.cli("--agent", "alpha", "callback", "answer", "query-a") == {
        "callback_query_id": "query-a"}
    minimal = f.telegram.requests("answerCallbackQuery")[-1]
    assert minimal["callback_query_id"] == "query-a"
    assert minimal.get("text") is None and minimal.get("show_alert", False) is False
    before = len(f.telegram.requests("answerCallbackQuery"))
    for params in ({"agent": "beta", "callback_query_id": "query-z"},
                   {"agent": "alpha", "callback_query_id": "unknown"},
                   {"agent": "alpha", "callback_query_id": "query-z", "text": "a" * 201},
                   {"agent": "alpha", "callback_query_id": "query-z", "text": None},
                   {"agent": "alpha", "callback_query_id": "query-z", "show_alert": "yes"},
                   {"agent": "alpha", "callback_query_id": "query-z", "unexpected": True},
                   {"callback_query_id": "query-z"}):
        f.api("callback/answer", ok=False, **params)
    assert len(f.telegram.requests("answerCallbackQuery")) == before
    outbox = f.sql("SELECT COUNT(*) FROM outbox")[0][0]
    for mode in ("reject", "rate", "lost", "expired"):
        f.telegram.fault("answerCallbackQuery", mode)
        failure = f.api("callback/answer", ok=False, agent="alpha", callback_query_id="query-z")
        if mode == "expired":
            assert "query is too old" in failure["error"]
    count = len(f.telegram.requests("answerCallbackQuery"))
    assert count == before + 4
    f.restart()
    time.sleep(1.3)
    assert len(f.telegram.requests("answerCallbackQuery")) == count
    assert f.sql("SELECT COUNT(*) FROM outbox")[0][0] == outbox
    assert f.callbacks(topic=f.topic)["callbacks"][0]["callback_query_id"] == "query-z"
    return "owner-only synchronous callback answers, text limits, visible failures, no retries"


def verify_edits(f):
    history = f.history(f.topic)
    count = len(history)
    subs = f.api("subscriptions", agent="alpha")
    outbox = f.sql("SELECT COUNT(*) FROM outbox")[0][0]
    result = f.cli("--agent", "alpha", "edit", "text", f.topic, f.msg, "Revised text")
    assert result == {"msg_id": f.msg}
    sent = f.telegram.requests("editMessageText")[-1]
    assert (sent["chat_id"], sent["message_id"], sent["text"]) == (CHAT, f.mid, "alpha:\nRevised text")
    assert f.telegram.message(f.mid)["reply_markup"] == KEYBOARD
    updated = next(m for m in f.history(f.topic) if m["msg_id"] == f.msg)
    old = next(m for m in history if m["msg_id"] == f.msg)
    assert updated == dict(old, content="Revised text")
    assert len(f.history(f.topic)) == count
    assert f.api("subscriptions", agent="alpha") == subs
    assert f.api("unread", agent="alpha", topic=f.topic)["messages"] == []
    assert f.sql("SELECT COUNT(*) FROM outbox")[0][0] == outbox
    markdown = "**bold** & <literal>"
    f.cli("--agent", "alpha", "edit", "text", f.topic, f.msg, markdown,
          "--format", "markdown", "--no-header")
    rendered = f.telegram.requests("editMessageText")[-1]
    assert rendered["text"] == "<b>bold</b> &amp; &lt;literal&gt;"
    assert rendered["parse_mode"] == "HTML"
    assert next(m for m in f.history(f.topic) if m["msg_id"] == f.msg)["content"] == markdown
    f.api("edit/text", agent="alpha", topic=f.topic, msg_id=f.msg, content="plain again",
          format="plain", no_header=True)
    assert f.telegram.requests("editMessageText")[-1]["text"] == "plain again"
    assert f.telegram.requests("editMessageText")[-1].get("parse_mode") is None
    replacement = {"inline_keyboard": [[{"text": "Next", "callback_data": "next"}]]}
    f.cli("--agent", "alpha", "edit", "markup", f.topic, f.msg,
          "--reply-markup", json.dumps(replacement))
    assert f.telegram.message(f.mid)["reply_markup"] == replacement
    last = f.callback("replaced-old", f.mid)
    f.settle(last)
    assert "replaced-old" not in [c["callback_query_id"] for c in f.callbacks(limit=200)["callbacks"]]
    last = f.callback("replaced-new", f.mid, data="next")
    f.settle(last)
    assert "replaced-new" in [c["callback_query_id"] for c in f.callbacks(limit=200)["callbacks"]]
    f.api("edit/text", agent="alpha", topic=f.topic, msg_id=f.msg, content="new buttons",
          reply_markup=KEYBOARD)
    assert f.telegram.message(f.mid)["reply_markup"] == KEYBOARD
    f.cli("--agent", "alpha", "edit", "markup", f.topic, f.msg,
          "--reply-markup", json.dumps(EMPTY))
    assert f.telegram.message(f.mid)["reply_markup"] == EMPTY
    last = f.callback("cleared-old", f.mid)
    f.settle(last)
    assert "cleared-old" not in [c["callback_query_id"] for c in f.callbacks(limit=200)["callbacks"]]
    f.restart()
    f.api("edit/text", agent="alpha", topic=f.topic, msg_id=f.msg, content="still clear")
    assert f.telegram.message(f.mid)["reply_markup"] == EMPTY
    # Markup can be added to an originally plain own outgoing message.
    f.api("edit/markup", agent="alpha", topic=f.topic, msg_id=f.plain, reply_markup=KEYBOARD)
    last = f.callback("newly-added", f.plain_mid)
    f.settle(last)
    assert "newly-added" in [c["callback_query_id"] for c in f.callbacks(limit=200)["callbacks"]]
    return "text/markup edits, rendering, preserved/cleared keyboards, local history and ack stability"


def verify_edit_rejections(f):
    base = dict(agent="alpha", topic=f.topic, msg_id=f.msg)
    invalid_targets = [dict(base, agent="beta"), dict(base, msg_id=f.incoming_msg),
                       dict(base, msg_id="m999999"), dict(base, msg_id=f.other_msg),
                       dict(base, topic=f.other), dict(base, msg_id="invalid")]
    before = len(f.telegram.requests("editMessageText"))
    markup_before = len(f.telegram.requests("editMessageReplyMarkup"))
    for params in invalid_targets:
        f.api("edit/text", ok=False, **params, content="forbidden")
        f.api("edit/markup", ok=False, **params, reply_markup=KEYBOARD)
    for content in ("", "x" * 4096):
        f.api("edit/text", ok=False, **base, content=content)
    for extra in ({"reply_markup": None}, {"reply_markup": {"inline_keyboard": [[{"text": "x"}]]}},
                  {"format": "html"}, {"no_header": None}, {"extra": True}):
        f.api("edit/text", ok=False, **base, content="invalid", **extra)
    f.api("edit/markup", ok=False, **base)
    f.api("edit/markup", ok=False, **base, reply_markup=None)
    f.api("edit/markup", ok=False, **base, reply_markup=KEYBOARD, extra=True)
    f.api("edit/text", ok=False, topic=f.topic, msg_id=f.msg, content="missing agent")
    assert len(f.telegram.requests("editMessageText")) == before
    assert len(f.telegram.requests("editMessageReplyMarkup")) == markup_before
    f.cli("--agent", "alpha", "topic", "close", f.topic)
    f.api("edit/text", ok=False, **base, content="closed")
    f.api("edit/markup", ok=False, **base, reply_markup=KEYBOARD)
    f.cli("--agent", "alpha", "topic", "reopen", f.topic)
    uid = f.push(my_chat_member={"chat": {"id": CHAT, "title": "Buttons"},
                 "from": {"id": ADMIN}, "new_chat_member": {"status": "left"}})
    f.settle(uid)
    f.api("edit/text", ok=False, **base, content="disconnected")
    f.api("edit/markup", ok=False, **base, reply_markup=KEYBOARD)
    last = f.callback("disconnected", f.plain_mid)
    f.settle(last)
    assert "disconnected" not in [c["callback_query_id"] for c in f.callbacks(limit=200)["callbacks"]]
    f.incoming("/manage")
    f.delivered_all()
    assert len(f.telegram.requests("editMessageText")) == before
    assert len(f.telegram.requests("editMessageReplyMarkup")) == markup_before
    return "edit ownership, target/delivery scope, validation, closed/unavailable Topic rejection"


def verify_ambiguous_edit_recovery(f):
    base = dict(agent="alpha", topic=f.topic, msg_id=f.msg)
    history = f.history(f.topic)
    outbox = f.sql("SELECT COUNT(*) FROM outbox")[0][0]
    for method, route, params in (
            ("editMessageText", "edit/text", {"content": "rejected change"}),
            ("editMessageReplyMarkup", "edit/markup", {"reply_markup": KEYBOARD})):
        for mode in ("reject", "rate"):
            f.telegram.fault(method, mode)
            f.api(route, ok=False, **base, **params)
        assert f.history(f.topic) == history
    last = f.callback("rejected-markup", f.mid)
    f.settle(last)
    assert "rejected-markup" not in [c["callback_query_id"] for c in f.callbacks(limit=200)["callbacks"]]
    f.telegram.fault("editMessageText", "lost")
    f.api("edit/text", ok=False, **base, content="ambiguous text")
    assert f.history(f.topic) == history
    assert f.telegram.message(f.mid)["text"] == "alpha:\nambiguous text"
    counts = {method: len(f.telegram.requests(method)) for method in
              ("editMessageText", "editMessageReplyMarkup")}
    f.restart()
    time.sleep(1.3)
    assert all(len(f.telegram.requests(method)) == count for method, count in counts.items())
    assert f.sql("SELECT COUNT(*) FROM outbox")[0][0] == outbox
    # The mock returns Telegram's real not-modified error on this exact retry.
    assert f.api("edit/text", **base, content="ambiguous text") == {"msg_id": f.msg}
    assert next(m for m in f.history(f.topic) if m["msg_id"] == f.msg)["content"] == "ambiguous text"
    f.telegram.fault("editMessageReplyMarkup", "lost")
    f.api("edit/markup", ok=False, **base, reply_markup=KEYBOARD)
    last = f.callback("uncertain-markup", f.mid)
    f.settle(last)
    assert "uncertain-markup" not in [c["callback_query_id"] for c in f.callbacks(limit=200)["callbacks"]]
    assert f.api("edit/markup", **base, reply_markup=KEYBOARD) == {"msg_id": f.msg}
    last = f.callback("recovered-markup", f.mid)
    f.settle(last)
    assert "recovered-markup" in [c["callback_query_id"] for c in f.callbacks(limit=200)["callbacks"]]
    assert len(f.history(f.topic)) == len(history)
    return "failed edits do not commit, no implicit retries, exact retries reconcile not-modified success"


def verify_malformed_success(f):
    base = dict(agent="alpha", topic=f.topic, msg_id=f.msg)
    outbox = f.sql("SELECT COUNT(*) FROM outbox")[0][0]
    modes = ("missing_message_id", "wrong_message_id", "wrong_chat")
    for mode in modes:
        history = f.history(f.topic)
        desired = f"Malformed response: {mode}"
        before = len(f.telegram.requests("editMessageText"))
        f.telegram.fault("editMessageText", mode)
        f.api("edit/text", ok=False, **base, content=desired)
        assert f.history(f.topic) == history
        assert f.telegram.message(f.mid)["text"] == f"alpha:\n{desired}"
        assert len(f.telegram.requests("editMessageText")) == before + 1
        # Exact retry receives not-modified and confirms the desired local state.
        assert f.api("edit/text", **base, content=desired) == {"msg_id": f.msg}
        assert next(message for message in f.history(f.topic)
                    if message["msg_id"] == f.msg)["content"] == desired
    current_data = "approve"
    for mode in modes:
        history = f.history(f.topic)
        data = f"valid-after-{mode}"
        keyboard = {"inline_keyboard": [[{"text": "Confirm", "callback_data": data}]]}
        before = len(f.telegram.requests("editMessageReplyMarkup"))
        f.telegram.fault("editMessageReplyMarkup", mode)
        f.api("edit/markup", ok=False, **base, reply_markup=keyboard)
        assert f.history(f.topic) == history
        assert f.telegram.message(f.mid)["reply_markup"] == keyboard
        assert len(f.telegram.requests("editMessageReplyMarkup")) == before + 1
        rejected = f"malformed-pending-{mode}"
        f.callback(rejected, f.mid, data=data)
        retained = f"malformed-retained-{mode}"
        last = f.callback(retained, f.mid, data=current_data)
        f.settle(last)
        queries = [callback["callback_query_id"] for callback in f.callbacks(limit=200)["callbacks"]]
        assert rejected not in queries and retained in queries
        assert f.api("edit/markup", **base, reply_markup=keyboard) == {"msg_id": f.msg}
        recovered = f"malformed-recovered-{mode}"
        last = f.callback(recovered, f.mid, data=data)
        f.settle(last)
        assert recovered in [callback["callback_query_id"]
                             for callback in f.callbacks(limit=200)["callbacks"]]
        current_data = data
    for mode in ("false_result", "null_result"):
        before = len(f.telegram.requests("answerCallbackQuery"))
        f.telegram.fault("answerCallbackQuery", mode)
        f.api("callback/answer", ok=False, agent="alpha", callback_query_id="query-z")
        assert len(f.telegram.requests("answerCallbackQuery")) == before + 1
    counts = {method: len(f.telegram.requests(method)) for method in
              ("editMessageText", "editMessageReplyMarkup", "answerCallbackQuery")}
    f.restart()
    time.sleep(1.3)
    assert all(len(f.telegram.requests(method)) == count for method, count in counts.items())
    assert f.sql("SELECT COUNT(*) FROM outbox")[0][0] == outbox
    return "malformed success is visible and uncommitted; exact edit retries reconcile; answers require true"


def verify_pending_send_and_restart(f):
    # A rejected send remains durable, but its unmapped local message is not editable.
    f.delivered_all()
    f.telegram.fault("sendMessage", "blocked")
    msg = f.cli("--agent", "alpha", "send", f.topic, "Durable keyboard",
                "--reply-markup", json.dumps(KEYBOARD))["msg_id"]
    eventually(lambda: f.sql("SELECT status FROM outbox WHERE kind='send' AND message=?",
                              (int(msg[1:]),)) == [("blocked",)])
    assert not f.sql("SELECT 1 FROM telegram_messages WHERE message=?", (int(msg[1:]),))
    f.api("edit/text", ok=False, agent="alpha", topic=f.topic, msg_id=msg, content="too soon")
    f.api("edit/markup", ok=False, agent="alpha", topic=f.topic, msg_id=msg, reply_markup=EMPTY)
    f.restart()
    mid = f.wait_for_delivery(msg)
    assert f.telegram.message(mid)["reply_markup"] == KEYBOARD
    last = f.callback("after-send-restart", mid)
    f.settle(last)
    assert next(c for c in f.callbacks(limit=200)["callbacks"]
                if c["callback_query_id"] == "after-send-restart")["msg_id"] == msg
    f.telegram.fault("sendMessage", "rate", "lost")
    repeated = f.cli("--agent", "alpha", "send", f.topic, "Retried keyboard",
                     "--reply-markup", json.dumps(KEYBOARD))["msg_id"]
    f.wait_for_delivery(repeated, timeout=25)
    attempts = [request for request in f.telegram.requests("sendMessage")
                if request.get("text") == "alpha:\nRetried keyboard"]
    assert len(attempts) == 3 and all(request["reply_markup"] == KEYBOARD for request in attempts)
    assert sum(message["msg_id"] == repeated for message in f.history(f.topic)) == 1
    return "undelivered edit rejection, durable keyboard restart recovery, rate/lost send retries"


def verify_identity_readiness(f):
    saved = f.callbacks(limit=200)
    f.stop()
    f.telegram.fault("getMe", *(["blocked"] * 20))
    counts = {method: len(f.telegram.requests(method)) for method in
              ("answerCallbackQuery", "editMessageText", "editMessageReplyMarkup")}
    f.start(wait_telegram=False)
    assert f.callbacks(limit=200) == saved
    f.api("callback/answer", ok=False, agent="alpha", callback_query_id="query-z")
    f.api("edit/text", ok=False, agent="alpha", topic=f.topic, msg_id=f.msg,
          content="unverified Bot")
    f.api("edit/markup", ok=False, agent="alpha", topic=f.topic, msg_id=f.msg,
          reply_markup=EMPTY)
    assert all(len(f.telegram.requests(method)) == count for method, count in counts.items())
    polls = len(f.telegram.requests("getUpdates"))
    with f.telegram.lock:
        f.telegram.faults["getMe"].clear()
    eventually(lambda: len(f.telegram.requests("getUpdates")) > polls)
    assert f.api("callback/answer", agent="alpha", callback_query_id="query-z") == {
        "callback_query_id": "query-z"}
    return "callback reads survive Telegram unavailability; mutations require verified Bot identity"


def verify_edited_mentions(f):
    topic = f.cli("--agent", "alpha", "topic", "create", "--group", str(CHAT),
                  "--name", "Edited mentions")["topic"]
    f.cli("--agent", "beta", "subscribe", topic, "--muted")
    msg, _ = f.send(topic, "Pending without a mention", markup=None)
    assert f.api("wait", agent="beta", topic=topic, timeout=1) == {"topics": []}
    waiter = subprocess.Popen(
        [str(ROOT / "target/debug/tbg"), "--agent", "beta", "wait", "--topic", topic],
        env=f.env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        eventually(lambda: next(agent for agent in f.api("agent/list", agent="alpha")["agents"]
                                if agent["name"] == "beta")["waiting"])
        f.api("edit/text", agent="alpha", topic=topic, msg_id=msg,
              content="Updated: @beta please review")
        output, error = waiter.communicate(timeout=5)
        assert waiter.returncode == 0, (output, error)
        result = json.loads(output)
        assert result["topics"] == [{"topic": topic, "trigger_msg_id": msg, "pending_count": 1}]
    finally:
        if waiter.poll() is None:
            waiter.terminate()
            waiter.wait(timeout=3)
    assert len(f.history(topic)) == 1
    f.cli("--agent", "beta", "ack", topic, "--through", msg)
    before = f.api("subscriptions", agent="beta")
    f.api("edit/text", agent="alpha", topic=topic, msg_id=msg,
          content="Updated again: @beta already acknowledged")
    assert f.api("subscriptions", agent="beta") == before
    assert f.api("unread", agent="beta", topic=topic)["messages"] == []
    assert f.api("wait", agent="beta", topic=topic, timeout=1) == {"topics": []}
    return "edited mentions wake muted unacked waiters with the same ID; acked edits add no pending work"


@contextlib.contextmanager
def record_local_gateway_run():
    REPORT.mkdir(parents=True, exist_ok=True)
    assert (ROOT / "target/debug/tbg-gateway").exists(), "build with --features test-support first"
    results = []
    error = None
    with tempfile.TemporaryDirectory(prefix="tbg-buttons-e2e-") as tmp:
        fixture = Fixture(pathlib.Path(tmp))
        try:
            yield fixture, results
        except BaseException:
            error = traceback.format_exc()
            raise
        finally:
            try:
                fixture.close()
            finally:
                (REPORT / "report.json").write_text(json.dumps(
                    {"passed": results, "failure": error,
                     "scope": "local simulated Telegram integration; not live Telegram"},
                    indent=2) + "\n")
    print(json.dumps({"passed": len(results), "report": str(REPORT / "report.json")}))


def main():
    # Proves button workflows and records a repeatable local integration artifact.
    with record_local_gateway_run() as (fixture, results):
        results.append(prepare_topics(fixture))

        results.append(verify_send_and_validation(fixture))

        results.append(verify_callback_admission(fixture))

        results.append(verify_callback_pagination(fixture))

        results.append(verify_answers(fixture))

        results.append(verify_edits(fixture))

        results.append(verify_edit_rejections(fixture))

        results.append(verify_ambiguous_edit_recovery(fixture))

        results.append(verify_malformed_success(fixture))

        results.append(verify_pending_send_and_restart(fixture))

        results.append(verify_identity_readiness(fixture))

        results.append(verify_edited_mentions(fixture))


if __name__ == "__main__":
    main()
