"""B0/B1 ActionTicket stub (Weighted only, no JEV, no HGRA).

B0       — points at A1 / Browser Use alone (see B01.md).
B1-smoke — guard → ActionTicket → inspect target → harness CDP click (no LLM).

Not yet an invisible Browser Use monkeypatch; that is the full B1 interceptor.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from arms.combo import cdp_click  # noqa: E402
from arms.mcp_stdio import McpStdio  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]


def aui_bin() -> Path:
    release = ROOT / "target" / "release" / "ultra-instinct"
    debug = ROOT / "target" / "debug" / "ultra-instinct"
    if release.exists():
        return release
    if debug.exists():
        return debug
    raise SystemExit("build ultra-instinct first: cargo build -p aui-cli")


def tool_json(mcp: McpStdio, name: str, arguments: dict) -> dict:
    text = mcp.call(name, arguments)
    if text.startswith("ERROR:"):
        raise RuntimeError(text)
    return json.loads(text)


def run_b0() -> int:
    print(
        json.dumps(
            {
                "arm": "b0",
                "ok": True,
                "note": "B0 is Browser Use alone. Run: bench/bench run --arms a1 --scenarios acme-mail",
            },
            indent=2,
        )
    )
    return 0


def run_b1_smoke(cdp: str, server: str, target: str, role: str | None) -> int:
    env = os.environ.copy()
    mcp = McpStdio([str(aui_bin()), "mcp"], env=env)
    try:
        obs = tool_json(mcp, "observe", {"cdp": cdp})
        snap = obs.get("snapshot")
        args: dict = {"cdp": cdp, "target": target, "seen_snapshot": snap}
        if role:
            args["role"] = role
        decision = tool_json(mcp, "guard", args)
        if decision.get("decision") != "allow":
            print(json.dumps({"arm": "b1-smoke", "ok": False, "decision": decision}, indent=2))
            return 1
        ticket = decision.get("ticket")
        if not ticket:
            print("Allow missing ticket", file=sys.stderr)
            return 1
        # Fresh observe so a host interceptor would revalidate world fingerprints.
        after = tool_json(mcp, "observe", {"cdp": cdp})
        insp = tool_json(mcp, "inspect", {"cdp": cdp, "region": ticket["target_id"]})
        # inspect puts geometry on the target object.
        box = insp.get("target") or insp
        x = float(box.get("x", 0))
        y = float(box.get("y", 0))
        w = float(box.get("width", 0))
        h = float(box.get("height", 0))
        err = cdp_click(cdp, server, x + w / 2.0, y + h / 2.0)
        print(
            json.dumps(
                {
                    "arm": "b1-smoke",
                    "ok": err is None,
                    "ticket_id": ticket.get("ticket_id"),
                    "target_id": ticket.get("target_id"),
                    "world_fingerprint": ticket.get("world_fingerprint"),
                    "before_snapshot": snap,
                    "after_snapshot": after.get("snapshot"),
                    "click_error": err,
                    "note": "LLM never saw Ultra-Instinct; harness consumed the Allow ticket",
                },
                indent=2,
            )
        )
        return 0 if err is None else 1
    finally:
        mcp.close()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--arm", choices=["b0", "b1-smoke"], required=True)
    ap.add_argument("--cdp", default="http://127.0.0.1:9222")
    ap.add_argument("--server", default="http://127.0.0.1:8765")
    ap.add_argument("--target", default="Compose")
    ap.add_argument("--role", default="button")
    args = ap.parse_args()
    if args.arm == "b0":
        return run_b0()
    return run_b1_smoke(args.cdp, args.server, args.target, args.role or None)


if __name__ == "__main__":
    raise SystemExit(main())
