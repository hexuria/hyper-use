# Head-to-head race: jev-ultrafast vs ultra-instinct `aui run` (2026-10-06)

Run `20261006-075851`, seed 42, config [`bench/race.toml`](../race.toml).

- **Tasks:** 23 tasks across the 6 local scenarios.
- **Runs:** 3 reps per arm per task, `max_tries = 1`, so every attempt counts. That makes 207 attempts.
- **Browser:** headless Chrome 137 (Linux), a fresh profile per attempt, 1280×800.
- **Caps:** 20 steps, 40 model calls, 180 s.

Raw data: [`20261006-075851.json`](20261006-075851.json). `bench/bench report 20261006-075851` regenerates the leaderboard.

| Arm | What | Pin |
|---|---|---|
| A3q | jev-ultrafast `Agent` on Browser Harness. TypeSafe JEV decides every step. Text is the quoted goal literal (no text model). | jev-ultrafast `1231850` |
| A9 | `aui run --policy instinct`. Instinct decides locally; no model calls. | ultra-instinct `8d819ff` (PR #37 head, includes #35) |
| A10 | `aui run --policy jev`. JEV decides every step through our loop (gate, ticket, verify). | same binary, `--features jev` |

Text is quoted goal literals on every arm, so the race compares decisions and loops, not text models.

## Results

| Arm | Pass (all) | Press-only | Type / select | False DONE | Wrong actions | Crashed | Cap hits | Model calls / attempt | In-loop s, median | Process wall s, median |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| A3q | **48/69** | **33/54** | **15/15** | 0 | 0 | 1 | 5 | 5.25 | 0.81 | 15.8 |
| A10 | 21/69 | 21/54 | 0/15 | 8 | 6 | 15 | 0 | 4.83 | 0.64 | 0.8 |
| A9 | 12/69 | 12/54 | 0/15 | 30 | 0 | 0 | 0 | 0 | 0.03 | 0.25 |

How the timing columns are measured:
- **In-loop** time is the `t` of the arm's final trace row.
- **Process wall** is the harness's per-attempt timer. A3q's process wall includes about 15.2 s (median) outside its agent loop: the Python imports and the Browser Harness daemon startup.

## What this shows

1. **jev-ultrafast completes far more tasks.**
   - A3q passes 48/69, against 21/69 for A10 and 12/69 for A9.
   - A3q passes every typing/select task (15/15); both of our arms pass none (0/15).
   - None of A9's or A10's passes is on a task A3q failed, except `hd-iframe` (A10 3/3, A3q 0/3).
2. **A9 (Instinct only) is not competitive on natural-language goals.**
   - All 12 of its passes are zero-step trap/no-op tasks.
   - 30 of its 33 `done` outcomes are false: it declared DONE without acting. The cause is `done_language` in `aui-policy/src/evidence.rs`, which does a whole-goal substring match for "done". The shared goal suffix every arm receives ("If the task cannot be done on this site, stop…") trips it on every task.
   - A diagnostic run without the suffix (`20261006-082640`, A9b, n=1, [jsonl](20261006-082640-A9b-diagnostic.jsonl)) passes 7/23. Most tasks still abstain at step 0, so fixing the suffix match alone does not close the gap.
3. **A10 (JEV in our loop) is not at parity with JEV in its own loop.**
   - 5 crashes: `CDP transport: no CDP result for Runtime.evaluate after 64 messages`, on every acme-mail `am-star` rep and on two `am-reply` reps. Our transport gives up when CDP events flood in.
   - 7 crashes: `max steps exceeded` on `shop-add-qty`, `admin-tm-delete` and `admin-suspend`.
   - Also: 6 wrong actions, 8 false DONEs, and 80 untrusted (script-dispatched) clicks.
4. **Speed: no superiority claim.**
   - Inside the loop, the medians are A3q 0.81 s and A10 0.64 s.
   - On the 18 task-reps both passed, the medians are 0.54 s and 0.40 s. Those are mostly one-call trap tasks, and n is small.
   - A3q's ~15 s process overhead is real for one-shot CLI use, but it is startup cost, not per-decision cost.

## Not covered by this run

- **Live sites:** live Google Flights / Wikipedia were not run. travel-search is the local hotel/search stand-in.
- **Text model:** no text model was used on any arm.
- **Parallel agents:** no parallel multi-agent race. The tab-per-agent change (#34) is not in the measured binary.
- **CDP call counts:** not metered for A3q. Browser Harness owns its socket.
- **Statistics:** 3 reps is a side-by-side comparison, not statistics.
