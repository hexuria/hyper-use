"""Write RESULTS.md (top level and per scenario) from one run.

    bench/bench report [<run-id>]      # default: latest non-mock run

n=1 per task per arm: this is a side-by-side comparison, not statistics.
"""

from __future__ import annotations

import json
import statistics
import sys
from pathlib import Path

BENCH = Path(__file__).resolve().parent
ROOT = BENCH.parent
sys.path.insert(0, str(BENCH))
from run import ARMS  # noqa: E402
from tasks import EXAMPLES, load_all  # noqa: E402

BRANCH_URL = "https://github.com/hexuria/hyper-use/tree/bench/uniform"


def pct(n: int, d: int) -> str:
    return f"{100 * n / d:.0f}% ({n}/{d})" if d else "–"


def untrusted_clicks(run_id: str, row: dict) -> int:
    total = 0
    for a in row.get("attempts", []):
        p = EXAMPLES / row["scenario"] / "runs" / run_id / row["arm"] / row["task"] / f"rep-{row.get('rep', 0)}" / f"attempt-{a['attempt']}" / "journal.json"
        if p.exists():
            acts = json.loads(p.read_text()).get("acts", [])
            total += sum(1 for x in acts if x.get("kind") == "click" and x.get("trusted") is False)
    return total


def cell(row: dict) -> str:
    if "not_run" in row:
        return "not run"
    outs = row["attempt_outcomes"]
    marks = []
    for o in outs:
        if o["pass"]:
            marks.append("PASS")
        else:
            why = o["cap"] or (o["outcome"] if o["outcome"] != "finished" else "fail")
            marks.append(f"fail({why})" if why != "fail" else "fail")
    return " → ".join(marks)


def arm_stats(rows: list[dict], run_id: str) -> dict:
    ran = [r for r in rows if "not_run" not in r]
    tm = [r for r in ran if r["class"] == "target-missing"]
    press = [r for r in ran if not r["needs"]]
    walls = [r["wall_s_first"] for r in ran]
    return {
        "ran": len(ran),
        "first": sum(r["pass_first"] for r in ran),
        "within": sum(r["pass_within"] for r in ran),
        "press_n": len(press),
        "press_first": sum(r["pass_first"] for r in press),
        "wrong": sum(r["wrong_actions"] for r in ran),
        "wrong_first": sum(r["wrong_actions_first"] for r in ran),
        "tm_n": len(tm),
        "tm_safe": sum(r["safe_refusal_first"] for r in tm),
        "cap_hit": sum(r["cap_hits"] for r in ran),
        "stuck": sum(r["stuck"] for r in ran),
        "crashed": sum(r["crashed"] for r in ran),
        "steps": sum(r["steps"] for r in ran),
        "calls": sum(r["model_calls"] for r in ran),
        "tokens": sum(r["tokens"] for r in ran),
        "wall": round(sum(r["wall_s"] for r in ran), 1),
        "wall_p50": round(statistics.median(walls), 1) if walls else 0,
        "untrusted": sum(untrusted_clicks(run_id, r) for r in ran),
        "not_run": sorted({r["not_run"] for r in rows if "not_run" in r}),
    }


