"""Shared orchestrator for the combined arms A7 (Luna + JEV + ultra-instinct + CUA) and
A8 (Luna + JEV + ultra-instinct + Browser Use). Protocol: ``bench/arms/COMBO.md``.

Roles, one owner each:
- Luna (GPT 6 Luna, OpenCodex) is the planner: a tool-calling loop that states intents.
- ultra-instinct (``ultra-instinct mcp`` over CDP) observes and locates; this harness CDP-clicks after a confidence-gated pick (product ``act``/``guard`` never clicks).
- JEV (TypeSafe systemone) picks one candidate id whenever the deterministic ranker
  cannot separate the top two (ultra-instinct's own act gate: top >= 0.55 and margin >= 0.05),
  and for executor fallbacks when the label match is not clear. JEV always has a NONE option.
- The executor (CUA driver for A7, Browser Use for A8) does what ultra-instinct cannot:
  type, select, scroll, a read of the page in its own format, and a gated fallback
  click that is only allowed right after a ultra-instinct press failed.

Only the standard library, httpx, and websocket-client (both in the main bench env).
"""

from __future__ import annotations

import json
import math
import os
import re
import time
import urllib.request
from pathlib import Path

import httpx

from arms.common import Trace, goal_text, tool_loop
from arms.mcp_stdio import McpStdio

GATE_TOP = 0.55       # ultra-instinct act gate (crates/aui-executor): top confidence floor
GATE_MARGIN = 0.05    # and margin over the runner-up
PLAUSIBLE = 0.5       # a locate text miss is capped at 0.45, so >= 0.5 means some text matched
MAX_JEV_CANDS = 24
NOISE_ROLES = {"generic", "image", "statictext", "none", "presentation", "img", "text", "paragraph", "group"}

SYSTEM = (
    "You control a web browser through a combined toolset. ultra-instinct is the primary engine: observe lists page "
    "regions, press clicks one control by describing it (the engine ranks matches, a picker model breaks ties using "
    "the goal, and a safety gate refuses unclear clicks). ultra-instinct cannot type or choose dropdown values, so "
    "type_text and select_option run on a second browser executor ({executor}). read_page shows the page through "
    "that executor (values, checked states, iframes) when observe is not detailed enough. fallback_click uses the "
    "executor to click, and is only allowed right after a press failed (no match, refused, or no visible effect). "
    "Describe targets by their visible label; add context (for example the row, card, or panel it belongs to) when "
    "several controls share a label, and position (left, right, top, bottom, center) when that separates them. "
    "Every action returns the updated page. Work step by step and check the result. When the task is complete call "
    "done. If it cannot be done on this site, call give_up and change nothing."
)

# ultra-instinct's role vocabulary (crates/aui-core/src/vocab.rs); any other role is an UnknownRole error.
ROLE_ENUM = ["button", "link", "checkbox", "menuitem", "tab", "text_field", "slider", "generic", "image", "heading", "navigation", "text"]
TOOLS = [
    {"type": "function", "function": {
        "name": "observe", "description": "ultra-instinct: list the page's regions (id, role, label, state).",
        "parameters": {"type": "object", "properties": {}}}},
    {"type": "function", "function": {
        "name": "press", "description": "ultra-instinct: click one control. target is its visible label. context names the row/card/panel it is in when labels repeat.",
        "parameters": {"type": "object", "properties": {
            "target": {"type": "string"}, "context": {"type": "string"}, "role": {"type": "string", "enum": ROLE_ENUM},
            "position": {"type": "string", "enum": ["left", "right", "top", "bottom", "center"]}}, "required": ["target"]}}},
    {"type": "function", "function": {
        "name": "type_text", "description": "Executor: type text into a field (replaces its contents). field is the field's label or placeholder.",
        "parameters": {"type": "object", "properties": {"field": {"type": "string"}, "text": {"type": "string"}, "context": {"type": "string"}},
                       "required": ["field", "text"]}}},
    {"type": "function", "function": {
        "name": "select_option", "description": "Executor: choose an option in a dropdown/select. field is the dropdown's label, option the option text.",
        "parameters": {"type": "object", "properties": {"field": {"type": "string"}, "option": {"type": "string"}, "context": {"type": "string"}},
                       "required": ["field", "option"]}}},
    {"type": "function", "function": {
        "name": "scroll", "description": "Executor: scroll the page.",
        "parameters": {"type": "object", "properties": {"direction": {"type": "string", "enum": ["down", "up"]}}, "required": ["direction"]}}},
    {"type": "function", "function": {
        "name": "read_page", "description": "Executor: read the page in the executor's own format (values, checked states, iframes).",
        "parameters": {"type": "object", "properties": {}}}},
    {"type": "function", "function": {
        "name": "fallback_click", "description": "Executor: click a control ultra-instinct could not press. Only allowed right after a failed press.",
        "parameters": {"type": "object", "properties": {"target": {"type": "string"}, "context": {"type": "string"}}, "required": ["target"]}}},
]
READ_TOOLS = frozenset({"observe", "read_page"})


