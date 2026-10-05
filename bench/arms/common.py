"""Shared bits for arm wrappers: the spec, the trace, the goal text, and a Luna tool loop.

Every arm is a separate process started by ``bench/run.py`` with one argument,
the path of a JSON spec. It must write a JSONL trace at ``spec["trace"]``:

- ``{"ev": "action", "sig": "<stable action signature>", ...}`` before each action
- ``{"ev": "final", "status": "done" | "give_up" | "error", "note": "..."}`` once at the end

The harness counts steps from these action lines (and from the page journal),
enforces every cap from outside, and may kill the process group at any time.
Only depends on the standard library plus httpx (present in both bench envs).
"""

from __future__ import annotations

import json
import sys
import time
from pathlib import Path

# One instruction appended to the goal for every free-text arm, identical across arms.
GOAL_SUFFIX = (
    " The page is already open in the browser. Use only this site. If the task cannot be done "
    "on this site, stop and say so (give up) without changing anything."
)


def load_spec() -> dict:
    return json.loads(Path(sys.argv[1]).read_text())


class Trace:
    def __init__(self, path: str) -> None:
        self.path = Path(path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.t0 = time.time()
        self.finished = False

    def write(self, ev: str, **fields) -> None:
        row = {"ev": ev, "t": round(time.time() - self.t0, 3), **fields}
        with self.path.open("a", encoding="utf-8") as fh:
            fh.write(json.dumps(row, default=str) + "\n")
            fh.flush()

    def action(self, sig: str, **fields) -> None:
        self.write("action", sig=sig, **fields)

    def final(self, status: str, note: str = "") -> None:
        if not self.finished:
            self.finished = True
            self.write("final", status=status, note=note[:500])


def goal_text(spec: dict) -> str:
    return spec["task"]["goal"] + GOAL_SUFFIX


FINISH_TOOLS = [
    {
        "type": "function",
        "function": {
            "name": "done",
            "description": "Call when the task is complete. Summarize what you did.",
            "parameters": {"type": "object", "properties": {"summary": {"type": "string"}}, "required": ["summary"]},
        },
    },
    {
        "type": "function",
        "function": {
            "name": "give_up",
            "description": "Call when the task cannot be done on this site (for example the control does not exist). Nothing should be changed.",
            "parameters": {"type": "object", "properties": {"reason": {"type": "string"}}, "required": ["reason"]},
        },
    },
]


def chat(spec: dict, messages: list[dict], tools: list[dict], client) -> dict:
    """One Luna chat-completions call through the counting proxy."""
    body = {
        "model": spec["luna_model"],
        "messages": messages,
        "tools": tools,
        "tool_choice": "required",
        "reasoning_effort": spec.get("luna_effort", "low"),
    }
    resp = client.post(spec["luna_base"].rstrip("/") + "/chat/completions", json=body,
                       headers={"Authorization": "Bearer local"}, timeout=120)
    if resp.status_code == 429:
        raise RuntimeError("model-call cap reached")
    resp.raise_for_status()
    return resp.json()


def clip(text: str, limit: int) -> str:
    return text if len(text) <= limit else text[:limit] + f"\n...[truncated {len(text) - limit} chars]"


def tool_loop(spec: dict, trace: Trace, system: str, first_user: str, tools: list[dict], execute,
              observe_tools: frozenset[str] = frozenset(), max_turns: int = 60, tool_result_limit: int = 12000) -> None:
    """Generic Luna loop: the model calls tools until done/give_up.

    ``execute(name, args) -> str`` runs a non-finish tool. Calls named in
    ``observe_tools`` are reads, not actions, and are not traced as steps.
    """
    import httpx

    messages = [{"role": "system", "content": system}, {"role": "user", "content": first_user}]
    all_tools = tools + FINISH_TOOLS
    with httpx.Client() as client:
        for _ in range(max_turns):
            reply = chat(spec, messages, all_tools, client)
            msg = reply["choices"][0]["message"]
            calls = msg.get("tool_calls") or []
            messages.append({"role": "assistant", "content": msg.get("content") or "", "tool_calls": calls} if calls
                            else {"role": "assistant", "content": msg.get("content") or ""})
            if not calls:
                messages.append({"role": "user", "content": "Call a tool: act, or call done / give_up."})
                continue
            for call in calls:
                name = call["function"]["name"]
                try:
                    args = json.loads(call["function"].get("arguments") or "{}")
                except json.JSONDecodeError:
                    args = {}
                if name in ("done", "give_up"):
                    print(f"FINISH {name} {json.dumps(args)[:300]}", flush=True)
                if name == "done":
                    trace.final("done", str(args.get("summary", "")))
                    return
                if name == "give_up":
                    trace.final("give_up", str(args.get("reason", "")))
                    return
                if name not in observe_tools:
                    trace.action(json.dumps({"tool": name, "args": args}, sort_keys=True), tool=name)
                try:
                    result = execute(name, args)
                except Exception as error:  # report to the model, keep going
                    result = f"ERROR: {type(error).__name__}: {error}"
                print(f"TOOL {name} {json.dumps(args)[:300]}\n  -> {str(result)[:600]}", flush=True)
                messages.append({"role": "tool", "tool_call_id": call["id"], "content": clip(str(result), tool_result_limit)})
    trace.final("error", "turn limit")
