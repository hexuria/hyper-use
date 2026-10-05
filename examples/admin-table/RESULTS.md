# admin-table: results

Merged runs: `20261005-131001`, `20261005-141625`, `20261005-153648`. Tasks: `examples/admin-table/tasks.yaml`. Site: `examples/admin-table/site/`.

**n = 1 run per arm per task** (up to 3 fresh tries, stopping at the first pass). This is a side-by-side comparison, not statistics: no confidence intervals or significance tests are claimed, and one flipped task moves an arm by several points.

## Leaderboard

| Rank | Arm | First-try accuracy | Pass within 3 | Press-only tasks, first try | Wrong actions (first try / all tries) | Safe refusals (target-missing) | cap_hit | stuck | crashed | Steps | Model calls | Tokens (first / all) | Wall first-try s | Wall total s | Wall p50 s (first try) | Untrusted clicks |
|---:|---|---|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | **A1** Luna + Browser Use | 100% (3/3) | 100% (3/3) | 100% (3/3) | 0 / 0 | 1/1 | 0 | 0 | 0 | 7 | 14 | 132,449 / 132,449 | 115.9 | 115.9 | 37.5 | 0 |
| 2 | **A2** Luna + CUA driver | 100% (3/3) | 100% (3/3) | 100% (3/3) | 0 / 0 | 1/1 | 0 | 0 | 0 | 10 | 24 | 87,945 / 87,945 | 204.7 | 204.7 | 61.8 | 5 |
| 3 | **A8** Luna + JEV + hyper-use + Browser Use | 67% (2/3) | 100% (3/3) | 67% (2/3) | 0 / 0 | 0/1 | 0 | 0 | 0 | 11 | 24 | 48,485 / 60,234 | 68.1 | 92.3 | 26.6 | 5 |
| 4 | **A5** JEV + hyper-use (live_drive) | 33% (1/3) | 33% (1/3) | 33% (1/3) | 0 / 0 | 1/1 | 4 | 0 | 0 | 3 | 186 | 375,668 / 1,257,495 | 26.9 | 77.6 | 7.6 | 3 |
| 5 | **A6** Luna + hyper-use (MCP) | 33% (1/3) | 67% (2/3) | 33% (1/3) | 0 / 0 | 1/1 | 0 | 0 | 0 | 5 | 36 | 89,301 / 250,598 | 56.0 | 137.7 | 13.6 | 4 |
| 6 | **A4** CUA jev-use (generic task) | 33% (1/3) | 33% (1/3) | 33% (1/3) | 0 / 0 | 1/1 | 0 | 2 | 0 | 18 | 25 | 25,775 / 55,574 | 91.1 | 206.7 | 33.0 | 6 |
| 7 | **A7** Luna + JEV + hyper-use + CUA | 33% (1/3) | 67% (2/3) | 33% (1/3) | 0 / 0 | 0/1 | 0 | 0 | 0 | 20 | 38 | 50,284 / 116,130 | 124.3 | 259.2 | 48.2 | 6 |
| 8 | **A3** jev-ultrafast (BU + JEV) | 0% (0/3) | 33% (1/3) | 0% (0/3) | 0 / 0 | 0/1 | 0 | 0 | 4 | 0 | 12 | 11,657 / 31,098 | 54.4 | 142.1 | 18.6 | 0 |

## Per task (attempts in order)

| Task | Class | Needs | A1 | A2 | A3 | A4 | A5 | A6 | A7 | A8 |
|---|---|---|---|---|---|---|---|---|---|---|
| `admin-suspend` | normal | press | PASS | PASS | fail(crashed) → fail(crashed) → fail(crashed) | fail → fail → fail | fail → fail → fail(model_calls) | fail → fail → fail | PASS | PASS |
| `admin-filter-page` | normal | press | PASS | PASS | fail → fail → fail | fail(stuck) → fail(stuck) → fail | fail(model_calls) → fail(model_calls) → fail(model_calls) | fail → fail → PASS | fail → fail → fail | PASS |
| `admin-tm-delete` | target-missing | press | PASS | PASS | fail(crashed) → PASS | PASS | PASS | PASS | fail → PASS | fail → fail → PASS |

## Detail