def tokens(text: str) -> list[str]:
    return re.findall(r"[a-z0-9]+", (text or "").lower())


def label_score(query: str, desc: str) -> float:
    """Share of query tokens present in the candidate description, +0.5 for an exact label."""
    q, d = tokens(query), set(tokens(desc))
    if not q:
        return 0.0
    hit = sum(1 for t in q if t in d) / len(q)
    return hit


# ---------------------------------------------------------------- CDP read helper (context only)

def page_ws_url(cdp_http: str, server: str) -> str | None:
    try:
        with urllib.request.urlopen(cdp_http.rstrip("/") + "/json/list", timeout=3) as resp:
            targets = json.loads(resp.read())
    except Exception:
        return None
    pages = [t for t in targets if t.get("type") == "page"]
    pages.sort(key=lambda t: not str(t.get("url", "")).startswith(server))
    return pages[0]["webSocketDebuggerUrl"] if pages else None


def cdp_eval(cdp_http: str, server: str, expression: str):
    import websocket

    ws_url = page_ws_url(cdp_http, server)
    if not ws_url:
        return None
    try:
        ws = websocket.create_connection(ws_url, timeout=5, suppress_origin=True)
    except Exception:
        return None
    try:
        ws.send(json.dumps({"id": 1, "method": "Runtime.evaluate", "params": {"expression": expression, "returnByValue": True}}))
        while True:
            msg = json.loads(ws.recv())
            if msg.get("id") == 1:
                return ((msg.get("result") or {}).get("result") or {}).get("value")
    except Exception:
        return None
    finally:
        ws.close()



def cdp_click(cdp_http: str, server: str, x: float, y: float) -> str | None:
    """Harness-owned click (firewall ``act``/``guard`` never presses). Returns None on success."""
    import websocket

    ws_url = page_ws_url(cdp_http, server)
    if not ws_url:
        return "no page websocket"
    try:
        ws = websocket.create_connection(ws_url, timeout=5, suppress_origin=True)
    except Exception as err:
        return f"ws connect failed: {err}"
    try:
        for i, kind in enumerate(("mousePressed", "mouseReleased"), start=1):
            ws.send(json.dumps({
                "id": i,
                "method": "Input.dispatchMouseEvent",
                "params": {"type": kind, "x": x, "y": y, "button": "left", "clickCount": 1},
            }))
            ws.recv()
    except Exception as err:
        return f"dispatchMouseEvent failed: {err}"
    finally:
        try:
            ws.close()
        except Exception:
            pass
    time.sleep(0.35)
    return None


def region_ids(observe_raw: str) -> set[str]:
    try:
        data = json.loads(observe_raw.removeprefix("ERROR: "))
    except json.JSONDecodeError:
        return set()
    return {str(r.get("id")) for r in (data.get("regions") or []) if r.get("id")}


CONTAINER_JS = """(() => {
  const X = %f, Y = %f;
  const SEL = 'button,a,input,select,textarea,summary,label,[role],[tabindex]';
  const STRUCT = 'li,tr,[role=row],[role=listitem],[role=gridcell],article,[role=dialog],dialog,fieldset';
  // Descend through open shadow roots: document.elementsFromPoint retargets shadow content to its host.
  let root = document, pick = null;
  for (let i = 0; i < 8; i++) {
    const els = root.elementsFromPoint(X, Y).filter(e => root === document || root.contains(e));
    if (!els.length) break;
    pick = els.find(e => e.matches(SEL)) || els[0];
    const host = els.find(e => e.shadowRoot && e.shadowRoot !== root);
    if (host && (host === els[0] || !els[0].matches(SEL))) { root = host.shadowRoot; continue; }
    break;
  }
  if (!pick) return '';
  const up = n => n.parentElement || (n.parentNode && n.parentNode.host) || null;
  const own = (pick.innerText || pick.getAttribute('aria-label') || '').trim();
  let s = null;
  for (let c = pick; c; c = up(c)) { if (c.matches && c.matches(STRUCT)) { s = c; break; } }
  let t = s ? (s.innerText || '') : '';
  if (!t.trim() || t.length > 400) {
    t = '';
    for (let c = up(pick); c && c !== document.body; c = up(c)) {
      const x = (c.innerText || '').trim();
      if (x.length > own.length + 24) { t = x; break; }  // past sibling-button strips to the card/section text
    }
  }
  return t.replace(/\\s+/g, ' ').trim().slice(0, 220);
})()"""


