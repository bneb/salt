#!/usr/bin/env python3
"""Salt federation heartbeat (WS-R Sprint-0 telemetry).

Writes .round1-staging/state/heartbeat.json at every round close-out so
monitoring is a pure file-read (no session access needed). An external
cron can alert when unix_ts goes stale; crash recovery checks one file.

Usage:
  python3 scripts/heartbeat.py --round N --commit SHA \
      --suite-pass P --suite-fail F --note "one line"

Any gate field omitted carries forward from the previous beat, so a
close-out only needs to pass what changed. Exit codes: 0 written,
2 bad invocation.
"""
import argparse
import json
import pathlib
import sys
import time

STATE = pathlib.Path(__file__).resolve().parent.parent / ".round1-staging" / "state" / "heartbeat.json"


def main() -> int:
    parser = argparse.ArgumentParser(description="Append a Salt round heartbeat")
    parser.add_argument("--round", type=int, required=True)
    parser.add_argument("--commit", default="")
    parser.add_argument("--suite-pass", type=int, default=None)
    parser.add_argument("--suite-fail", type=int, default=None)
    parser.add_argument("--z3-pass", type=int, default=None)
    parser.add_argument("--proof", default="")
    parser.add_argument("--det", default="")
    parser.add_argument("--note", default="")
    args = parser.parse_args()

    prev = {}
    if STATE.exists():
        try:
            prev = json.loads(STATE.read_text())
        except json.JSONDecodeError:
            prev = {}

    def carry(field: str, fallback=""):
        value = getattr(args, field)
        return prev.get(field, fallback) if value is None else value

    now = time.time()
    beat = {
        "unix_ts": int(now),
        "iso": time.strftime("%Y-%m-%dT%H:%M:%S", time.localtime(now)),
        "minutes_since_prev": (
            None if not prev else round((now - prev.get("unix_ts", now)) / 60, 1)
        ),
        "round": args.round,
        "commit": args.commit or prev.get("commit", ""),
        "suite_pass": carry("suite_pass"),
        "suite_fail": carry("suite_fail"),
        "z3_pass": carry("z3_pass"),
        "proof": carry("proof"),
        "det": carry("det"),
        "note": args.note,
    }
    STATE.parent.mkdir(parents=True, exist_ok=True)
    STATE.write_text(json.dumps(beat, indent=2) + "\n")
    delta = beat["minutes_since_prev"]
    delta_s = "first beat" if delta is None else f"+{delta}min"
    print(f"heartbeat: round {beat['round']} @ {beat['iso']} ({delta_s})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
