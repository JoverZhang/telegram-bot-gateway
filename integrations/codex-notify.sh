#!/bin/sh
# Usage: codex-notify.sh <agent> <topic> '<Codex notify JSON>'
# Codex appends the JSON argument. Notification errors must not fail the task.
python3 - "$@" <<'PY'
import json
import subprocess
import sys

try:
    agent, topic, payload = sys.argv[1:]
    event = json.loads(payload)
    if event.get("type") == "agent-turn-complete":
        content = event.get("last-assistant-message") or "Task completed."
        result = subprocess.run(
            ["tbg", "--agent", agent, "send", topic, content],
            stdout=subprocess.DEVNULL,
            timeout=45,
        )
        if result.returncode:
            print("tbg notification was not accepted; see CLI error above", file=sys.stderr)
except Exception as error:
    print(f"tbg notification failed: {type(error).__name__}", file=sys.stderr)
PY
exit 0
