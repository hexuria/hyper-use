# hyper-use uniform benchmark: results

Branch: https://github.com/hexuria/hyper-use/tree/bench/uniform. Merged runs: `20261005-131001`, `20261005-141625`. Harness: `bench/` (see `bench/README.md`).

**n = 1 run per arm per task** (up to 3 fresh tries, stopping at the first pass). This is a side-by-side comparison, not statistics: no confidence intervals or significance tests are claimed, and one flipped task moves an arm by several points.

## Leaderboard (all scenarios)

| Rank | Arm | First-try accuracy | Pass within 3 | Press-only tasks, first try | Wrong actions (first try / all tries) | Safe refusals (target-missing) | cap_hit | stuck | crashed | Steps | Model calls | Tokens (first / all) | Wall first-try s | Wall total s | Wall p50 s (first try) | Untrusted clicks |
|---:|---|---|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | **A1** Luna + Browser Use | 91% (21/23) | 100% (23/23) | 94% (17/18) | 0 / 0 | 4/4 | 0 | 0 | 0 | 59 | 100 | 804,749 / 899,877 | 770.0 | 861.5 | 35.8 | 5 |
| 2 | **A2** Luna + CUA driver | 83% (19/23) | 91% (21/23) | 94% (17/18) | 0 / 0 | 4/4 | 0 | 0 | 0 | 97 | 203 | 521,358 / 762,828 | 1177.3 | 1651.3 | 48.3 | 36 |
| 3 | **A4** CUA jev-use (generic task) | 70% (16/23) | 70% (16/23) | 72% (13/18) | 4 / 13 | 4/4 | 0 | 8 | 0 | 103 | 136 | 202,208 / 337,116 | 686.7 | 1148.3 | 24.6 | 48 |
| 4 | **A6** Luna + hyper-use (MCP) | 43% (10/23) | 61% (14/23) | 56% (10/18) | 0 / 0 | 4/4 | 0 | 0 | 0 | 50 | 290 | 693,085 / 1,838,115 | 505.1 | 1159.8 | 17.0 | 29 |
| 5 | **A3** jev-ultrafast (BU + JEV) | 39% (9/23) | 48% (11/23) | 44% (8/18) | 1 / 1 | 2/4 | 4 | 0 | 21 | 87 | 228 | 454,033 / 1,004,831 | 400.4 | 872.1 | 17.8 | 0 |
| 6 | **A5** JEV + hyper-use (live_drive) | 30% (7/23) | 30% (7/23) | 39% (7/18) | 1 / 2 | 3/4 | 21 | 3 | 0 | 45 | 1403 | 2,961,164 / 8,676,362 | 209.0 | 580.8 | 8.6 | 30 |

Scoring. A task passes strictly when every success predicate holds in the page's own journal (events and
state the page recorded, never the agent's claims), no forbidden event fired, and the attempt did not end in
`cap_hit` or `stuck`. Target-missing tasks also need the arm to end with a refusal (give up / abstain / blocked by
choice). An arm that crashes after leaving the page in the right state still passes. Ranking is by first-try
accuracy; ties break on lower total first-try wall time. "Press-only" is the subset of tasks with no typing or select,
which is all hyper-use's `act` can do today (press/click). "Untrusted clicks" counts page clicks that were
synthetic DOM events (`isTrusted=false`), which can reach controls a person could not click (for example under a modal).

## Per scenario

- [acme-mail](examples/acme-mail/RESULTS.md): A1 7/7, A2 7/7, A3 3/7, A4 6/7, A5 3/7, A6 4/7
- [shop-checkout](examples/shop-checkout/RESULTS.md): A1 4/4, A2 3/4, A3 3/4, A4 3/4, A5 2/4, A6 2/4
- [admin-table](examples/admin-table/RESULTS.md): A1 3/3, A2 3/3, A3 0/3, A4 1/3, A5 1/3, A6 1/3
- [booking-calendar](examples/booking-calendar/RESULTS.md): A1 3/3, A2 1/3, A3 3/3, A4 2/3, A5 0/3, A6 0/3
- [hard-dom](examples/hard-dom/RESULTS.md): A1 3/4, A2 4/4, A3 0/4, A4 3/4, A5 1/4, A6 2/4
- [travel-search](examples/travel-search/RESULTS.md): A1 1/2, A2 1/2, A3 0/2, A4 1/2, A5 0/2, A6 1/2

## Per task, all scenarios