# ---------------------------------------------------------------- JEV picker

class Jev:
    def __init__(self, spec: dict, trace: Trace) -> None:
        self.url = spec["typesafe_base"].rstrip("/") + "/v1/systemone"
        self.model = spec.get("jev_model", "jev-latest")
        self.key = os.environ.get("TYPESAFE_API_KEY", "")
        self.trace = trace
        self.client = httpx.Client(timeout=30)
        self.calls = 0

    def pick(self, goal: str, intent: str, page: dict, criteria: dict[str, str]) -> tuple[str, dict[str, float], float]:
        """One choice over ``criteria`` plus NONE. Returns (choice, probabilities, confidence)."""
        if not self.key:
            raise RuntimeError("TYPESAFE_API_KEY missing; JEV picker unavailable")
        crit = dict(criteria)
        crit["NONE"] = "None of the listed elements matches the intended target; do not act."
        body = {
            "model": self.model,
            "state": {"goal": goal, "intent": intent, "page": page},
            "questions": {"target": {"type": "choice", "criteria": crit, "instructions": {
                "goal": goal, "intent": intent,
                "rules": "Pick the one element that the intent refers to, using its label, role, state and surrounding "
                         "context. Prefer the element whose context matches the goal (row, card, panel, dialog). "
                         "Choose NONE when no element fits; never pick a merely similar control."}}},
        }
        self.calls += 1
        resp = self.client.post(self.url, json=body, headers={"Authorization": f"Bearer {self.key}"})
        if resp.status_code == 429:
            raise RuntimeError("model-call cap reached")
        resp.raise_for_status()
        ans = resp.json()["answers"]["target"]
        probs = {k: float(v) for k, v in (ans.get("probabilities") or {}).items()}
        choice = ans["choice"]
        conf = float(ans.get("confidence", probs.get(choice, 0.0)))
        if choice not in crit or not all(math.isfinite(p) and 0 <= p <= 1 for p in probs.values()):
            raise ValueError("invalid JEV answer")
        self.trace.write("jev", intent=intent[:200], n=len(criteria), choice=choice, p=round(probs.get(choice, conf), 3),
                         criteria=json.dumps(criteria)[:1500])
        return choice, probs, conf


# ---------------------------------------------------------------- ultra-instinct

def compact_observe(raw: str, limit: int = 220) -> str:
    try:
        data = json.loads(raw.removeprefix("ERROR: "))
    except json.JSONDecodeError:
        return raw[:4000]
    regions = data.get("regions") or []
    lines = []
    shown = 0
    for r in regions:
        label = (r.get("label") or "").strip()
        role = r.get("role") or ""
        if not label and role in NOISE_ROLES:
            continue
        st = r.get("state") or {}
        flags = [v for k, v in st.items() if v not in ("enabled", "visible", None) and isinstance(v, str)]
        lines.append(f"{r.get('id')} {role} {json.dumps(label[:120])}{' [' + ','.join(flags) + ']' if flags else ''}")
        shown += 1
        if shown >= limit:
            lines.append(f"... {len(regions)} regions total, rest not shown")
            break
    return "REGIONS (id role \"label\" [state]):\n" + "\n".join(lines)