def leaderboard(by_arm: dict[str, list[dict]], run_id: str) -> list[str]:
    stats = {a: arm_stats(rows, run_id) for a, rows in by_arm.items()}
    ranked = sorted([a for a in stats if stats[a]["ran"]], key=lambda a: (-stats[a]["first"] / stats[a]["ran"], stats[a]["wall"]))
    out = ["| Rank | Arm | First-try accuracy | Pass within 3 | Press-only tasks, first try | Wrong actions (first try / all tries) | Safe refusals (target-missing) | cap_hit | stuck | crashed | Steps | Model calls | Tokens | Wall total s | Wall p50 s (first try) | Untrusted clicks |",
           "|---:|---|---|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
    for i, a in enumerate(ranked, 1):
        s = stats[a]
        out.append(f"| {i} | **{a}** {ARMS[a]['name']} | {pct(s['first'], s['ran'])} | {pct(s['within'], s['ran'])} | {pct(s['press_first'], s['press_n'])} | "
                   f"{s['wrong_first']} / {s['wrong']} | {s['tm_safe']}/{s['tm_n']} | {s['cap_hit']} | {s['stuck']} | {s['crashed']} | {s['steps']} | {s['calls']} | "
                   f"{s['tokens']:,} | {s['wall']} | {s['wall_p50']} | {s['untrusted']} |")
    for a, s in stats.items():
        if not s["ran"]:
            out.append(f"| – | **{a}** {ARMS[a]['name']} | not run | | | | | | | | | | | | | |")
    notes = [f"- **{a}** not run: {'; '.join(s['not_run'])}" for a, s in stats.items() if s["not_run"] and not s["ran"]]
    return out + ([""] + notes if notes else [])


def per_task(rows: list[dict], arms: list[str], tasks) -> list[str]:
    head = "| Task | Class | Needs | " + " | ".join(arms) + " |"
    out = [head, "|---|---|---|" + "---|" * len(arms)]
    idx = {(r["arm"], r["task"]): r for r in rows}
    for t in tasks:
        cells = [cell(idx[(a, t.id)]) if (a, t.id) in idx else "–" for a in arms]
        out.append(f"| `{t.id}` | {t.cls} | {', '.join(t.needs) or 'press'} | " + " | ".join(cells) + " |")
    return out


def detail(rows: list[dict], arms: list[str], tasks) -> list[str]:
    out = ["| Arm | Task | Tries | Outcomes (final status) | Wrong actions | Steps | Model calls | Tokens | Wall s | Unmet / forbidden (first try) |",
           "|---|---|---:|---|---:|---:|---:|---:|---:|---|"]
    idx = {(r["arm"], r["task"]): r for r in rows}
    for a in arms:
        for t in tasks:
            r = idx.get((a, t.id))
            if not r or "not_run" in r:
                continue
            outs = ", ".join(f"{o['outcome']}{'/' + o['cap'] if o['cap'] else ''} ({o['final']})" for o in r["attempt_outcomes"])
            first = r["attempts"][0]
            why = "; ".join(first["unmet"] + [f"forbidden {f['type']} {json.dumps(f['data'])[:80]}" for f in first["forbidden"]])
            out.append(f"| {a} | `{t.id}` | {r['tries_used']} | {outs} | {r['wrong_actions']} | {r['steps']} | {r['model_calls']} | {r['tokens']:,} | {r['wall_s']} | {why.replace('|', '/')[:220] or '–'} |")
    return out


def manifest_md(man: dict) -> list[str]:
    g = man["git"]
    return [
        "| Item | Value |", "|---|---|",
        f"| Run id | `{man['run_id']}` (started {man['started']}) |",
        f"| bench/uniform HEAD at run | `{g['bench_uniform_head'][:12]}`{' (dirty)' if g['dirty'] else ''} |",
        f"| hyper-use merged | `{g['hyper_use_merged']}` (gol/serene-cray-7dwros, `{g['hyper_use_merged_full'][:12]}`) |",
        f"| jev-ultrafast pin | `{g['jev_ultrafast'][:12]}` |",
        f"| cua (jev-use) pin | `{g['cua'][:12]}` |",
        f"| Chrome | {man['chrome']} (headless for harness-launched arms; CUA arms use a driver-launched headed window sized to the same viewport) |",
        f"| cua-driver | {man['cua_driver']} |",
        f"| browser-use | {man['browser_use']} |",
        f"| Models | Luna `{man['models']['luna']}` (reasoning effort {man['models']['luna_effort']}) via OpenCodex; JEV `{man['models']['jev']}` |",
        f"| Caps per attempt | {man['caps']['steps']} steps, {man['caps']['model_calls']} model calls, {man['caps']['wall_s']} s wall, stuck after {man['caps']['stuck_repeats']} identical no-change actions, {man['caps']['page_acts']} page acts backstop |",
        f"| Viewport | {man['viewport']['width']}×{man['viewport']['height']} |",
        f"| Seed / reps / max tries | {man['seed']} / {man['reps']} / {man['max_tries']} |",
        f"| Config hash | `{man['config_hash']}` |",
        f"| TYPESAFE_API_KEY present | {man['typesafe_key_present']} |",
    ]


NOTE = ("**n = 1 run per arm per task** (up to 3 fresh tries, stopping at the first pass). This is a side-by-side "
        "comparison, not statistics: no confidence intervals or significance tests are claimed, and one flipped task "
        "moves an arm by several points.")

SCORING = """Scoring. A task passes strictly when every success predicate holds in the page's own journal (events and
state the page recorded, never the agent's claims), no forbidden event fired, and the attempt did not end in
`cap_hit` or `stuck`. Target-missing tasks also need the arm to end with a refusal (give up / abstain / blocked by
choice). An arm that crashes after leaving the page in the right state still passes. Ranking is by first-try
accuracy; ties break on lower total wall time. "Press-only" is the subset of tasks with no typing or select,
which is all hyper-use's `act` can do today (press/click). "Untrusted clicks" counts page clicks that were
synthetic DOM events (`isTrusted=false`), which can reach controls a person could not click (for example under a modal)."""


def main() -> None:
    runs = sorted(p for p in (BENCH / "runs").iterdir() if p.is_dir() and (p / "results.jsonl").exists() and not p.name.endswith("-mock"))
    run_dir = BENCH / "runs" / sys.argv[1] if len(sys.argv) > 1 else runs[-1]
    run_id = run_dir.name
    man = json.loads((run_dir / "manifest.json").read_text())
    rows = [json.loads(l) for l in (run_dir / "results.jsonl").read_text().splitlines() if l.strip()]
    arms = [a for a in ARMS if any(r["arm"] == a for r in rows)]
    all_tasks = load_all()
    scenarios = list(dict.fromkeys(t.scenario for t in all_tasks))
    by_arm = {a: [r for r in rows if r["arm"] == a] for a in arms}

    top = ["# hyper-use uniform benchmark: results", "",
           f"Branch: {BRANCH_URL}. Run `{run_id}`. Harness: `bench/` (see `bench/README.md`).", "", NOTE, "",
           "## Leaderboard (all scenarios)", "", *leaderboard(by_arm, run_id), "", SCORING, "",
           "## Per scenario", ""]
    for sc in scenarios:
        tasks = [t for t in all_tasks if t.scenario == sc]
        srows = [r for r in rows if r["scenario"] == sc]
        if not srows:
            continue
        sby = {a: [r for r in srows if r["arm"] == a] for a in arms}
        title = sc
        md = [f"# {sc}: results", "", f"Run `{run_id}`. Tasks: `examples/{sc}/tasks.yaml`. Site: `examples/{sc}/site/`.", "", NOTE, "",
              "## Leaderboard", "", *leaderboard(sby, run_id), "", "## Per task (attempts in order)", "", *per_task(srows, arms, tasks), "",
              "## Detail", "", *detail(srows, arms, tasks), "", "## Manifest", "", *manifest_md(man), ""]
        (EXAMPLES / sc / "RESULTS.md").write_text("\n".join(md))
        (EXAMPLES / sc / "runs").mkdir(parents=True, exist_ok=True)
        (EXAMPLES / sc / "runs" / "summary.json").write_text(json.dumps(
            {"run_id": run_id, "rows": [{k: v for k, v in r.items() if k != "attempts"} for r in srows]}, indent=1))
        s = {a: arm_stats(sby[a], run_id) for a in arms}
        top.append(f"- [{title}](examples/{sc}/RESULTS.md): " + ", ".join(
            f"{a} {s[a]['first']}/{s[a]['ran']}" if s[a]["ran"] else f"{a} not run" for a in arms))
    top += ["", "## Per task, all scenarios", "", *per_task(rows, arms, all_tasks), "", "## Manifest", "", *manifest_md(man), ""]
    (ROOT / "RESULTS.md").write_text("\n".join(top))
    out = BENCH / "results"
    out.mkdir(exist_ok=True)
    (out / f"{run_id}.json").write_text(json.dumps({"manifest": man, "rows": rows}, indent=1))
    print(f"wrote RESULTS.md, examples/*/RESULTS.md, bench/results/{run_id}.json")


if __name__ == "__main__":
    main()
