"""A1: GPT 6 Luna (OpenCodex, via the counting proxy) driving Browser Use on the harness Chrome."""

from __future__ import annotations

import asyncio
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from arms.common import Trace, goal_text, load_spec  # noqa: E402


async def main() -> None:
    spec = load_spec()
    trace = Trace(spec["trace"])
    from browser_use import Agent, Browser, ChatOpenAI

    llm = ChatOpenAI(
        model=spec["luna_model"],
        base_url=spec["luna_base"],
        api_key="local",
        reasoning_effort=spec.get("luna_effort", "low"),
        reasoning_models=[spec["luna_model"]],
        max_completion_tokens=4096,
    )
    browser = Browser(cdp_url=spec["cdp_http"], keep_alive=True)

    def on_step(_state, model_output, _n) -> None:
        for action in getattr(model_output, "action", None) or []:
            data = action.model_dump(exclude_none=True, exclude_unset=True)
            if "done" in data:
                continue  # finishing is not a step
            trace.action(json.dumps(data, sort_keys=True, default=str), tool=next(iter(data), "?"))

    agent = Agent(
        task=goal_text(spec),
        llm=llm,
        browser=browser,
        register_new_step_callback=on_step,
        use_vision=True,
        directly_open_url=False,
        calculate_cost=False,
    )
    try:
        history = await agent.run(max_steps=spec["caps"]["steps"])
    except Exception as error:  # noqa: BLE001
        trace.final("error", f"{type(error).__name__}: {error}")
        raise
    note = history.final_result() or ""
    if history.is_done():
        trace.final("done" if history.is_successful() else "give_up", note)
    else:
        trace.final("error", "not done: " + "; ".join(str(e) for e in history.errors() if e)[:300])


if __name__ == "__main__":
    asyncio.run(main())