| Task | Class | Needs | A1 | A2 | A3 | A4 | A5 | A6 |
|---|---|---|---|---|---|---|---|---|
| `am-compose-send` | normal | type | PASS | PASS | fail(crashed) → fail(crashed) → fail(crashed) | PASS | fail → fail → fail | fail → fail → fail |
| `am-reply` | normal | type | PASS | PASS | fail(crashed) → fail(crashed) → fail(crashed) | PASS | fail → fail → fail | fail → fail → fail |
| `am-twin-send` | normal | press | PASS | PASS | fail(steps) → fail(steps) → fail(steps) | PASS | fail → fail(model_calls) → fail | PASS |
| `am-enabled-save` | normal | press | PASS | PASS | PASS | PASS | PASS | PASS |
| `am-archive` | normal | press | PASS | PASS | PASS | PASS | PASS | PASS |
| `am-star` | normal | press | PASS | PASS | fail → PASS | fail → fail → fail | fail → fail → fail | fail → fail → PASS |
| `am-tm-print` | target-missing | press | PASS | PASS | PASS | PASS | PASS | PASS |
| `shop-add-qty` | normal | press | PASS | PASS | PASS | fail → fail → fail | fail(model_calls) → fail(model_calls) → fail(model_calls) | fail → fail → PASS |
| `shop-checkout` | normal | type | PASS | fail → PASS | fail(crashed) → fail(crashed) → fail(crashed) | PASS | fail → fail → fail | fail → fail → fail |
| `shop-noop-shipping` | no-op | press | PASS | PASS | PASS | PASS | PASS | PASS |
| `shop-tm-coupon` | target-missing | press | PASS | PASS | PASS | PASS | PASS | PASS |
| `admin-suspend` | normal | press | PASS | PASS | fail(crashed) → fail(crashed) → fail(crashed) | fail → fail → fail | fail → fail → fail(model_calls) | fail → fail → fail |
| `admin-filter-page` | normal | press | PASS | PASS | fail → fail → fail | fail(stuck) → fail(stuck) → fail | fail(model_calls) → fail(model_calls) → fail(model_calls) | fail → fail → PASS |
| `admin-tm-delete` | target-missing | press | PASS | PASS | fail(crashed) → PASS | PASS | PASS | PASS |
| `book-slot` | normal | press | PASS | fail → PASS | PASS | PASS | fail(model_calls) → fail(model_calls) → fail(model_calls) | fail → fail → fail |
| `book-timezone` | normal | select | PASS | fail → fail → fail | PASS | fail(stuck) → fail(stuck) → fail(stuck) | fail → fail → fail | fail → fail → fail |
| `book-noop` | no-op | press | PASS | PASS | PASS | PASS | fail → fail(model_calls) → fail | fail → fail → fail |
| `hd-star` | normal | press | PASS | PASS | fail(model_calls) → fail(crashed) → fail(crashed) | fail → fail → fail | fail(model_calls) → fail(model_calls) → fail(model_calls) | fail → fail → PASS |
| `hd-shadow` | normal | press | PASS | PASS | fail → fail → fail | PASS | fail(stuck) → fail(stuck) → fail(stuck) | PASS |
| `hd-iframe` | normal | press | fail → PASS | PASS | fail → fail → fail | PASS | fail → fail → fail | fail → fail → fail |
| `hd-overlay` | normal | press | PASS | PASS | fail → fail → fail | PASS | PASS | PASS |
| `travel-lisbon` | normal | type, select | fail → fail → PASS | fail → fail → fail | fail(crashed) → fail(crashed) → fail(crashed) | fail(stuck) → fail(stuck) → fail(stuck) | fail(model_calls) → fail(model_calls) → fail(model_calls) | fail → fail → fail |
| `travel-tm` | target-missing | press | PASS | PASS | fail(crashed) → fail(crashed) → fail(crashed) | PASS | fail(model_calls) → fail(model_calls) → fail(model_calls) | PASS |

## Manifest

| Item | Value |
|---|---|
| Report id | `20261005-131001+20261005-141625` |
| Run `20261005-131001` started | 2026-10-05 13:10:04 +0800 |
| Run `20261005-131001` bench HEAD | `456b9c062a06` |
| Run `20261005-131001` hyper-use merged | `4ae30d3` |
| Run `20261005-131001` TYPESAFE_API_KEY present | False |
| Run `20261005-141625` started | 2026-10-05 14:16:28 +0800 |
| Run `20261005-141625` bench HEAD | `984ac2c6a515` |
| Run `20261005-141625` hyper-use merged | `4ae30d3` |
| Run `20261005-141625` TYPESAFE_API_KEY present | True |
| jev-ultrafast pin | `1231850a0bf1` |
| cua (jev-use) pin | `9ccafc981412` |
| Chrome | Chrome/154.0.8037.98 (headless for harness-launched arms; CUA arms use a driver-launched headed window sized to the same viewport) |
| cua-driver | cua-driver 0.23.2 |
| browser-use | 0.13.10 |
| Models | Luna `gpt-6-luna` (reasoning effort low) via OpenCodex; JEV `jev-latest` |
| Caps per attempt | 20 steps, 40 model calls, 180 s wall, stuck after 3 identical no-change actions, 40 page acts backstop |
| Viewport | 1280×800 |
| Seed / reps / max tries | 42 / 1 / 3 |
| Config hash | `c7242d275cf8` |

## Honesty notes

- Luna pass `20261005-131001` ran A1/A2/A6; A3/A4/A5 were skipped there (`TYPESAFE_API_KEY not available`).
- JEV pass `20261005-141625` ran A3/A4/A5 with the key present. This report merges both.
- hyper-use arms (A5, A6) can only press/click today. Tasks that need type/select show under Needs and in the press-only column; unmet body/form fields are expected failures for those arms until typing lands.
- A3 (jev-ultrafast) recorded many `crashed` outcomes in this pass; treat those as harness/arm failures, not page successes.
