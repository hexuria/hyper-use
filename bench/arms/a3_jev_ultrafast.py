"""A3: jev-ultrafast (browser-harness + JEV choices, text fields from a small LLM) on the harness Chrome.

Pinned at bench/vendor/jev-ultrafast. Three bench patches, all by monkeypatch, no vendor edits:
- the JEV systemone URL goes to the counting proxy (``spec["typesafe_base"]``),
- the 1120x780 device-metrics override becomes the bench's 1280x800,
- the text helper is GPT 6 Luna through the proxy (its upstream default is DeepSeek).
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from arms.common import Trace, goal_text, load_spec  # noqa: E402


def main() -> None:
    spec = load_spec()
    trace = Trace(spec["trace"])
    os.environ.update({
        "BU_CDP_URL": spec["cdp_http"],
        "BU_NAME": spec["bu_name"],
        "TEXT_MODEL_BASE_URL": spec["luna_base"],
        "TEXT_MODEL": spec["luna_model"],
        "TEXT_MODEL_API_KEY": "local",
        "TYPESAFE_MODEL": spec.get("jev_model", "jev-latest"),
    })
    sys.path.insert(0, str(Path(spec["root"]) / "bench/vendor/jev-ultrafast"))
    from jev_ultrafast import browser as jbrowser
    from jev_ultrafast import model as jmodel
    from jev_ultrafast.agent import Agent

    real_post = jmodel.post_json

    def post_json(url, key, body):
        return real_post(url.replace("https://api.typesafe.ai", spec["typesafe_base"]), key, body)

    jmodel.post_json = post_json
    real_call = jbrowser.Browser.call
    vp = spec["viewport"]

    def call(self, method, **params):
        if method == "Emulation.setDeviceMetricsOverride":
            params.update(width=vp["width"], height=vp["height"])
        return real_call(self, method, **params)

    jbrowser.Browser.call = call
    agent = None
    try:
        agent = Agent(spec["start_url"], goal_text(spec))
        seen = 0
        state = None
        for state in agent.run():
            history = state.get("history") or []
            for item in history[seen:]:
                trace.action(json.dumps({"action": item.get("action"), "kind": item.get("kind"), "text": item.get("text")}, sort_keys=True),
                             tool=item.get("kind"))
            seen = len(history)
        decisions = (state or {}).get("decisions") or []
        last = decisions[-1].get("choice") if decisions else None
        status = (state or {}).get("status")
        if status == "done":
            trace.final("done", "DONE")
        elif last == "BLOCKED":
            trace.final("give_up", "JEV chose BLOCKED")
        else:
            trace.final("error", f"stopped: {status} (repeat guard or budget)")
    except Exception as error:  # noqa: BLE001
        trace.final("error", f"{type(error).__name__}: {error}"[:400])
        raise
    finally:
        try:
            if agent:
                agent.close()
        finally:
            try:
                from browser_harness.admin import restart_daemon
                restart_daemon(spec["bu_name"])
            except Exception:
                pass


if __name__ == "__main__":
    main()
