"""A5: JEV + ultra-instinct through the generalized ``live_drive`` example (bench mode).

Runs ``target/release/examples/live_drive --task-json ... --trace ...`` against the
harness Chrome and translates its JSONL log into the bench trace while it runs:
each ultra-instinct ``act`` call is a step; the final outcome maps done -> done,
give_up -> give_up, anything else -> error. JEV calls go through the proxy
(TYPESAFE_BASE_URL is set by the harness).
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from arms.common import Trace, goal_text, load_spec  # noqa: E402

ACTING = {"act"}


def main() -> None:
    spec = load_spec()
    trace = Trace(spec["trace"])
    root = Path(spec["root"])
    out = Path(spec["out"])
    task_file = out / "live_drive_task.json"
    task_file.write_text(json.dumps({"id": spec["task"]["id"], "start_url": spec["start_url"], "text": goal_text(spec)}))
    log_path = out / "live_drive.jsonl"
    log_path.unlink(missing_ok=True)
    argv = [str(root / "target/release/examples/live_drive"), "--bin", str(root / "target/release/ultra-instinct"),
            "--cdp", spec["cdp_http"], "--site", spec["server"], "--out", str(out / "live_drive_out"),
            "--task-json", str(task_file), "--trace", str(log_path), "--max-steps", str(spec["caps"]["steps"])]
    env = {**os.environ, "ULTRA_INSTINCT_JEV": "1"}
    proc = subprocess.Popen(argv, cwd=str(root), env=env, stdout=sys.stdout, stderr=subprocess.STDOUT)
    seen = 0
    outcome = None

    def pump() -> None:
        nonlocal seen, outcome
        if not log_path.exists():
            return
        lines = log_path.read_text(encoding="utf-8", errors="replace").splitlines()
        for line in lines[seen:]:
            try:
                ev = json.loads(line)
            except json.JSONDecodeError:
                continue
            if ev.get("kind") == "mcp" and ev.get("tool") in ACTING:
                trace.action(json.dumps({"tool": ev["tool"], "args": ev.get("arguments")}, sort_keys=True), tool=ev["tool"])
            elif ev.get("kind") == "final":
                outcome = ev.get("outcome")
        seen = len(lines)

    try:
        while proc.poll() is None:
            pump()
            time.sleep(0.2)
        pump()
    finally:
        if proc.poll() is None:
            proc.terminate()
    status = {"done": "done", "give_up": "give_up"}.get(outcome or "", "error")
    trace.final(status, f"live_drive outcome: {outcome} (exit {proc.returncode})")


if __name__ == "__main__":
    main()
