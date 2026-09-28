#!/usr/bin/env python3
"""Install, inspect, test or remove the native Codex notification plugin."""

import argparse
import json
import shutil
import subprocess
import sys
import uuid
from dataclasses import asdict
from pathlib import Path

from notifications import (
    NotificationConfig,
    NotificationError,
    check_topic,
    data_root,
    deliver_notification,
    load_config,
    save_config,
)

# The shipped layout is <marketplace>/plugins/tbg-notify/scripts/install.py.
MARKETPLACE_ROOT = Path(__file__).resolve().parents[3]
SELECTOR = "tbg-notify@tbg-codex"


def parse_request():
    parser = argparse.ArgumentParser(description=__doc__)
    actions = parser.add_mutually_exclusive_group(required=True)
    actions.add_argument(
        "--topic", help="install/update and select the notification Topic"
    )
    actions.add_argument(
        "--status",
        action="store_true",
        help="show config, plugin state and connectivity",
    )
    actions.add_argument(
        "--test",
        action="store_true",
        help="send a test notification (creates a test Agent)",
    )
    actions.add_argument(
        "--uninstall",
        action="store_true",
        help="remove plugin; retain config, bindings and logs",
    )
    parser.add_argument(
        "--tbg", help="tbg executable; defaults to PATH on first install"
    )
    args = parser.parse_args()
    if args.tbg is not None and args.topic is None:
        parser.error("--tbg requires --topic")
    if args.topic is not None and not args.topic.strip():
        parser.error("--topic must not be empty")
    return args


def resolve_install_config(request):
    existing = load_config() if (data_root() / "config.json").exists() else None
    requested_cli = request.tbg or (existing.tbg_path if existing else "tbg")
    executable = shutil.which(requested_cli)
    if not executable:
        raise ValueError("tbg executable not found; install tbg or provide --tbg")
    executable = str(Path(executable).resolve())
    result = subprocess.run(
        [executable, "--help"],
        capture_output=True,
        text=True,
        check=True,
        timeout=3,
    )
    if "--request-timeout-ms" not in result.stdout:
        raise ValueError("tbg is too old; install the CLI shipped with this plugin")
    send_help = subprocess.run(
        [executable, "send", "--help"],
        capture_output=True,
        text=True,
        check=True,
        timeout=3,
    )
    if "--format" not in send_help.stdout or "--no-header" not in send_help.stdout:
        raise ValueError(
            "tbg is too old; install a CLI and Gateway with Markdown send support"
        )
    return NotificationConfig(request.topic, executable)


def stage_marketplace():
    source = data_root() / "marketplace"
    source.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    shutil.copytree(
        MARKETPLACE_ROOT / "plugins",
        source / "plugins",
        dirs_exist_ok=True,
        ignore=shutil.ignore_patterns("__pycache__"),
    )
    shutil.copytree(
        MARKETPLACE_ROOT / ".agents", source / ".agents", dirs_exist_ok=True
    )
    return source


def install_native_plugin(source):
    subprocess.run(
        ["codex", "plugin", "marketplace", "add", str(source)], check=True, timeout=60
    )
    subprocess.run(["codex", "plugin", "add", SELECTOR], check=True, timeout=60)


def install_plugin(request):
    # Installs a stable plugin source and saves the selected notification target.
    config = resolve_install_config(request)

    source = stage_marketplace()

    install_native_plugin(source)

    save_config(config)

    print(
        "Installed. Enable the plugin and review/trust its Hook in Codex; start a new thread."
    )
    print(
        "Run --test to verify delivery. Existing notification hooks were not removed."
    )


def show_status():
    # Reports native installation state and checks the configured Topic when a binding exists.
    print(f"Plugin: {SELECTOR}\nData: {data_root()}", flush=True)
    subprocess.run(
        ["codex", "plugin", "list", "--marketplace", "tbg-codex", "--json"],
        check=True,
        timeout=60,
    )
    config = load_config()
    print(json.dumps(asdict(config), indent=2))
    topic = check_topic(config)
    if topic is None:
        print(
            "Connectivity not checked: no Agent binding. Run --test to register and send."
        )
    else:
        print(json.dumps(topic, indent=2))


def send_test_notification():
    receipt = deliver_notification(
        f"test-{uuid.uuid4()}",
        "TBG notification test.\n\nFinal-response content will appear here.",
    )
    print(json.dumps(receipt, indent=2))
    print("Gateway accepted the message. Verify delivery in Telegram.")


def uninstall_plugin():
    subprocess.run(["codex", "plugin", "remove", SELECTOR], check=True, timeout=60)
    print("Plugin removed. Configuration, Session bindings and Gateway data retained.")


def main():
    # Dispatches one operator request without modifying unrelated Codex Hooks.
    request = parse_request()

    if request.status:
        show_status()
    elif request.test:
        send_test_notification()
    elif request.uninstall:
        uninstall_plugin()
    else:
        install_plugin(request)


if __name__ == "__main__":
    try:
        main()
    except (
        NotificationError,
        OSError,
        ValueError,
        subprocess.SubprocessError,
    ) as error:
        print(f"tbg plugin setup failed: {error}", file=sys.stderr)
        sys.exit(1)