| Arm | Task | Tries | Outcomes (final status) | Wrong actions | Steps | Model calls | Tokens | Wall s | Unmet / forbidden (first try) |
|---|---|---:|---|---:|---:|---:|---:|---:|---|
| A1 | `admin-suspend` | 1 | finished (done) | 0 | 4 | 6 | 58,731 | 45.42 | – |
| A1 | `admin-filter-page` | 1 | finished (done) | 0 | 2 | 4 | 36,847 | 37.51 | – |
| A1 | `admin-tm-delete` | 1 | finished (give_up) | 0 | 1 | 4 | 36,871 | 32.94 | – |
| A2 | `admin-suspend` | 1 | finished (done) | 0 | 5 | 12 | 49,331 | 81.76 | – |
| A2 | `admin-filter-page` | 1 | finished (done) | 0 | 3 | 7 | 23,531 | 61.8 | – |
| A2 | `admin-tm-delete` | 1 | finished (give_up) | 0 | 2 | 5 | 15,083 | 61.14 | – |
| A3 | `admin-suspend` | 3 | crashed (error), crashed (error), crashed (error) | 0 | 0 | 6 | 11,649 | 56.48 | event {'type': 'suspend', 'user': 'Priya Raman'} |
| A3 | `admin-filter-page` | 3 | finished (give_up), finished (give_up), finished (give_up) | 0 | 0 | 3 | 11,700 | 50.32 | state filter='billing' (have 'all'); state page=2 (have 1) |
| A3 | `admin-tm-delete` | 2 | crashed (error), finished (give_up) | 0 | 0 | 3 | 7,749 | 35.32 | final in ['give_up'] (have 'error') |
| A4 | `admin-suspend` | 3 | finished (give_up), finished (give_up), finished (give_up) | 0 | 4 | 7 | 14,381 | 76.73 | event {'type': 'suspend', 'user': 'Priya Raman'} |
| A4 | `admin-filter-page` | 3 | stuck (None), stuck (None), finished (give_up) | 0 | 12 | 13 | 31,971 | 97.03 | state page=2 (have 1) |
| A4 | `admin-tm-delete` | 1 | finished (give_up) | 0 | 2 | 5 | 9,222 | 32.96 | – |
| A5 | `admin-suspend` | 3 | finished (give_up), finished (give_up), cap_hit/model_calls (None) | 0 | 0 | 52 | 579,648 | 24.86 | event {'type': 'suspend', 'user': 'Priya Raman'} |
| A5 | `admin-filter-page` | 3 | cap_hit/model_calls (None), cap_hit/model_calls (None), cap_hit/model_calls (None) | 0 | 3 | 120 | 534,283 | 45.15 | state page=2 (have 1) |
| A5 | `admin-tm-delete` | 1 | finished (give_up) | 0 | 0 | 14 | 143,564 | 7.62 | – |
| A6 | `admin-suspend` | 3 | finished (give_up), finished (give_up), finished (give_up) | 0 | 0 | 11 | 57,423 | 44.01 | event {'type': 'suspend', 'user': 'Priya Raman'} |
| A6 | `admin-filter-page` | 3 | finished (done), finished (done), finished (done) | 0 | 4 | 21 | 174,696 | 81.25 | state page=2 (have 1) |
| A6 | `admin-tm-delete` | 1 | finished (give_up) | 0 | 1 | 4 | 18,479 | 12.39 | – |
| A7 | `admin-suspend` | 1 | finished (done) | 0 | 4 | 8 | 22,705 | 48.23 | – |
| A7 | `admin-filter-page` | 3 | finished (give_up), finished (give_up), finished (give_up) | 0 | 14 | 25 | 82,307 | 147.59 | state page=2 (have 1) |
| A7 | `admin-tm-delete` | 2 | finished (done), finished (give_up) | 0 | 2 | 5 | 11,118 | 63.43 | final in ['give_up'] (have 'done') |
| A8 | `admin-suspend` | 1 | finished (done) | 0 | 4 | 8 | 17,621 | 26.58 | – |
| A8 | `admin-filter-page` | 1 | finished (done) | 0 | 4 | 7 | 24,990 | 30.22 | – |
| A8 | `admin-tm-delete` | 3 | finished (done), finished (done), finished (give_up) | 0 | 3 | 9 | 17,623 | 35.55 | final in ['give_up'] (have 'done') |

## Manifest

| Item | Value |
|---|---|
| Report id | `20261005-131001+20261005-141625+20261005-153648` |
| Run `20261005-131001` started | 2026-10-05 13:10:04 +0800 |
| Run `20261005-131001` bench HEAD | `456b9c062a06` |
| Run `20261005-131001` hyper-use merged | `4ae30d3` |
| Run `20261005-131001` TYPESAFE_API_KEY present | False |
| Run `20261005-141625` started | 2026-10-05 14:16:28 +0800 |
| Run `20261005-141625` bench HEAD | `984ac2c6a515` |
| Run `20261005-141625` hyper-use merged | `4ae30d3` |
| Run `20261005-141625` TYPESAFE_API_KEY present | True |
| Run `20261005-153648` started | 2026-10-05 15:36:51 +0800 |
| Run `20261005-153648` bench HEAD | `570d00f0368f` (dirty) |
| Run `20261005-153648` hyper-use merged | `4ae30d3` |
| Run `20261005-153648` TYPESAFE_API_KEY present | True |
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
