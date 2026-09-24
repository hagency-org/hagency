#!/usr/bin/env python3
"""Opt-in real-client qualification. Never invoked by offline tests or CI.

Uses an already-authenticated Robrix automation rig and an already-provisioned
isolated native service. It does not create accounts, approve requests, recover
failed work, or restart either process. Failed observations stop the soak.
"""
import argparse
import html
import json
import re
import shlex
import subprocess
import sys
import time
from pathlib import Path


def run(command, timeout=30):
    result = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        # Never emit arbitrary stderr, environment or private rig configuration.
        raise RuntimeError("qualification command refused")
    return result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--action", type=Path, required=True)
    parser.add_argument("--ssh-host", required=True)
    parser.add_argument("--state-dir", required=True)
    parser.add_argument("--container", default="hagency-rust-live")
    parser.add_argument("--docker", default="/opt/homebrew/bin/docker")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--seconds", type=int, default=3600)
    parser.add_argument("--pulse-seconds", type=int, default=60)
    args = parser.parse_args()
    if not 60 <= args.seconds <= 43200 or not 30 <= args.pulse_seconds <= 600:
        parser.error("duration/pulse outside qualification bounds")
    if not re.fullmatch(r"[A-Za-z0-9_.@-]+", args.ssh_host) or args.ssh_host.startswith("-"):
        parser.error("invalid SSH target")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+", args.container):
        parser.error("invalid container")
    if not args.action.is_file() or not args.state_dir.startswith("/"):
        parser.error("existing action script and absolute private state are required")
    args.out.mkdir(mode=0o700, parents=True, exist_ok=False)
    started = time.monotonic()
    prefix = "HAGENCY_RUST_SOAK_" + time.strftime("%Y%m%dT%H%M%S", time.gmtime())
    log = args.out / "observations.jsonl"

    def record(kind, **values):
        value = {"kind": kind, "elapsed_seconds": round(time.monotonic() - started, 3),
                 "recorded_at_ms": int(time.time() * 1000), **values}
        with log.open("a", encoding="utf-8") as stream:
            stream.write(json.dumps(value) + "\n")
        print(json.dumps(value), flush=True)

    def action(value):
        return json.loads(run([sys.executable, str(args.action), json.dumps(value)], 65))

    def sql(query):
        command = "/usr/bin/sqlite3 -readonly " + shlex.quote(args.state_dir + "/domain.sqlite3") + " " + shlex.quote(query)
        return run(["ssh", args.ssh_host, command]).strip()

    def status():
        inner = "token=$(tr -d '\\r\\n' < " + shlex.quote(args.state_dir + "/operator.token") + "); curl -fsS --max-time 5 -H \"Authorization: Bearer $token\" http://127.0.0.1:13300/api/native/v1/capabilities"
        command = " ".join(map(shlex.quote, [args.docker, "exec", args.container, "sh", "-c", inner]))
        value = json.loads(run(["ssh", args.ssh_host, command]))["development_execution"]
        if value.get("error") or value.get("state") in {"outcome_unknown", "closed"}:
            record("native_refusal", state=value.get("state"), error=value.get("error"),
                   matrix_error=value.get("matrix_error"), owned_failure=value.get("owned_failure"))
            raise RuntimeError("native owner is not healthy")
        client = action({"action": "status"})
        if client.get("exit") is not None or not client.get("startup"):
            raise RuntimeError("Robrix is not running")
        return value.get("state")

    baseline = int(sql("SELECT count(*) FROM runner_dispatches WHERE state='outcome_unknown';"))
    record("started", duration_seconds=args.seconds, pulse_seconds=args.pulse_seconds,
           preserved_unknown_dispatches=baseline, evidence="real Robrix automation + native durable reply + rendered HTML")
    next_pulse = started
    next_health = started
    sequence = 0
    pending = None
    passed = 0
    try:
        while time.monotonic() - started < args.seconds or pending:
            now = time.monotonic()
            if now >= next_health:
                phase = status()
                unknown = int(sql("SELECT count(*) FROM runner_dispatches WHERE state='outcome_unknown';"))
                if unknown != baseline:
                    raise RuntimeError("new unsettled dispatch observed")
                record("healthy", native_state=phase, passed_pulses=passed)
                next_health = now + 30
            if pending:
                ack, sent = pending
                # ACK is locally generated alphanumeric text, never source SQL.
                row = sql("SELECT state || '|' || coalesce(event_id,'') FROM final_replies WHERE body='" + ack + "';")
                if row.startswith("delivered|"):
                    widgets = action({"action": "snapshot"})["widgets"]
                    rendered = any(widget.get("widget_type") == "Html"
                        and html.unescape(re.sub(r"<[^>]*>", "", widget.get("text", ""))).strip() == ack
                        for widget in widgets)
                    if rendered:
                        capture = action({"action": "capture", "label": prefix + "_" + str(sequence)})
                        record("pulse_passed", sequence=sequence, ack=ack, reply_event_id=row.split("|", 1)[1],
                               response_seconds=round(now - sent, 3), frame=capture.get("frame"))
                        passed += 1
                        pending = None
                        next_pulse = max(next_pulse, now + 1)
                if pending and now - sent > 65:
                    raise RuntimeError("reply was not durably delivered and rendered within 65 seconds")
            elif now >= next_pulse and now - started < args.seconds:
                sequence += 1
                ack = prefix + "_" + str(sequence).zfill(3) + "_ACK"
                action({"action": "click", "id": "text_input"})
                action({"action": "text", "text": "Reply exactly " + ack + " in this encrypted DM. Use get_task and complete_task_with_reply for the assigned canonical task. Do not call other tools, run commands, ask for input or modify files."})
                action({"action": "click", "id": "send_message_button", "wait": 1})
                pending = (ack, time.monotonic())
                next_pulse = now + args.pulse_seconds
                record("pulse_sent", sequence=sequence, ack=ack)
            time.sleep(2)
        status()
        if not passed:
            raise RuntimeError("no completed pulse")
        record("soak_passed", passed_pulses=passed, scope="encrypted owner-DM task/reply only; not entire-port parity")
        return 0
    except (RuntimeError, subprocess.TimeoutExpired, OSError, ValueError, KeyError):
        record("soak_failed", passed_pulses=passed, pending_sequence=sequence if pending else None,
               reason="qualification observation failed; private command output withheld")
        return 1


if __name__ == "__main__":
    sys.exit(main())
