#!/usr/bin/env python3
"""Operator entry point. Codex owns installation, enablement and Hook trust."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT / "plugins/tbg-notify/scripts"))
import notify

SELECTOR = "tbg-notify@tbg-codex"


def codex(*args):
    subprocess.run(["codex", "plugin", *args], check=True, timeout=60)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    actions = parser.add_mutually_exclusive_group(required=True)
    actions.add_argument("--topic", help="install/update and select the notification Topic")
    actions.add_argument("--status", action="store_true", help="show config, plugin state and connectivity")
    actions.add_argument("--test", action="store_true", help="send a test notification (creates a test Agent)")
    actions.add_argument("--uninstall", action="store_true", help="remove plugin; retain config, bindings and logs")
    parser.add_argument("--tbg", help="tbg executable; defaults to PATH on first install")
    args = parser.parse_args()
    if args.tbg and not args.topic:
        parser.error("--tbg requires --topic")
    root = notify.data_root()
    config_path = root / "config.json"
    if args.uninstall:
        codex("remove", SELECTOR)
        print("Plugin removed. Configuration, Session bindings and Gateway data retained.")
        return
    if args.status:
        print(f"Plugin: {SELECTOR}\nData: {root}")
        codex("list", "--marketplace", "tbg-codex", "--json")
        config = json.loads(config_path.read_text())
        print(json.dumps(config, indent=2))
        bindings = sorted((root / "sessions").glob("*.json"))
        if not bindings:
            print("Connectivity not checked: no Agent binding. Run --test to register and send.")
            return
        agent = json.loads(bindings[0].read_text())["agent"]
        print(json.dumps(notify.call(config, time.monotonic() + 3, "--agent", agent,
                                     "topic", "show", config["topic"]), indent=2))
        return
    if args.test:
        result = notify.notify({"hook_event_name": "Stop", "session_id": f"test-{uuid.uuid4()}",
                                "cwd": str(Path.cwd())}, test=True)
        notify.log(result)
        print(json.dumps(result, indent=2))
        print("Gateway accepted the message. Verify delivery in Telegram.")
        return
    if not args.topic.strip():
        parser.error("--topic must not be empty")
    existing = json.loads(config_path.read_text()) if config_path.exists() else {}
    executable = shutil.which(args.tbg or existing.get("tbg_path", "tbg"))
    if not executable:
        parser.error("tbg executable not found; install tbg or provide --tbg")
    executable = str(Path(executable).resolve())
    help_result = subprocess.run([executable, "--help"], capture_output=True, text=True, check=True, timeout=3)
    if "--request-timeout-ms" not in help_result.stdout:
        parser.error("tbg is too old; install the CLI shipped with this plugin")
    # Copy to a stable source so deleting the checkout cannot break reinstall.
    source = root / "marketplace"
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    shutil.copytree(ROOT / "plugins", source / "plugins", dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns("__pycache__"))
    shutil.copytree(ROOT / ".agents", source / ".agents", dirs_exist_ok=True)
    codex("marketplace", "add", str(source))
    codex("add", SELECTOR)
    notify.save(config_path, {"topic": args.topic, "tbg_path": executable})
    print("Installed. Enable the plugin and review/trust its Hook in Codex; start a new thread.")
    print("Run --test to verify delivery. Existing notification hooks were not removed.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"tbg plugin setup failed: {error}", file=sys.stderr)
        sys.exit(1)
