# booking-calendar: results

Merged runs: `20261005-131001`, `20261005-141625`, `20261005-153648`. Tasks: `examples/booking-calendar/tasks.yaml`. Site: `examples/booking-calendar/site/`.

**n = 1 run per arm per task** (up to 3 fresh tries, stopping at the first pass). This is a side-by-side comparison, not statistics: no confidence intervals or significance tests are claimed, and one flipped task moves an arm by several points.

## Leaderboard

| Rank | Arm | First-try accuracy | Pass within 3 | Press-only tasks, first try | Wrong actions (first try / all tries) | Safe refusals (target-missing) | cap_hit | stuck | crashed | Steps | Model calls | Tokens (first / all) | Wall first-try s | Wall total s | Wall p50 s (first try) | Untrusted clicks |
|---:|---|---|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | **A3** jev-ultrafast (BU + JEV) | 100% (3/3) | 100% (3/3) | 100% (2/2) | 0 / 0 | 0/0 | 0 | 0 | 0 | 6 | 9 | 35,285 / 35,285 | 54.4 | 54.4 | 18.5 | 0 |
| 2 | **A8** Luna + JEV + ultra-instinct + Browser Use | 100% (3/3) | 100% (3/3) | 100% (2/2) | 0 / 0 | 0/0 | 0 | 0 | 0 | 15 | 33 | 65,259 / 65,259 | 110.1 | 110.1 | 27.2 | 10 |
| 3 | **A1** Luna + Browser Use | 100% (3/3) | 100% (3/3) | 100% (2/2) | 0 / 0 | 0/0 | 0 | 0 | 0 | 7 | 13 | 118,626 / 118,626 | 111.7 | 111.7 | 36.5 | 0 |
| 4 | **A4** CUA jev-use (generic task) | 67% (2/3) | 67% (2/3) | 100% (2/2) | 0 / 0 | 0/0 | 0 | 3 | 0 | 13 | 15 | 18,155 / 26,899 | 83.5 | 137.1 | 26.7 | 4 |
| 5 | **A7** Luna + JEV + ultra-instinct + CUA | 67% (2/3) | 67% (2/3) | 100% (2/2) | 0 / 0 | 0/0 | 0 | 0 | 0 | 11 | 31 | 36,111 / 57,844 | 106.0 | 179.7 | 39.7 | 4 |
| 6 | **A2** Luna + CUA driver | 33% (1/3) | 67% (2/3) | 50% (1/2) | 0 / 0 | 0/0 | 0 | 0 | 0 | 18 | 40 | 23,590 / 129,515 | 105.8 | 314.3 | 37.6 | 6 |
| 7 | **A5** JEV + ultra-instinct (live_drive) | 0% (0/3) | 0% (0/3) | 0% (0/2) | 0 / 0 | 0/0 | 4 | 0 | 0 | 10 | 280 | 276,645 / 974,276 | 31.2 | 103.3 | 11.2 | 7 |
| 8 | **A6** Luna + ultra-instinct (MCP) | 0% (0/3) | 0% (0/3) | 0% (0/2) | 0 / 0 | 0/0 | 0 | 0 | 0 | 9 | 61 | 110,427 / 349,958 | 88.3 | 243.8 | 28.8 | 9 |

## Per task (attempts in order)

| Task | Class | Needs | A1 | A2 | A3 | A4 | A5 | A6 | A7 | A8 |
|---|---|---|---|---|---|---|---|---|---|---|
| `book-slot` | normal | press | PASS | fail → PASS | PASS | PASS | fail(model_calls) → fail(model_calls) → fail(model_calls) | fail → fail → fail | PASS | PASS |
| `book-timezone` | normal | select | PASS | fail → fail → fail | PASS | fail(stuck) → fail(stuck) → fail(stuck) | fail → fail → fail | fail → fail → fail | fail → fail → fail | PASS |
| `book-noop` | no-op | press | PASS | PASS | PASS | PASS | fail → fail(model_calls) → fail | fail → fail → fail | PASS | PASS |

## Detail

