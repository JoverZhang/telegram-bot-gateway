#!/usr/bin/env python3
"""Codex Stop entry point; failures must never block the originating turn."""

import json
import sys

from notifications import (
    NotificationError,
    data_root,
    deliver_notification,
    record_delivery,
)


def parse_stop_event():
    try:
        event = json.load(sys.stdin)
    except (OSError, ValueError) as error:
        raise NotificationError(
            "parse_event", "cannot read a valid JSON event"
        ) from error
    if not isinstance(event, dict):
        raise NotificationError("parse_event", "expected a JSON object")
    if event.get("hook_event_name") != "Stop":
        return None
    session = event.get("session_id")
    if not isinstance(session, str) or not session or len(session) > 256:
        raise NotificationError(
            "parse_event", "session_id must contain 1–256 characters"
        )
    content = event.get("last_assistant_message")
    if content is not None and not isinstance(content, str):
        raise NotificationError(
            "parse_event", "last_assistant_message must be a string or null"
        )
    if content is None or not content.strip():
        content = "Codex turn finished (no final response)."
    return session, content


def report_failure(error, session_id):
    detail = (
        str(error) if isinstance(error, NotificationError) else type(error).__name__
    )
    record = {
        "status": "failed",
        "error": type(error).__name__,
        "detail": detail,
        "session_id": session_id,
    }
    try:
        record_delivery(record)
    except Exception as log_error:
        print(
            f"tbg notification log failed: {type(log_error).__name__}", file=sys.stderr
        )
    print(
        f"tbg notification failed: {detail}; log: {data_root() / 'notifications.jsonl'}",
        file=sys.stderr,
    )


def main():
    # Converts a main-session Stop into a notification without changing Codex control flow.
    event = None
    try:
        event = parse_stop_event()

        if event is not None:
            deliver_notification(*event)
    except Exception as error:
        report_failure(error, event[0] if event else None)
    finally:
        # The outer Hook boundary deliberately handles unexpected failures too.
        print("{}")


if __name__ == "__main__":
    main()
