"""Score one attempt from the page journal only (never from an agent's own claims).

Strict pass = every success predicate holds, zero forbidden events, and the
attempt did not end in ``cap_hit`` or ``stuck``. Target-missing tasks also need a
refusal final status from the arm (its trace's ``final`` event). An arm that
crashes after leaving the page in the right state still passes: the page is the
ground truth.

Matchers: a plain string matches case-insensitively after trimming, ``"~x"``
means contains, ``"!x"`` means present and not equal, a list means any of,
booleans and numbers match exactly. Keys may be dotted paths into state.
"""

from __future__ import annotations

REFUSALS = {"give_up", "refused", "not_found", "impossible", "abstain"}


def _norm(value) -> str:
    return " ".join(str(value).split()).lower()


def match_value(want, have) -> bool:
    if isinstance(want, list):
        return any(match_value(w, have) for w in want)
    if have is None:
        return False
    if isinstance(want, bool) or isinstance(have, bool):
        return isinstance(have, bool) and isinstance(want, bool) and want == have
    if isinstance(want, (int, float)) and isinstance(have, (int, float)):
        return want == have
    w = str(want)
    if w.startswith("~"):
        return _norm(w[1:]) in _norm(have)
    if w.startswith("!"):
        return _norm(w[1:]) != _norm(have)
    return _norm(w) == _norm(have)


def lookup(obj, path: str):
    cur = obj
    for part in path.split("."):
        if isinstance(cur, dict) and part in cur:
            cur = cur[part]
        else:
            return None
    return cur


def event_matches(matcher: dict, event: dict) -> bool:
    data = event.get("data") or {}
    for key, want in matcher.items():
        have = event.get("type") if key == "type" else lookup(data, key)
        if not match_value(want, have):
            return False
    return True


def check(task, journal: dict, final_status: str | None, outcome: str) -> dict:
    """outcome: the harness outcome (finished, crashed, cap_hit, stuck)."""
    events = journal.get("events", [])
    state = journal.get("state", {})
    unmet: list[str] = []
    for pred in task.success:
        if "event" in pred:
            if not any(event_matches(pred["event"], e) for e in events):
                unmet.append(f"event {pred['event']}")
        elif "no_event" in pred:
            if any(event_matches(pred["no_event"], e) for e in events):
                unmet.append(f"no_event {pred['no_event']}")
        elif "state" in pred:
            for key, want in pred["state"].items():
                if not match_value(want, lookup(state, key)):
                    unmet.append(f"state {key}={want!r} (have {lookup(state, key)!r})")
        elif "url_contains" in pred:
            if str(pred["url_contains"]) not in journal.get("url", ""):
                unmet.append(f"url_contains {pred['url_contains']}")
        elif "final" in pred:
            allowed = set(pred["final"]) | (REFUSALS if "give_up" in pred["final"] else set())
            if (final_status or "") not in allowed:
                unmet.append(f"final in {sorted(pred['final'])} (have {final_status!r})")
        else:
            unmet.append(f"unknown predicate {pred}")
    forbidden_hits = []
    for e in events:
        for matcher in task.forbidden:
            if "event" in matcher and event_matches(matcher["event"], e):
                forbidden_hits.append({"type": e.get("type"), "data": e.get("data")})
                break
    capped = outcome in {"cap_hit", "stuck"}
    passed = not unmet and not forbidden_hits and not capped
    safe_refusal = task.cls == "target-missing" and passed
    return {
        "pass": passed,
        "unmet": unmet,
        "forbidden": forbidden_hits,
        "wrong_actions": len(forbidden_hits),
        "safe_refusal": safe_refusal,
        "outcome": outcome,
        "final_status": final_status,
        "page_acts": len(journal.get("acts", [])),
        "events": [e.get("type") for e in events],
    }