class Combo:
    def __init__(self, spec: dict, trace: Trace, executor, cdp_http: str) -> None:
        self.spec = spec
        self.trace = trace
        self.ex = executor
        self.cdp = cdp_http
        self.goal = goal_text(spec)
        self.jev = Jev(spec, trace)
        # ultra-instinct gets no TypeSafe key: JEV is called by this orchestrator only, so every JEV call is counted here.
        self.hu = McpStdio([str(Path(spec["root"]) / "target/release/ultra-instinct"), "mcp"],
                           env={k: v for k, v in os.environ.items() if k != "TYPESAFE_API_KEY"})
        self.last_press_failed: str | None = None
        self.stats = {"press_ultra_instinct": 0, "press_jev": 0, "press_refused": 0, "press_no_match": 0,
                      "fallback": 0, "type": 0, "select": 0}

    # -- ultra-instinct calls
    def hcall(self, name: str, args: dict) -> dict | str:
        raw = self.hu.call(name, {**args, "cdp": self.cdp})
        try:
            return json.loads(raw.removeprefix("ERROR: ")) if not raw.startswith("ERROR: ") else raw
        except json.JSONDecodeError:
            return raw

    def observe_text(self) -> str:
        return compact_observe(self.hu.call("observe", {"cdp": self.cdp}))

    def page_info(self) -> dict:
        info = cdp_eval(self.cdp, self.spec["server"], "({url: location.href, title: document.title})") or {}
        return {"url": info.get("url"), "title": info.get("title")}

    def context_of(self, region_id: str) -> tuple[str, dict | None]:
        found = self.hcall("inspect", {"region": region_id})
        if not isinstance(found, dict):
            return "", None
        t = found.get("target") or found
        try:
            cx, cy = t["x"] + t["width"] / 2, t["y"] + t["height"] / 2
        except (KeyError, TypeError):
            return "", None
        text = cdp_eval(self.cdp, self.spec["server"], CONTAINER_JS % (cx, cy)) or ""
        return text, {"x": round(t["x"]), "y": round(t["y"]), "w": round(t["width"]), "h": round(t["height"])}

    # -- tools
    def press(self, args: dict) -> str:
        target = str(args.get("target", "")).strip()
        context = str(args.get("context", "") or "").strip()
        q = {"text": target}
        if args.get("role"):
            q["role"] = args["role"]
        if args.get("position") in {"left", "right", "top", "bottom", "center"}:
            q["position"] = args["position"]
        loc = self.hcall("locate", q)
        if not isinstance(loc, dict) and "role" in q:
            q.pop("role")
            loc = self.hcall("locate", q)
        if not isinstance(loc, dict):
            self.last_press_failed = target
            return f"press failed: ultra-instinct locate error {str(loc)[:300]}"
        cands = loc.get("candidates") or []
        plaus = [c for c in cands if c.get("confidence", 0) >= PLAUSIBLE][:8]
        if not plaus:
            self.stats["press_no_match"] += 1
            self.last_press_failed = target
            top = ", ".join(f"{c.get('id')} {c.get('role')} {json.dumps(c.get('label'))} {c.get('confidence')}" for c in cands[:5])
            return (f"NO MATCH: ultra-instinct found no region labelled like {json.dumps(target)} (text misses score 0.45). "
                    f"Closest: {top}. Nothing was clicked. If the control exists but ultra-instinct cannot see it (iframe, canvas), "
                    f"fallback_click is now allowed; if it does not exist, give_up.")
        top, second = plaus[0], (plaus[1] if len(plaus) > 1 else None)
        clear = top["confidence"] >= GATE_TOP and (second is None or top["confidence"] - second["confidence"] >= GATE_MARGIN)
        picker = "ultra-instinct"
        if clear and not context:
            region, conf = top["id"], top["confidence"]
            runner = {"id": second["id"], "confidence": second["confidence"]} if second else (
                {"id": cands[1]["id"], "confidence": cands[1]["confidence"]} if len(cands) > 1 else None)
        elif clear and context and second is None:
            region, conf = top["id"], top["confidence"]
            runner = {"id": cands[1]["id"], "confidence": cands[1]["confidence"]} if len(cands) > 1 else None
        else:
            picker = "jev"
            criteria = {}
            for c in plaus:
                ctx, rect = self.context_of(c["id"])
                st = c.get("state") or {}
                criteria[c["id"]] = json.dumps({"label": c.get("label"), "role": c.get("role"),
                                                "availability": st.get("availability"), "visibility": st.get("visibility"),
                                                "box": rect, "container_text": ctx})
            intent = f"click {json.dumps(target)}" + (f" in {json.dumps(context)}" if context else "") + \
                     (f" ({args['position']})" if args.get("position") else "")
            choice, probs, _conf = self.jev.pick(self.goal, intent, self.page_info(), criteria)
            if choice == "NONE":
                self.stats["press_refused"] += 1
                self.last_press_failed = target
                return (f"REFUSED: the picker found none of {len(plaus)} candidates matching {json.dumps(target)}"
                        f"{' in ' + json.dumps(context) if context else ''}. Nothing was clicked.")
            others = sorted(((p, k) for k, p in probs.items() if k not in (choice, "NONE")), reverse=True)
            region, conf = choice, probs.get(choice, 0.0)
            runner = {"id": others[0][1], "confidence": others[0][0]} if others else None
        act_args = {"region": region, "confidence": round(conf, 4)}
        if runner:
            act_args["runner_up"] = {"id": runner["id"], "confidence": round(runner["confidence"], 4)}
        self.trace.write("press", picker=picker, region=region, confidence=act_args["confidence"], runner_up=runner)
        # Firewall-era ``act`` is guard-only (never clicks) and re-ranks with an empty
        # query when only ``region``+``confidence`` are passed, so JEV's host pick
        # always hits proposed-not-top. Restore the COMBO.md contract: after the
        # confidence gate, this harness CDP-clicks the picked region (same as
        # live_drive after Allow). Optionally refuse when guard says front-layer.
        gargs = {"text": target, "proposed": region}
        if args.get("role"):
            gargs["role"] = args["role"]
        if args.get("position") in {"left", "right", "top", "bottom", "center"}:
            gargs["position"] = args["position"]
        guard = self.hcall("guard", gargs)
        if isinstance(guard, dict):
            reason = guard.get("reason") or guard.get("fallback")
            if reason == "front-layer":
                self.stats["press_refused"] += 1
                self.last_press_failed = target
                return (f"REFUSED by ultra-instinct gate ({picker} pick {region}, confidence {act_args['confidence']}): "
                        f"{{\"reason\": \"front-layer\"}}. Nothing was clicked.\n" + self.observe_text())
            # proposed-not-top / low-confidence / etc.: host already disambiguated via
            # JEV or a clear locate; click the pick (pre-firewall act semantics).
        found = self.hcall("inspect", {"region": region})
        if not isinstance(found, dict):
            self.stats["press_refused"] += 1
            self.last_press_failed = target
            return f"REFUSED: inspect failed for {region}: {str(found)[:300]}. Nothing was clicked.\n" + self.observe_text()
        box = found.get("target") or found
        try:
            cx = float(box["x"]) + float(box["width"]) / 2.0
            cy = float(box["y"]) + float(box["height"]) / 2.0
        except (KeyError, TypeError, ValueError) as err:
            self.stats["press_refused"] += 1
            self.last_press_failed = target
            return f"REFUSED: no box for {region}: {err}. Nothing was clicked.\n" + self.observe_text()
        before_raw = self.hu.call("observe", {"cdp": self.cdp})
        before_ids = region_ids(before_raw)
        click_err = cdp_click(self.cdp, self.spec["server"], cx, cy)
        if click_err:
            self.stats["press_refused"] += 1
            self.last_press_failed = target
            return f"REFUSED: CDP click failed for {region}: {click_err}. Nothing was clicked.\n" + self.observe_text()
        after_raw = self.hu.call("observe", {"cdp": self.cdp})
        after_ids = region_ids(after_raw)
        added_ids = sorted(after_ids - before_ids)
        removed_ids = sorted(before_ids - after_ids)
        # Detect label/state churn on the pressed control via a second inspect.
        after_found = self.hcall("inspect", {"region": region})
        changed = {}
        if isinstance(after_found, dict):
            at = after_found.get("target") or after_found
            bt = box
            if (at.get("label") or "") != (bt.get("label") or ""):
                changed["text_changed"] = [region]
            bst = (bt.get("state") if isinstance(bt.get("state"), dict) else {}) or {}
            ast = (at.get("state") if isinstance(at.get("state"), dict) else {}) or {}
            if bst != ast:
                changed.setdefault("changed", [region])
        self.stats["press_" + ("jev" if picker == "jev" else "ultra_instinct")] += 1
        no_effect = not added_ids and not removed_ids and not changed
        self.last_press_failed = target if no_effect else None
        return (f"PRESSED {region} ({picker} pick, confidence {act_args['confidence']}). delta: +{len(added_ids)} -{len(removed_ids)} regions"
                f"{' ' + json.dumps(changed)[:300] if changed else ''}{' (no visible effect)' if no_effect else ''}"
                f"\n" + compact_observe(after_raw))

    def pick_executor(self, kind: str, query: str, context: str, elements: list[dict]) -> tuple[dict | None, str]:
        """elements: [{"id", "desc", "context"}] of the executor kind. Label match, JEV on ambiguity."""
        if not elements:
            return None, f"no {kind} elements on the page (executor view)"
        full = f"{query} {context}".strip()
        scored = sorted(((label_score(query, e["desc"]) + 0.25 * label_score(context, e.get("context", "")) if context else
                          label_score(query, e["desc"]), i) for i, e in enumerate(elements)), reverse=True)
        best, second = scored[0], (scored[1] if len(scored) > 1 else (0.0, -1))
        if best[0] >= 0.99 and best[0] - second[0] >= 0.25:
            return elements[best[1]], "label match"
        pool = [elements[i] for s, i in scored if s > 0][:MAX_JEV_CANDS] or [elements[i] for _, i in scored][:MAX_JEV_CANDS]
        criteria = {f"e{n}": json.dumps({"element": e["desc"][:200], "context": e.get("context", "")[:200]}) for n, e in enumerate(pool)}
        choice, probs, _ = self.jev.pick(self.goal, f"{kind} target: {full}", self.page_info(), criteria)
        if choice == "NONE":
            return None, f"picker found no {kind} element matching {json.dumps(full)}"
        p = probs.get(choice, 0.0)
        others = sorted((v for k, v in probs.items() if k not in (choice, "NONE")), reverse=True)
        if p < GATE_TOP or (others and p - others[0] < GATE_MARGIN):
            return None, f"picker unsure between look-alike {kind} elements (p={p:.2f}); add context"
        return pool[int(choice[1:])], f"JEV pick p={p:.2f}"

    def after(self, head: str) -> str:
        time.sleep(0.3)
        return head + "\n" + self.observe_text()

    def type_text(self, args: dict) -> str:
        el, how = self.pick_executor("type", str(args.get("field", "")), str(args.get("context", "") or ""), self.ex.elements("type"))
        if el is None:
            return f"NOT TYPED: {how}. Nothing changed."
        self.stats["type"] += 1
        return self.after(f"TYPED via {self.ex.name} into {el['desc'][:120]} ({how}): {self.ex.type(el, str(args.get('text', '')))}")

    def select_option(self, args: dict) -> str:
        el, how = self.pick_executor("select", str(args.get("field", "")), str(args.get("context", "") or ""), self.ex.elements("select"))
        if el is None:
            return f"NOT SELECTED: {how}. Nothing changed."
        self.stats["select"] += 1
        return self.after(f"SELECTED via {self.ex.name} in {el['desc'][:120]} ({how}): {self.ex.select(el, str(args.get('option', '')))}")

    def fallback_click(self, args: dict) -> str:
        if not self.last_press_failed:
            return "NOT ALLOWED: fallback_click is only for a control ultra-instinct could not press. Call press first."
        el, how = self.pick_executor("click", str(args.get("target", "")), str(args.get("context", "") or ""), self.ex.elements("click"))
        if el is None:
            return f"NOT CLICKED: {how}. Nothing changed."
        self.stats["fallback"] += 1
        self.last_press_failed = None
        return self.after(f"CLICKED via {self.ex.name} {el['desc'][:120]} ({how}): {self.ex.click(el)}")

    def execute(self, name: str, args: dict) -> str:
        if name == "observe":
            return self.observe_text()
        if name == "press":
            return self.press(args)
        if name == "type_text":
            return self.type_text(args)
        if name == "select_option":
            return self.select_option(args)
        if name == "scroll":
            return self.after(f"SCROLLED {args.get('direction')}: {self.ex.scroll(args.get('direction') != 'up')}")
        if name == "read_page":
            return self.ex.read()
        if name == "fallback_click":
            return self.fallback_click(args)
        return f"unknown tool {name}"

    def run(self) -> None:
        system = SYSTEM.format(executor=self.ex.name)
        first = f"Task: {self.goal}\n\nCurrent page (ultra-instinct observe):\n{self.observe_text()}"
        try:
            tool_loop(self.spec, self.trace, system, first, TOOLS, self.execute, observe_tools=READ_TOOLS)
        finally:
            self.trace.write("combo_stats", **self.stats, jev_calls=self.jev.calls)
            self.hu.close()
