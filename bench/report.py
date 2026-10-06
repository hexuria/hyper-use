"""Write RESULTS.md (top level and per scenario) from one or more runs.

    bench/bench report [<run-id> ...]   # default: latest non-mock run
    bench/bench report 20261005-131001 20261005-141625

When multiple run ids are given, rows are merged by (arm, task, rep).
A row that actually ran wins over a `not_run` placeholder. If both ran,
the later run id on the command line wins. Each row keeps `_source_run_id`
so journal lookups (untrusted clicks) hit the right attempt directory.

n=1 per task per arm: this is a side-by-side comparison, not statistics.
Ranking: first-try accuracy primary; lower total first-try wall time tiebreak.
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

BRANCH_URL = "https://github.com/hexuria/ultra-instinct/tree/bench/uniform"


def pct(n: int, d: int) -> str:
    return f"{100 * n / d:.0f}% ({n}/{d})" if d else "–"


def untrusted_clicks(row: dict) -> int:
    run_id = row.get("_source_run_id")
    if not run_id:
        return 0
    total = 0
    for a in row.get("attempts", []):
        p = (
            EXAMPLES
            / row["scenario"]
            / "runs"
            / run_id
            / row["arm"]
            / row["task"]
            / f"rep-{row.get('rep', 0)}"
            / f"attempt-{a['attempt']}"
            / "journal.json"
        )
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


def arm_stats(rows: list[dict]) -> dict:
    ran = [r for r in rows if "not_run" not in r]
    tm = [r for r in ran if r["class"] == "target-missing"]
    press = [r for r in ran if not r["needs"]]
    walls_first = [r["wall_s_first"] for r in ran]
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
        "tokens_first": sum(r.get("tokens_first", 0) for r in ran),
        "wall": round(sum(r["wall_s"] for r in ran), 1),
        "wall_first": round(sum(r["wall_s_first"] for r in ran), 1),
        "wall_p50": round(statistics.median(walls_first), 1) if walls_first else 0,
        "untrusted": sum(untrusted_clicks(r) for r in ran),
        "not_run": sorted({r["not_run"] for r in rows if "not_run" in r}),
    }


def leaderboard(by_arm: dict[str, list[dict]]) -> list[str]:
    stats = {a: arm_stats(rows) for a, rows in by_arm.items()}
    ranked = sorted(
        [a for a in stats if stats[a]["ran"]],
        key=lambda a: (-stats[a]["first"] / stats[a]["ran"], stats[a]["wall_first"]),
    )
    out = [
        "| Rank | Arm | First-try accuracy | Pass within 3 | Press-only tasks, first try | Wrong actions (first try / all tries) | Safe refusals (target-missing) | cap_hit | stuck | crashed | Steps | Model calls | Tokens (first / all) | Wall first-try s | Wall total s | Wall p50 s (first try) | Untrusted clicks |",
        "|---:|---|---|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for i, a in enumerate(ranked, 1):
        s = stats[a]
        out.append(
            f"| {i} | **{a}** {ARMS[a]['name']} | {pct(s['first'], s['ran'])} | {pct(s['within'], s['ran'])} | "
            f"{pct(s['press_first'], s['press_n'])} | {s['wrong_first']} / {s['wrong']} | {s['tm_safe']}/{s['tm_n']} | "
            f"{s['cap_hit']} | {s['stuck']} | {s['crashed']} | {s['steps']} | {s['calls']} | "
            f"{s['tokens_first']:,} / {s['tokens']:,} | {s['wall_first']} | {s['wall']} | {s['wall_p50']} | {s['untrusted']} |"
        )
    for a, s in stats.items():
        if not s["ran"]:
            out.append(f"| – | **{a}** {ARMS[a]['name']} | not run | | | | | | | | | | | | | | |")
    notes = [
        f"- **{a}** not run: {'; '.join(s['not_run'])}"
        for a, s in stats.items()
        if s["not_run"] and not s["ran"]
    ]
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
    out = [
        "| Arm | Task | Tries | Outcomes (final status) | Wrong actions | Steps | Model calls | Tokens | Wall s | Unmet / forbidden (first try) |",
        "|---|---|---:|---|---:|---:|---:|---:|---:|---|",
    ]
    idx = {(r["arm"], r["task"]): r for r in rows}
    for a in arms:
        for t in tasks:
            r = idx.get((a, t.id))
            if not r or "not_run" in r:
                continue
            outs = ", ".join(
                f"{o['outcome']}{'/' + o['cap'] if o['cap'] else ''} ({o['final']})" for o in r["attempt_outcomes"]
            )
            first = r["attempts"][0]
            why = "; ".join(
                first["unmet"] + [f"forbidden {f['type']} {json.dumps(f['data'])[:80]}" for f in first["forbidden"]]
            )
            out.append(
                f"| {a} | `{t.id}` | {r['tries_used']} | {outs} | {r['wrong_actions']} | {r['steps']} | "
                f"{r['model_calls']} | {r['tokens']:,} | {r['wall_s']} | {why.replace('|', '/')[:220] or '–'} |"
            )
    return out


def manifest_md(mans: list[dict], merged_label: str) -> list[str]:
    lines = [
        "| Item | Value |",
        "|---|---|",
        f"| Report id | `{merged_label}` |",
    ]
    for man in mans:
        g = man["git"]
        rid = man["run_id"]
        lines += [
            f"| Run `{rid}` started | {man['started']} |",
            f"| Run `{rid}` bench HEAD | `{g['bench_uniform_head'][:12]}`{' (dirty)' if g['dirty'] else ''} |",
            f"| Run `{rid}` ultra-instinct merged | `{g['ultra_instinct_merged']}` |",
            f"| Run `{rid}` TYPESAFE_API_KEY present | {man['typesafe_key_present']} |",
        ]
    # shared settings from the first manifest (same config_hash across both)
    man = mans[0]
    g = man["git"]
    lines += [
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
    ]
    return lines


NOTE = (
    "**n = 1 run per arm per task** (up to 3 fresh tries, stopping at the first pass). This is a side-by-side "
    "comparison, not statistics: no confidence intervals or significance tests are claimed, and one flipped task "
    "moves an arm by several points."
)

SCORING = """Scoring. A task passes strictly when every success predicate holds in the page's own journal (events and
state the page recorded, never the agent's claims), no forbidden event fired, and the attempt did not end in
`cap_hit` or `stuck`. Target-missing tasks also need the arm to end with a refusal (give up / abstain / blocked by
choice). An arm that crashes after leaving the page in the right state still passes. Ranking is by first-try
accuracy; ties break on lower total first-try wall time. "Press-only" is the subset of tasks with no typing or select,
which is all ultra-instinct's `act` can do today (press/click). "Untrusted clicks" counts page clicks that were
synthetic DOM events (`isTrusted=false`), which can reach controls a person could not click (for example under a modal)."""


def load_run(run_dir: Path) -> tuple[dict, list[dict]]:
    man = json.loads((run_dir / "manifest.json").read_text())
    rows = [json.loads(l) for l in (run_dir / "results.jsonl").read_text().splitlines() if l.strip()]
    for r in rows:
        r["_source_run_id"] = run_dir.name
    return man, rows


def merge_rows(row_lists: list[list[dict]]) -> list[dict]:
    """Later lists override earlier ones for the same (arm, task, rep).
    A ran row always beats a not_run placeholder from any list.
    """
    by_key: dict[tuple, dict] = {}
    order: list[tuple] = []
    for rows in row_lists:
        for r in rows:
            key = (r["arm"], r["task"], r.get("rep", 0))
            prev = by_key.get(key)
            if prev is None:
                by_key[key] = r
                order.append(key)
                continue
            prev_ran = "not_run" not in prev
            cur_ran = "not_run" not in r
            if cur_ran and not prev_ran:
                by_key[key] = r
            elif cur_ran and prev_ran:
                by_key[key] = r  # later wins
            elif not cur_ran and not prev_ran:
                by_key[key] = r  # later not_run reason
            # else: keep prev ran
    return [by_key[k] for k in order]


def main() -> None:
    all_runs = sorted(
        p
        for p in (BENCH / "runs").iterdir()
        if p.is_dir() and (p / "results.jsonl").exists() and not p.name.endswith("-mock")
    )
    if len(sys.argv) > 1:
        run_dirs = [BENCH / "runs" / a for a in sys.argv[1:]]
    else:
        run_dirs = [all_runs[-1]] if all_runs else []
    if not run_dirs:
        raise SystemExit("no runs found")
    for d in run_dirs:
        if not (d / "results.jsonl").exists():
            raise SystemExit(f"missing results: {d}")

    mans: list[dict] = []
    row_lists: list[list[dict]] = []
    for d in run_dirs:
        man, rows = load_run(d)
        mans.append(man)
        row_lists.append(rows)
    rows = merge_rows(row_lists)
    merged_label = "+".join(d.name for d in run_dirs)

    arms = [a for a in ARMS if any(r["arm"] == a for r in rows) and not ARMS[a].get("mock")]
    all_tasks = load_all()
    scenarios = list(dict.fromkeys(t.scenario for t in all_tasks))
    by_arm = {a: [r for r in rows if r["arm"] == a] for a in arms}

    sources = ", ".join(f"`{d.name}`" for d in run_dirs)
    top = [
        "# ultra-instinct uniform benchmark: results",
        "",
        f"Branch: {BRANCH_URL}. Merged runs: {sources}. Harness: `bench/` (see `bench/README.md`).",
        "",
        NOTE,
        "",
        "## Leaderboard (all scenarios)",
        "",
        *leaderboard(by_arm),
        "",
        SCORING,
        "",
        "## Per scenario",
        "",
    ]
    for sc in scenarios:
        tasks = [t for t in all_tasks if t.scenario == sc]
        srows = [r for r in rows if r["scenario"] == sc]
        if not srows:
            continue
        sby = {a: [r for r in srows if r["arm"] == a] for a in arms}
        md = [
            f"# {sc}: results",
            "",
            f"Merged runs: {sources}. Tasks: `examples/{sc}/tasks.yaml`. Site: `examples/{sc}/site/`.",
            "",
            NOTE,
            "",
            "## Leaderboard",
            "",
            *leaderboard(sby),
            "",
            "## Per task (attempts in order)",
            "",
            *per_task(srows, arms, tasks),
            "",
            "## Detail",
            "",
            *detail(srows, arms, tasks),
            "",
            "## Manifest",
            "",
            *manifest_md(mans, merged_label),
            "",
        ]
        (EXAMPLES / sc / "RESULTS.md").write_text("\n".join(md))
        (EXAMPLES / sc / "runs").mkdir(parents=True, exist_ok=True)
        (EXAMPLES / sc / "runs" / "summary.json").write_text(
            json.dumps(
                {
                    "merged_runs": [d.name for d in run_dirs],
                    "rows": [{k: v for k, v in r.items() if k != "attempts"} for r in srows],
                },
                indent=1,
            )
        )
        s = {a: arm_stats(sby[a]) for a in arms}
        top.append(
            f"- [{sc}](examples/{sc}/RESULTS.md): "
            + ", ".join(f"{a} {s[a]['first']}/{s[a]['ran']}" if s[a]["ran"] else f"{a} not run" for a in arms)
        )
    top += [
        "",
        "## Per task, all scenarios",
        "",
        *per_task(rows, arms, all_tasks),
        "",
        "## Manifest",
        "",
        *manifest_md(mans, merged_label),
        "",
        "## Honesty notes",
        "",
        "- Luna pass `20261005-131001` ran A1/A2/A6; A3/A4/A5 were skipped there (`TYPESAFE_API_KEY not available`).",
        "- JEV pass `20261005-141625` ran A3/A4/A5 with the key present. This report merges both.",
        "- ultra-instinct arms (A5, A6) can only press/click today. Tasks that need type/select show under Needs and in the press-only column; unmet body/form fields are expected failures for those arms until typing lands.",
        "- A3 (jev-ultrafast) recorded many `crashed` outcomes in this pass; treat those as harness/arm failures, not page successes.",
        "- Combined arms A7 (Luna + JEV + ultra-instinct + CUA) and A8 (Luna + JEV + ultra-instinct + Browser Use) ran in their own pass with the key present; protocol and limits in `bench/arms/COMBO.md`. A1 to A6 were not re-run.",
        "- A7 cannot set a `<select>`: cua-driver 0.23.2 has no select tool and refuses its trusted input route on the background window (A2 hit the same wall). A7's ~15 s browser launch and sizing counts toward its wall time, as for A2 and A4.",
        "- Untrusted clicks in A7/A8 come from ultra-instinct's `dom-semantic` press (same as A5/A6) and A7's CUA `dom_event` fallback; Browser Use clicks in A8 go through CDP mouse input.",
        "",
    ]
    (ROOT / "RESULTS.md").write_text("\n".join(top))
    out = BENCH / "results"
    out.mkdir(exist_ok=True)
    merged_path = out / f"{merged_label}.json"
    merged_path.write_text(
        json.dumps({"merged_runs": [d.name for d in run_dirs], "manifests": mans, "rows": rows}, indent=1)
    )
    # also write a convenience merged run dir for future single-arg report
    merged_dir = BENCH / "runs" / merged_label
    merged_dir.mkdir(parents=True, exist_ok=True)
    (merged_dir / "manifest.json").write_text(
        json.dumps(
            {
                **mans[-1],
                "run_id": merged_label,
                "merged_from": [d.name for d in run_dirs],
                "typesafe_key_present": any(m.get("typesafe_key_present") for m in mans),
            },
            indent=2,
        )
    )
    # strip internal key for on-disk jsonl? keep it for re-report
    with (merged_dir / "results.jsonl").open("w") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")
    print(f"wrote RESULTS.md, examples/*/RESULTS.md, {merged_path.relative_to(ROOT)}, {merged_dir.relative_to(ROOT)}/")


if __name__ == "__main__":
    main()
