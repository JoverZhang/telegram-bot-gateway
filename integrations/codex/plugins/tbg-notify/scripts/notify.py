#!/usr/bin/env python3
"""Codex Stop adapter. Never write notification output into the model's input."""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def data_root():
    # Independent of the versioned Codex plugin cache; survives reinstall/uninstall.
    return Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share")) / "tbg/codex"


def save(path, value):
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value, ensure_ascii=False) + "\n")
    temporary.chmod(0o600)
    temporary.replace(path)


def call(config, deadline, *args):
    remaining = deadline - time.monotonic()
    if remaining <= 0.1:
        raise TimeoutError("notification deadline exceeded")
    result = subprocess.run(
        [config["tbg_path"], "--request-timeout-ms", str(max(1, int((remaining - 0.05) * 1000))), *args],
        capture_output=True, text=True, timeout=remaining,
    )
    if result.returncode:
        # CLI errors may contain host details or remote response text. Keep the log bounded.
        raise RuntimeError(f"tbg exited with status {result.returncode}")
    return json.loads(result.stdout)


def notify(event, *, test=False):
    if event.get("hook_event_name") != "Stop":
        return None
    session = event.get("session_id")
    if not isinstance(session, str) or not session or len(session) > 256:
        raise ValueError("Stop requires a session_id of 1–256 characters")
    root = data_root()
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    config = json.loads((root / "config.json").read_text())
    deadline = time.monotonic() + 3
    key = hashlib.sha256(session.encode()).hexdigest()
    sessions = root / "sessions"
    sessions.mkdir(mode=0o700, exist_ok=True)
    with (sessions / f"{key}.lock").open("a") as lock:
        # Fail visibly rather than block Codex behind another hook for this Session.
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        state_path = sessions / f"{key}.json"
        state = json.loads(state_path.read_text()) if state_path.exists() else {}
        if "agent" not in state:
            state["agent"] = call(config, deadline, "agent", "register")["name"]
            save(state_path, state)
        project = Path(event.get("cwd") or ".").name
        # This is a short signal, not a truncated copy of the assistant's answer.
        content = ("TBG notification test" if test else "Codex turn finished") + (
            f"\nProject: {project[:120]}\nAgent: {state['agent']}\nSession: {session}"
        )
        response = call(config, deadline, "--agent", state["agent"], "send", config["topic"], content)
        return {"session_id": session, "topic": config["topic"], "agent": state["agent"],
                "msg_id": response["msg_id"], "status": "accepted"}


def log(record):
    root = data_root()
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    record = {"time": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), **record}
    # One append per event; no message text, token, or raw subprocess diagnostics.
    fd = os.open(root / "notifications.jsonl", os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
    with os.fdopen(fd, "a") as stream:
        stream.write(json.dumps(record, ensure_ascii=False) + "\n")


def main():
    event = {}
    try:
        event = json.load(sys.stdin)
        result = notify(event)
        if result:
            log(result)
    except Exception as error:
        try:
            log({"status": "failed", "error": type(error).__name__,
                 "session_id": event.get("session_id") if isinstance(event, dict) else None})
        except Exception:
            pass
        print(f"tbg notification failed: {type(error).__name__}; see notifications.jsonl", file=sys.stderr)
    # Do not block Stop, inject messages, or expose tbg JSON to Codex.
    print("{}")


if __name__ == "__main__":
    main()
