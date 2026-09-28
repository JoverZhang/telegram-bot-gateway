"""Shared notification workflow and its persistent state/CLI boundaries."""

import fcntl
import hashlib
import json
import os
import re
import subprocess
import sys
import time
from contextlib import contextmanager
from dataclasses import asdict, dataclass
from pathlib import Path


@dataclass(frozen=True)
class NotificationConfig:
    topic: str
    tbg_path: str


class NotificationError(Exception):
    """A failed workflow step, with diagnostics safe to record without message text."""

    def __init__(self, step, detail):
        super().__init__(f"{step}: {detail}")
        self.step = step


def data_root():
    # Bindings survive changes to Codex's versioned plugin cache.
    return (
        Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share"))
        / "tbg/codex"
    )


def load_config():
    path = data_root() / "config.json"
    try:
        value = json.loads(path.read_text())
        if not isinstance(value, dict) or any(
            not isinstance(value.get(field), str) or not value[field].strip()
            for field in ("topic", "tbg_path")
        ):
            raise ValueError("topic and tbg_path must be nonempty strings")
        return NotificationConfig(value["topic"], value["tbg_path"])
    except (OSError, ValueError) as error:
        raise NotificationError(
            "load_config", f"invalid or unreadable {path}"
        ) from error


def save_config(config):
    _save_private_json(data_root() / "config.json", asdict(config), "save_config")


def _save_private_json(path, value, step):
    try:
        path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        temporary = path.with_suffix(".tmp")
        temporary.write_text(json.dumps(value, ensure_ascii=False) + "\n")
        temporary.chmod(0o600)
        temporary.replace(path)
    except OSError as error:
        raise NotificationError(
            step, f"cannot save {path}: {error.strerror}"
        ) from error


def _request_gateway(config, deadline, step, *args):
    remaining = deadline - time.monotonic()
    if remaining <= 0.1:
        raise NotificationError(step, "notification deadline exceeded")
    try:
        result = subprocess.run(
            [
                config.tbg_path,
                "--request-timeout-ms",
                str(max(1, int((remaining - 0.05) * 1000))),
                *args,
            ],
            capture_output=True,
            text=True,
            timeout=remaining,
        )
    except subprocess.TimeoutExpired as error:
        raise NotificationError(
            step, "tbg exceeded the notification deadline"
        ) from error
    except OSError as error:
        raise NotificationError(
            step, f"cannot execute tbg: {error.strerror}"
        ) from error
    if result.returncode:
        # Preserve exit/HTTP status, without logging command arguments or remote text.
        http_status = re.search(r"\bHTTP (\d{3})\b", result.stderr)
        detail = f"tbg exited with status {result.returncode}"
        if http_status:
            detail += f" (HTTP {http_status[1]})"
        raise NotificationError(step, detail)
    try:
        return json.loads(result.stdout)
    except ValueError as error:
        raise NotificationError(step, "tbg returned invalid JSON") from error


@contextmanager
def lock_session(session_id):
    key = hashlib.sha256(session_id.encode()).hexdigest()
    sessions = data_root() / "sessions"
    try:
        sessions.mkdir(parents=True, exist_ok=True, mode=0o700)
        lock = (sessions / f"{key}.lock").open("a")
    except OSError as error:
        raise NotificationError(
            "lock_session", f"cannot open Session lock: {error.strerror}"
        ) from error
    with lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise NotificationError(
                "lock_session", "another notification owns this Session"
            ) from error
        except OSError as error:
            raise NotificationError(
                "lock_session", f"cannot acquire Session lock: {error.strerror}"
            ) from error
        yield sessions / f"{key}.json"


def load_agent_binding(path):
    try:
        value = json.loads(path.read_text())
        if (
            not isinstance(value, dict)
            or not isinstance(value.get("agent"), str)
            or not value["agent"]
        ):
            raise ValueError("binding requires an Agent name")
        return value["agent"]
    except (OSError, ValueError) as error:
        raise NotificationError(
            "restore_agent", f"invalid or unreadable binding {path}"
        ) from error


def restore_or_register_agent(binding_path, config, deadline):
    if binding_path.exists():
        return load_agent_binding(binding_path)
    agent = _request_gateway(config, deadline, "register_agent", "agent", "register")[
        "name"
    ]
    _save_private_json(binding_path, {"agent": agent}, "save_agent_binding")
    return agent


def resolve_project(cwd, deadline):
    # Git's common directory identifies the original repository for linked worktrees.
    fallback = Path(cwd).name or "Unknown project"
    remaining = deadline - time.monotonic()
    if remaining <= 0.1:
        return fallback
    try:
        result = subprocess.run(
            [
                "git",
                "-C",
                cwd,
                "rev-parse",
                "--show-toplevel",
                "--path-format=absolute",
                "--git-common-dir",
            ],
            capture_output=True,
            text=True,
            timeout=min(0.5, remaining),
        )
        if result.returncode == 0:
            top, common = result.stdout.strip().splitlines()
            common = Path(common)
            return common.parent.name if common.name == ".git" else Path(top).name
    except (OSError, ValueError, subprocess.TimeoutExpired):
        pass  # Project display is best effort and must not suppress a notification.
    return fallback


def format_notification(agent, project, content):
    project = " ".join(project.splitlines())[:120]
    project = re.sub(r"([\\`*_{}\[\]()#+.!<>|~-])", r"\\\1", project)
    # Stop events currently expose no session title; use the agreed fallback.
    return f"**Codex 本轮完成**\n项目：{project}\n#{agent}\n\n{content}"


def send_turn_notification(session_id, content, agent, config, deadline):
    response = _request_gateway(
        config,
        deadline,
        "send_notification",
        "--agent",
        agent,
        "send",
        config.topic,
        "--format",
        "markdown",
        "--no-header",
        "--",
        content,
    )
    return {
        "session_id": session_id,
        "topic": config.topic,
        "agent": agent,
        "msg_id": response["msg_id"],
        "status": "accepted",
    }


def deliver_notification(session_id, content, cwd="."):
    # Sends a titled Markdown response under the Session's Agent tag.
    config = load_config()

    deadline = time.monotonic() + 3
    project = resolve_project(cwd, deadline)

    with lock_session(session_id) as binding_path:
        agent = restore_or_register_agent(binding_path, config, deadline)

        message = format_notification(agent, project, content)

        receipt = send_turn_notification(session_id, message, agent, config, deadline)

    record_delivery(receipt)

    return receipt


def check_topic(config):
    bindings = sorted((data_root() / "sessions").glob("*.json"))
    if not bindings:
        return None
    agent = load_agent_binding(bindings[0])
    return _request_gateway(
        config,
        time.monotonic() + 3,
        "check_topic",
        "--agent",
        agent,
        "topic",
        "show",
        config.topic,
    )


def record_delivery(record):
    root = data_root()
    record = {"time": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), **record}
    try:
        root.mkdir(parents=True, exist_ok=True, mode=0o700)
        fd = os.open(
            root / "notifications.jsonl", os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600
        )
        with os.fdopen(fd, "a") as stream:
            stream.write(json.dumps(record, ensure_ascii=False) + "\n")
    except OSError as error:
        # Logging failure must not turn an accepted send into an apparent rejection.
        print(
            f"tbg notification log failed: {error.strerror}; "
            f"status={record['status']}, msg_id={record.get('msg_id')}",
            file=sys.stderr,
        )
