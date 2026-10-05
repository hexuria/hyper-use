"""A2: GPT 6 Luna driving cua-driver's typed browser tools over MCP.

Tools shown to the model: get_browser_state (semantic_v2, compacted), browser_click,
browser_type, browser_pointer (scroll/hover), plus done/give_up. The arm injects the
bound target, tab, and session, so the model only picks refs and text.
"""

from __future__ import annotations

import asyncio
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from arms.common import FINISH_TOOLS, Trace, clip, goal_text, load_spec  # noqa: E402
from arms.cua_common import CuaError, compact_snapshot, cua_browser  # noqa: E402

SYSTEM = (
    "You control a web browser through Cua Driver's typed browser tools. get_browser_state returns the page's "
    "actionable refs (like p3:12). Refs are invalidated by newer snapshots and by navigation, so read the state "
    "again after acting. browser_click clicks a ref. browser_type types into an editable ref (replace=true sets "
    "the field). browser_pointer can scroll or hover. When the task is complete call done. If it cannot be done on "
    "this site, call give_up and change nothing."
)

TOOLS = [
    {"type": "function", "function": {"name": "get_browser_state", "description": "Observe the page (semantic snapshot with refs). Optional query narrows to matching content.",
                                      "parameters": {"type": "object", "properties": {"query": {"type": "string"}}}}},
    {"type": "function", "function": {"name": "browser_click", "description": "Click a page element by ref from the latest snapshot. input_route trusted (default, real mouse events; Driver may refuse it for a background window) or dom_event (synthetic el.click(), the route Cua's jev-use example uses).",
                                      "parameters": {"type": "object", "properties": {"ref": {"type": "string"}, "input_route": {"type": "string", "enum": ["trusted", "dom_event"]}}, "required": ["ref"]}}},
    {"type": "function", "function": {"name": "browser_type", "description": "Type text into an editable ref. replace=true replaces the current contents.",
                                      "parameters": {"type": "object", "properties": {"ref": {"type": "string"}, "text": {"type": "string"}, "replace": {"type": "boolean"}}, "required": ["ref", "text"]}}},
    {"type": "function", "function": {"name": "browser_pointer", "description": "hover, scroll (delta_y in CSS px), double_click or right_click a ref.",
                                      "parameters": {"type": "object", "properties": {"action": {"type": "string", "enum": ["hover", "scroll", "double_click", "right_click"]},
                                                                                      "ref": {"type": "string"}, "delta_y": {"type": "number"}}, "required": ["action"]}}},
]


async def main() -> None:
    spec = load_spec()
    trace = Trace(spec["trace"])
    import httpx

    try:
        async with cua_browser(spec, trace) as (driver, ids):
            target = {"target_id": ids["target_id"], "tab_id": ids["tab_id"]}

            async def execute(name: str, args: dict) -> str:
                if name == "get_browser_state":
                    req = {**target, "snapshot_format": "semantic_v2"}
                    if args.get("query"):
                        req["query"] = args["query"]
                    return compact_snapshot(await driver.call("get_browser_state", req))
                if name in {"browser_click", "browser_type", "browser_pointer"}:
                    result = await driver.call(name, {**target, **{k: v for k, v in args.items() if v not in ("", None)}})
                    return json.dumps(result)[:2000]
                return f"unknown tool {name}"

            first = compact_snapshot(await driver.call("get_browser_state", {**target, "snapshot_format": "semantic_v2"}))
            messages = [{"role": "system", "content": SYSTEM},
                        {"role": "user", "content": f"Task: {goal_text(spec)}\n\nCurrent page state:\n{first}"}]
            async with httpx.AsyncClient() as client:
                for _ in range(60):
                    body = {"model": spec["luna_model"], "messages": messages, "tools": TOOLS + FINISH_TOOLS,
                            "tool_choice": "required", "reasoning_effort": spec.get("luna_effort", "low")}
                    resp = await client.post(spec["luna_base"] + "/chat/completions", json=body,
                                             headers={"Authorization": "Bearer local"}, timeout=120)
                    resp.raise_for_status()
                    msg = resp.json()["choices"][0]["message"]
                    calls = msg.get("tool_calls") or []
                    messages.append({"role": "assistant", "content": msg.get("content") or "", **({"tool_calls": calls} if calls else {})})
                    if not calls:
                        messages.append({"role": "user", "content": "Call a tool: act, or call done / give_up."})
                        continue
                    for call in calls:
                        name = call["function"]["name"]
                        try:
                            args = json.loads(call["function"].get("arguments") or "{}")
                        except json.JSONDecodeError:
                            args = {}
                        print(f"TOOL {name} {json.dumps(args)[:300]}", flush=True)
                        if name == "done":
                            trace.final("done", str(args.get("summary", "")))
                            return
                        if name == "give_up":
                            trace.final("give_up", str(args.get("reason", "")))
                            return
                        if name != "get_browser_state":
                            trace.action(json.dumps({"tool": name, "args": args}, sort_keys=True), tool=name)
                        try:
                            result = await execute(name, args)
                        except CuaError as error:
                            result = f"ERROR: {error}"
                        print(f"  -> {result[:400]}", flush=True)
                        messages.append({"role": "tool", "tool_call_id": call["id"], "content": clip(result, 16000)})
            trace.final("error", "turn limit")
    except Exception as error:  # noqa: BLE001
        trace.final("error", f"{type(error).__name__}: {error}")
        raise


if __name__ == "__main__":
    asyncio.run(main())
