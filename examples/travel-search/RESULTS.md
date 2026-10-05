# travel-search: results

Merged runs: `20261005-131001`, `20261005-141625`, `20261005-153648`. Tasks: `examples/travel-search/tasks.yaml`. Site: `examples/travel-search/site/`.

**n = 1 run per arm per task** (up to 3 fresh tries, stopping at the first pass). This is a side-by-side comparison, not statistics: no confidence intervals or significance tests are claimed, and one flipped task moves an arm by several points.

## Leaderboard

| Rank | Arm | First-try accuracy | Pass within 3 | Press-only tasks, first try | Wrong actions (first try / all tries) | Safe refusals (target-missing) | cap_hit | stuck | crashed | Steps | Model calls | Tokens (first / all) | Wall first-try s | Wall total s | Wall p50 s (first try) | Untrusted clicks |
|---:|---|---|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | **A6** Luna + hyper-use (MCP) | 50% (1/2) | 50% (1/2) | 100% (1/1) | 0 / 0 | 1/1 | 0 | 0 | 0 | 0 | 13 | 16,325 / 41,179 | 24.6 | 56.3 | 12.3 | 0 |
| 2 | **A8** Luna + JEV + hyper-use + Browser Use | 50% (1/2) | 100% (2/2) | 100% (1/1) | 0 / 0 | 1/1 | 0 | 0 | 0 | 13 | 20 | 34,942 / 71,027 | 44.1 | 81.8 | 22.1 | 5 |
| 3 | **A1** Luna + Browser Use | 50% (1/2) | 100% (2/2) | 100% (1/1) | 0 / 0 | 1/1 | 0 | 0 | 0 | 14 | 15 | 59,912 / 131,130 | 63.4 | 129.9 | 31.7 | 0 |
| 4 | **A4** CUA jev-use (generic task) | 50% (1/2) | 50% (1/2) | 100% (1/1) | 0 / 0 | 1/1 | 0 | 3 | 0 | 20 | 21 | 17,174 / 40,992 | 68.8 | 148.3 | 34.4 | 4 |
| 5 | **A7** Luna + JEV + hyper-use + CUA | 50% (1/2) | 50% (1/2) | 100% (1/1) | 0 / 0 | 1/1 | 0 | 0 | 0 | 20 | 33 | 41,168 / 138,372 | 93.0 | 244.3 | 46.5 | 5 |
| 6 | **A2** Luna + CUA driver | 50% (1/2) | 50% (1/2) | 100% (1/1) | 0 / 0 | 1/1 | 0 | 0 | 0 | 21 | 43 | 54,564 / 152,553 | 125.7 | 303.8 | 62.8 | 4 |
| 7 | **A5** JEV + hyper-use (live_drive) | 0% (0/2) | 0% (0/2) | 0% (0/1) | 0 / 0 | 0/1 | 6 | 0 | 0 | 3 | 240 | 466,268 / 1,399,584 | 29.6 | 90.8 | 14.8 | 3 |
| 8 | **A3** jev-ultrafast (BU + JEV) | 0% (0/2) | 0% (0/2) | 0% (0/1) | 0 / 0 | 0/1 | 0 | 0 | 6 | 0 | 12 | 6,806 / 20,418 | 38.4 | 113.2 | 19.2 | 0 |

## Per task (attempts in order)

| Task | Class | Needs | A1 | A2 | A3 | A4 | A5 | A6 | A7 | A8 |
|---|---|---|---|---|---|---|---|---|---|---|
| `travel-lisbon` | normal | type, select | fail → fail → PASS | fail → fail → fail | fail(crashed) → fail(crashed) → fail(crashed) | fail(stuck) → fail(stuck) → fail(stuck) | fail(model_calls) → fail(model_calls) → fail(model_calls) | fail → fail → fail | fail → fail → fail | fail → PASS |
| `travel-tm` | target-missing | press | PASS | PASS | fail(crashed) → fail(crashed) → fail(crashed) | PASS | fail(model_calls) → fail(model_calls) → fail(model_calls) | PASS | PASS | PASS |

