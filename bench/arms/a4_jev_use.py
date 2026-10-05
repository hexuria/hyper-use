"""A4: Cua's jev-use recipe (Cua Driver semantic_v2 refs + JEV choosing one candidate id), no declared steps.

The pinned example's only task (FixtureFormTask) is hard-coded, so the arm supplies a
generic browser task with the same shape: every step it reads ``get_browser_state``
(semantic_v2), offers one candidate per actionable ref (click buttons/links/toggles;
"type <quoted literal from the goal>" into text fields; scroll), plus the recipe's
reserved ``reobserve`` and ``abstain`` and a ``done`` candidate, and asks JEV through
the recipe's own ``choose_for_task``. No per-task steps and no oracle peeking: the arm
stops only on done/abstain or when the harness kills it.
"""

from __future__ import annotations

import asyncio
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from arms.common import Trace, goal_text, load_spec  # noqa: E402
from arms.cua_common import CuaError, cua_browser  # noqa: E402

CLICK_ROLES = {"button", "link", "checkbox", "radio", "switch", "tab", "menuitem", "menuitemradio", "menuitemcheckbox",
               "option", "gridcell", "combobox", "listbox", "treeitem", "row", "cell"}
TEXT_ROLES = {"textbox", "searchbox", "textarea"}
VISIBLE = {"in_viewport", "near_viewport", None}
MAX_CANDIDATES = 140


@dataclass
class Cand:
    id: str
    description: str
    tool: str | None
    arguments: dict
    sig: str


@dataclass
class GenericTask:
    goal: str
    snapshot: dict | None = None

    def redact(self, value):
        return value

    def state_summary(self, _sources):
        page = (self.snapshot or {}).get("page") or {}
        return {"url": str(page.get("url")), "title": str(page.get("title"))}


def literals(goal: str) -> list[str]:
    return list(dict.fromkeys(re.findall(r'"([^"]+)"', goal)))


def build_candidates(snap: dict, goal: str, target: dict) -> list[Cand]:
    out: list[Cand] = []
    quoted = literals(goal)
    for r in snap.get("refs") or []:
        role, name, ref = r.get("role"), r.get("name"), r.get("ref")
        if not ref or r.get("visibility") not in VISIBLE:
            continue
        states = r.get("states") or {}
        label = f'{role} "{name}"' if name else role
        flags = ", ".join(f"{k}={v}" for k, v in states.items() if k in {"checked", "pressed", "selected", "expanded", "disabled"})
        where = f" [{flags}]" if flags else ""
        editable = role in TEXT_ROLES or states.get("editable")
        if editable and role not in {"generic"}:
            current = r.get("value")
            for text in quoted:
                out.append(Cand("", f'Type "{text}" into {label}{where}, replacing its contents (now {json.dumps(current) if current else "empty"}).',
                                "browser_type", {**target, "ref": ref, "text": text, "replace": True}, f"type|{role}|{name}|{text}"))
        elif role in CLICK_ROLES and "click" in (r.get("actions") or []) and (name or role in {"checkbox", "radio", "switch"}):
            out.append(Cand("", f"Click {label}{where}.", "browser_click", {**target, "ref": ref, "input_route": "dom_event"},
                            f"click|{role}|{name}"))
    out = out[:MAX_CANDIDATES]
    out.append(Cand("", "Scroll the page down to reveal more content.", "browser_pointer",
                    {**target, "action": "scroll", "x": 640, "y": 400, "delta_y": 600}, "scroll|down"))
    out.append(Cand("", "Scroll the page up.", "browser_pointer", {**target, "action": "scroll", "x": 640, "y": 400, "delta_y": -600}, "scroll|up"))
    for i, c in enumerate(out):
        c.id = f"a{i}"
    out.append(Cand("done", "The goal is fully achieved in the observed page state; stop now.", None, {}, "done"))
    out.append(Cand("reobserve", "Take no action and obtain a fresh Driver observation, because the current observation is stale.", None, {}, "reobserve"))
    out.append(Cand("abstain", "Stop without acting: the goal cannot be done on this site or none of the actions is safe.", None, {}, "abstain"))
    return out


async def main() -> None:
    spec = load_spec()
    trace = Trace(spec["trace"])
    sys.path.insert(0, str(Path(spec["root"]) / "bench/vendor/cua/libs/cua-driver/examples/jev-use/python"))
    from core import Candidate  # noqa: E402
    from jev_adapter import choose_for_task  # noqa: E402
    from tasks import fixture_sources  # noqa: E402
    from typesafe_sdk import TypeSafeClient  # noqa: E402

    goal = goal_text(spec)
    task = GenericTask(goal)
    history: list[dict] = []
    try:
        async with cua_browser(spec, trace) as (driver, ids):
            target = {"target_id": ids["target_id"], "tab_id": ids["tab_id"]}
            with TypeSafeClient() as client:
                for step in range(1, 200):
                    snap = await driver.call("get_browser_state", {**target, "snapshot_format": "semantic_v2"})
                    task.snapshot = snap
                    cands = build_candidates(snap, goal, target)
                    recipe = [Candidate(c.id, c.description, c.tool, c.arguments) for c in cands]
                    choice, confidence, _probs = await asyncio.to_thread(
                        choose_for_task, client, task, fixture_sources(snap), recipe, history)
                    picked = next(c for c in cands if c.id == choice)
                    print(f"STEP {step} {picked.id} {picked.description[:160]} conf={confidence:.2f}", flush=True)
                    if picked.id == "done":
                        trace.final("done", "JEV chose done")
                        return
                    if picked.id == "abstain":
                        trace.final("give_up", "JEV chose abstain")
                        return
                    if picked.id == "reobserve":
                        history.append({"step": step, "selected_id": "reobserve", "outcome": "took no action and observed again"})
                        continue
                    trace.action(picked.sig, tool=picked.tool)
                    try:
                        result = await driver.call(picked.tool, picked.arguments)
                        outcome = f"{picked.description} Driver: {result.get('effect', result.get('status', 'ok'))}"
                    except CuaError as error:
                        outcome = f"{picked.description} Driver refused: {str(error)[:200]}"
                    history.append({"step": step, "selected_id": picked.id, "outcome": outcome})
                    history = history[-12:]
                    await asyncio.sleep(0.3)
    except Exception as error:  # noqa: BLE001
        trace.final("error", f"{type(error).__name__}: {error}"[:400])
        raise


if __name__ == "__main__":
    asyncio.run(main())
