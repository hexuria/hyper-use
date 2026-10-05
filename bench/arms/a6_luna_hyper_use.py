"""A6: GPT 6 Luna driving hyper-use's MCP tools (observe, locate, inspect, act, verify, diff).

The harness Chrome's CDP endpoint is injected into every call, so the model never
handles endpoints. hyper-use's act supports press/click only: there is no typing
or select tool, which the report shows as a capability gap (tasks tagged needs).
"""

from __future__ import annotations

import copy
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from arms.common import Trace, goal_text, load_spec, tool_loop  # noqa: E402
from arms.mcp_stdio import McpStdio  # noqa: E402

HIDDEN = {"cdp", "fixture", "executor"}
# The shipped MCP schemas list every knob with no enums and no mutual-exclusion rules, and Luna
# filled them all (UnknownPosition, DimsRequireHgra, ConfidenceWithQuery, EmptyText). The arm shows
# the same tools with the knobs the default path needs; values come from hyper-use's CLI help.
KEEP = {
    "observe": [],
    "locate": ["text", "role", "position"],
    "inspect": ["region"],
    "act": ["region", "confidence", "runner_up"],
    "verify": ["expect_text", "expect_absent"],
    "diff": ["before_snapshot", "after_snapshot"],
}
ENUMS = {"position": ["left", "right", "top", "bottom", "center"]}
SYSTEM = (
    "You control a web browser only through the hyper-use tools. observe lists page regions "
    "(id, role, label, state). locate ranks regions for a query. inspect shows one region. act presses one "
    "region id (pass locate's top confidence and runner_up when you have them). verify checks a postcondition. "
    "locate takes text (the label to match), optional role (an ARIA role such as button, link, checkbox) and "
    "optional position (left, right, top, bottom, center) to separate look-alike controls. Omit arguments you do "
    "not need. "
    "There is no typing tool. Work step by step: observe or locate, act, then check the result. When the task is "
    "complete call done. If it cannot be done with these tools on this site, call give_up and change nothing."
)


def main() -> None:
    spec = load_spec()
    trace = Trace(spec["trace"])
    root = Path(spec["root"])
    server = McpStdio([str(root / "target/release/hyper-use"), "mcp"])
    try:
        tools = []
        for tool in server.tools():
            schema = copy.deepcopy(tool.get("inputSchema") or {"type": "object", "properties": {}})
            keep = KEEP.get(tool["name"])
            props = schema.get("properties", {})
            if keep is not None:
                props = {k: v for k, v in props.items() if k in keep}
            for key in HIDDEN:
                props.pop(key, None)
            for key, values in ENUMS.items():
                if key in props:
                    props[key] = {**props[key], "enum": values}
            schema["properties"] = props
            schema["required"] = [r for r in schema.get("required", []) if r in props]
            tools.append({"type": "function", "function": {"name": tool["name"], "description": tool.get("description", "")[:1000],
                                                           "parameters": schema}})

        def execute(name: str, args: dict) -> str:
            args = {k: v for k, v in args.items() if k not in HIDDEN and v not in ("", None)}
            if name not in {"diff"}:
                args["cdp"] = spec["cdp_http"]
            return server.call(name, args)

        tool_loop(spec, trace, SYSTEM, "Task: " + goal_text(spec), tools, execute,
                  observe_tools=frozenset({"observe", "locate", "inspect", "verify", "diff"}))
    except Exception as error:  # noqa: BLE001
        trace.final("error", f"{type(error).__name__}: {error}")
        raise
    finally:
        server.close()


if __name__ == "__main__":
    main()