| Arm | Task | Tries | Outcomes (final status) | Wrong actions | Steps | Model calls | Tokens | Wall s | Unmet / forbidden (first try) |
|---|---|---:|---|---:|---:|---:|---:|---:|---|
| A1 | `book-slot` | 1 | finished (done) | 0 | 4 | 6 | 59,284 | 60.45 | – |
| A1 | `book-timezone` | 1 | finished (done) | 0 | 3 | 5 | 46,618 | 36.55 | – |
| A1 | `book-noop` | 1 | finished (done) | 0 | 0 | 2 | 12,724 | 14.73 | – |
| A2 | `book-slot` | 2 | finished (done), finished (done) | 0 | 7 | 17 | 87,853 | 132.63 | event {'type': 'booked', 'date': '2026-10-14', 'time': '2:30 PM'} |
| A2 | `book-timezone` | 3 | finished (give_up), finished (done), finished (give_up) | 0 | 11 | 21 | 38,630 | 152.31 | event {'type': 'settings_saved', 'timezone': 'Asia/Manila'} |
| A2 | `book-noop` | 1 | finished (done) | 0 | 0 | 2 | 3,032 | 29.35 | – |
| A3 | `book-slot` | 1 | finished (done) | 0 | 4 | 5 | 20,354 | 18.81 | – |
| A3 | `book-timezone` | 1 | finished (done) | 0 | 2 | 3 | 11,268 | 18.54 | – |
| A3 | `book-noop` | 1 | finished (done) | 0 | 0 | 1 | 3,663 | 17.01 | – |
| A4 | `book-slot` | 1 | finished (done) | 0 | 4 | 5 | 12,419 | 38.85 | – |
| A4 | `book-timezone` | 3 | stuck (None), stuck (None), stuck (None) | 0 | 9 | 9 | 13,116 | 80.27 | event {'type': 'settings_saved', 'timezone': 'Asia/Manila'} |
| A4 | `book-noop` | 1 | finished (done) | 0 | 0 | 1 | 1,364 | 18.01 | – |
| A5 | `book-slot` | 3 | cap_hit/model_calls (None), cap_hit/model_calls (None), cap_hit/model_calls (None) | 0 | 3 | 120 | 339,180 | 41.75 | event {'type': 'booked', 'date': '2026-10-14', 'time': '2:30 PM'} |
| A5 | `book-timezone` | 3 | finished (give_up), finished (give_up), finished (give_up) | 0 | 4 | 90 | 359,854 | 34.07 | event {'type': 'settings_saved', 'timezone': 'Asia/Manila'} |
| A5 | `book-noop` | 3 | finished (give_up), cap_hit/model_calls (None), finished (give_up) | 0 | 3 | 70 | 275,242 | 27.44 | state draft.reminders=True (have False) |
| A6 | `book-slot` | 3 | finished (done), finished (done), finished (done) | 0 | 6 | 28 | 265,408 | 112.68 | event {'type': 'booked', 'date': '2026-10-14', 'time': '2:30 PM'} |
| A6 | `book-timezone` | 3 | finished (give_up), finished (give_up), finished (give_up) | 0 | 0 | 14 | 33,260 | 48.73 | event {'type': 'settings_saved', 'timezone': 'Asia/Manila'} |
| A6 | `book-noop` | 3 | finished (done), finished (done), finished (done) | 0 | 3 | 19 | 51,290 | 82.42 | state draft.reminders=True (have False) |
| A7 | `book-slot` | 1 | finished (done) | 0 | 4 | 9 | 23,393 | 39.74 | – |
| A7 | `book-timezone` | 3 | finished (give_up), finished (give_up), finished (give_up) | 0 | 7 | 19 | 30,528 | 113.91 | event {'type': 'settings_saved', 'timezone': 'Asia/Manila'} |
| A7 | `book-noop` | 1 | finished (done) | 0 | 0 | 3 | 3,923 | 26.08 | – |
| A8 | `book-slot` | 1 | finished (done) | 0 | 4 | 9 | 22,354 | 27.19 | – |
| A8 | `book-timezone` | 1 | finished (done) | 0 | 3 | 7 | 9,807 | 24.48 | – |
| A8 | `book-noop` | 1 | finished (done) | 0 | 8 | 17 | 33,098 | 58.44 | – |

## Manifest

| Item | Value |
|---|---|
| Report id | `20261005-131001+20261005-141625+20261005-153648` |
| Run `20261005-131001` started | 2026-10-05 13:10:04 +0800 |
| Run `20261005-131001` bench HEAD | `456b9c062a06` |
| Run `20261005-131001` ultra-instinct merged | `4ae30d3` |
| Run `20261005-131001` TYPESAFE_API_KEY present | False |
| Run `20261005-141625` started | 2026-10-05 14:16:28 +0800 |
| Run `20261005-141625` bench HEAD | `984ac2c6a515` |
| Run `20261005-141625` ultra-instinct merged | `4ae30d3` |
| Run `20261005-141625` TYPESAFE_API_KEY present | True |
| Run `20261005-153648` started | 2026-10-05 15:36:51 +0800 |
| Run `20261005-153648` bench HEAD | `570d00f0368f` (dirty) |
| Run `20261005-153648` ultra-instinct merged | `4ae30d3` |
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
