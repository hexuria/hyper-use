"""Mock arms (no model, no keys) that validate the harness, pages, and checkers.

- oracle:   replays the task's ``oracle`` steps with trusted CDP input; should pass every task
- saboteur: replays ``saboteur`` steps; should trip a forbidden event where one is defined
- spinner:  clicks the same inert heading forever; should end as ``stuck``
- wanderer: alternates between two inert spots; should end as ``cap_hit`` (steps)
- sleeper:  does nothing; should end as ``cap_hit`` (wall clock)
"""

from __future__ import annotations

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from arms.common import Trace, load_spec  # noqa: E402
from chrome import Cdp  # noqa: E402

LOCATE = r"""
(spec) => {
  const visible = (el) => { const r = el.getBoundingClientRect(); const s = getComputedStyle(el);
    return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none'; };
  let doc = document, ox = 0, oy = 0;
  if (spec.frame) { const f = document.querySelector(spec.frame); if (!f) return null;
    const r = f.getBoundingClientRect(); ox = r.left; oy = r.top; doc = f.contentDocument; }
  let scope = doc;
  if (spec.row) {
    const rows = [...doc.querySelectorAll(spec.row.css)];
    scope = rows.find((e) => spec.row.label ? e.getAttribute('aria-label') === spec.row.label
                                            : e.textContent.includes(spec.row.text));
    if (!scope) return null;
  }
  if (spec.shadow) { const host = scope.querySelector(spec.shadow); if (!host || !host.shadowRoot) return null; scope = host.shadowRoot; }
  let els = [...scope.querySelectorAll(spec.css || '*')];
  if (spec.label) els = els.filter((e) => e.getAttribute('aria-label') === spec.label);
  if (spec.text) els = els.filter((e) => e.textContent.trim() === spec.text);
  els = els.filter(visible);
  const el = els[0];
  if (!el) return null;
  el.scrollIntoView({ block: 'center', inline: 'center' });
  const r = el.getBoundingClientRect();
  window.__benchEl = el;
  return { x: ox + r.left + r.width / 2, y: oy + r.top + r.height / 2, n: els.length };
}
"""


class Page:
    def __init__(self, ws: str) -> None:
        self.cdp = Cdp(ws)

    def eval(self, expr: str):
        res = self.cdp.call("Runtime.evaluate", expression=expr, returnByValue=True, awaitPromise=True)
        if "exceptionDetails" in res:
            raise RuntimeError(res["exceptionDetails"].get("text", "eval error"))
        return res.get("result", {}).get("value")

    def locate(self, spec: dict):
        return self.eval(f"({LOCATE})({json.dumps(spec)})")

    def click_xy(self, x: float, y: float) -> None:
        for kind in ("mouseMoved", "mousePressed", "mouseReleased"):
            self.cdp.call("Input.dispatchMouseEvent", type=kind, x=x, y=y, button="left", clickCount=1)

    def click(self, spec: dict) -> None:
        for _ in range(20):
            hit = self.locate(spec)
            if hit:
                time.sleep(0.15)  # let scrollIntoView settle, then re-measure
                hit = self.locate(spec) or hit
                self.click_xy(hit["x"], hit["y"])
                return
            time.sleep(0.25)
        raise RuntimeError(f"not found: {spec}")

    def type(self, spec: dict, text: str) -> None:
        self.click(spec)
        self.eval("(() => { const el = window.__benchEl; el.focus(); if (el.select) el.select(); })()")
        self.cdp.call("Input.insertText", text=text)

    def select(self, spec: dict, value: str) -> None:
        self.click(spec)
        self.eval(f"(() => {{ const el = window.__benchEl; el.value = {json.dumps(value)};"
                  " el.dispatchEvent(new Event('input', {bubbles: true}));"
                  " el.dispatchEvent(new Event('change', {bubbles: true})); el.blur(); })()")

    def key(self, key: str) -> None:
        codes = {"Enter": 13, "Escape": 27, "Tab": 9}
        base = {"key": key, "code": key, "windowsVirtualKeyCode": codes.get(key, 0)}
        self.cdp.call("Input.dispatchKeyEvent", type="keyDown", text="\r" if key == "Enter" else "", **base)
        self.cdp.call("Input.dispatchKeyEvent", type="keyUp", **base)


def replay(page: Page, trace: Trace, steps: list[dict]) -> str:
    for step in steps:
        (kind, arg), = step.items()
        if kind == "final":
            return arg
        trace.action(json.dumps(step, sort_keys=True), tool=kind)
        if kind == "click":
            page.click(arg)
        elif kind == "type":
            page.type({k: v for k, v in arg.items() if k != "text"}, arg["text"])
        elif kind == "select":
            page.select({k: v for k, v in arg.items() if k != "value"}, arg["value"])
        elif kind == "key":
            page.key(arg)
        elif kind == "eval":
            page.eval(arg)
        elif kind == "seek":
            for _ in range(arg.get("max", 3)):
                if page.locate(arg["target"]):
                    break
                page.click(arg["next"])
                time.sleep(0.3)
        elif kind == "wait":
            time.sleep(float(arg))
        else:
            raise ValueError(f"unknown step {kind}")
        time.sleep(0.35)
    return "done"


def main() -> None:
    spec = load_spec()
    trace = Trace(spec["trace"])
    mode = spec["arm_options"]["mode"]
    page = Page(spec["page_ws"])
    time.sleep(0.8)  # page load
    if mode in {"oracle", "saboteur"}:
        steps = spec["task_steps"].get(mode) or []
        if not steps:
            trace.final("done", f"no {mode} steps")
            return
        trace.final(replay(page, trace, steps))
    elif mode == "spinner":
        while True:
            trace.action(json.dumps({"click": "h1"}))
            page.click({"css": "h1, h2, h3"})
            time.sleep(0.4)
    elif mode == "wanderer":
        i = 0
        while True:
            target = ["h1, h2, h3", ".sub, p"][i % 2]
            trace.action(json.dumps({"click": target}))
            page.click({"css": target})
            i += 1
            time.sleep(0.3)
    elif mode == "sleeper":
        while True:
            time.sleep(1)


if __name__ == "__main__":
    main()