## Detail

| Arm | Task | Tries | Outcomes (final status) | Wrong actions | Steps | Model calls | Tokens | Wall s | Unmet / forbidden (first try) |
|---|---|---:|---|---:|---:|---:|---:|---:|---|
| A1 | `travel-lisbon` | 3 | finished (done), finished (done), finished (done) | 0 | 13 | 12 | 106,931 | 102.6 | event {'type': 'open', 'place': 'Casa Flora', 'category': 'Design', 'free': True, 'query': '~lisbon'} |
| A1 | `travel-tm` | 1 | finished (give_up) | 0 | 1 | 3 | 24,199 | 27.3 | – |
| A2 | `travel-lisbon` | 3 | finished (give_up), finished (done), finished (give_up) | 0 | 19 | 37 | 137,420 | 254.79 | event {'type': 'open', 'place': 'Casa Flora', 'category': 'Design', 'free': True, 'query': '~lisbon'} |
| A2 | `travel-tm` | 1 | finished (give_up) | 0 | 2 | 6 | 15,133 | 48.96 | – |
| A3 | `travel-lisbon` | 3 | crashed (error), crashed (error), crashed (error) | 0 | 0 | 6 | 10,293 | 55.88 | event {'type': 'open', 'place': 'Casa Flora', 'category': 'Design', 'free': True, 'query': '~lisbon'} |
| A3 | `travel-tm` | 3 | crashed (error), crashed (error), crashed (error) | 0 | 0 | 6 | 10,125 | 57.37 | final in ['give_up'] (have 'error') |
| A4 | `travel-lisbon` | 3 | stuck (None), stuck (None), stuck (None) | 0 | 18 | 18 | 35,727 | 118.08 | event {'type': 'open', 'place': 'Casa Flora', 'category': 'Design', 'free': True, 'query': '~lisbon'} |
| A4 | `travel-tm` | 1 | finished (give_up) | 0 | 2 | 3 | 5,265 | 30.23 | – |
| A5 | `travel-lisbon` | 3 | cap_hit/model_calls (None), cap_hit/model_calls (None), cap_hit/model_calls (None) | 0 | 0 | 120 | 839,082 | 45.3 | event {'type': 'open', 'place': 'Casa Flora', 'category': 'Design', 'free': True, 'query': '~lisbon'} |
| A5 | `travel-tm` | 3 | cap_hit/model_calls (None), cap_hit/model_calls (None), cap_hit/model_calls (None) | 0 | 3 | 120 | 560,502 | 45.52 | final in ['give_up'] (have None) |
| A6 | `travel-lisbon` | 3 | finished (give_up), finished (give_up), finished (give_up) | 0 | 0 | 10 | 31,678 | 43.98 | event {'type': 'open', 'place': 'Casa Flora', 'category': 'Design', 'free': True, 'query': '~lisbon'} |
| A6 | `travel-tm` | 1 | finished (give_up) | 0 | 0 | 3 | 9,501 | 12.37 | – |
| A7 | `travel-lisbon` | 3 | finished (give_up), finished (give_up), finished (done) | 0 | 18 | 30 | 133,021 | 217.84 | event {'type': 'open', 'place': 'Casa Flora', 'category': 'Design', 'free': True, 'query': '~lisbon'} |
| A7 | `travel-tm` | 1 | finished (give_up) | 0 | 2 | 3 | 5,351 | 26.5 | – |
| A8 | `travel-lisbon` | 2 | finished (done), finished (done) | 0 | 11 | 17 | 66,246 | 69.83 | event {'type': 'open', 'place': 'Casa Flora', 'category': 'Design', 'free': True, 'query': '~lisbon'} |
| A8 | `travel-tm` | 1 | finished (give_up) | 0 | 2 | 3 | 4,781 | 11.93 | – |

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
