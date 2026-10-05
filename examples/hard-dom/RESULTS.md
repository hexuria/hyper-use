# hard-dom: results

Merged runs: `20261005-131001`, `20261005-141625`. Tasks: `examples/hard-dom/tasks.yaml`. Site: `examples/hard-dom/site/`.

**n = 1 run per arm per task** (up to 3 fresh tries, stopping at the first pass). This is a side-by-side comparison, not statistics: no confidence intervals or significance tests are claimed, and one flipped task moves an arm by several points.

## Leaderboard

| Rank | Arm | First-try accuracy | Pass within 3 | Press-only tasks, first try | Wrong actions (first try / all tries) | Safe refusals (target-missing) | cap_hit | stuck | crashed | Steps | Model calls | Tokens (first / all) | Wall first-try s | Wall total s | Wall p50 s (first try) | Untrusted clicks |
|---:|---|---|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | **A2** Luna + CUA driver | 100% (4/4) | 100% (4/4) | 100% (4/4) | 0 / 0 | 0/0 | 0 | 0 | 0 | 9 | 20 | 49,189 / 49,189 | 177.2 | 177.2 | 44.6 | 5 |
| 2 | **A4** CUA jev-use (generic task) | 75% (3/4) | 75% (3/4) | 75% (3/4) | 1 / 3 | 0/0 | 0 | 0 | 0 | 6 | 12 | 15,149 / 22,759 | 95.2 | 142.8 | 23.8 | 6 |
| 3 | **A1** Luna + Browser Use | 75% (3/4) | 100% (4/4) | 75% (3/4) | 0 / 0 | 0/0 | 0 | 0 | 0 | 8 | 17 | 122,085 / 145,995 | 116.4 | 141.4 | 30.2 | 5 |
| 4 | **A6** Luna + hyper-use (MCP) | 50% (2/4) | 75% (3/4) | 50% (2/4) | 0 / 0 | 0/0 | 0 | 0 | 0 | 10 | 48 | 108,371 / 323,977 | 77.5 | 189.8 | 15.9 | 3 |
| 5 | **A5** JEV + hyper-use (live_drive) | 25% (1/4) | 25% (1/4) | 25% (1/4) | 0 / 0 | 0/0 | 3 | 3 | 0 | 12 | 221 | 678,147 / 1,866,426 | 35.4 | 94.7 | 7.2 | 3 |
| 6 | **A3** jev-ultrafast (BU + JEV) | 0% (0/4) | 0% (0/4) | 0% (0/4) | 0 / 0 | 0/0 | 1 | 0 | 2 | 0 | 85 | 138,791 / 274,297 | 65.4 | 223.4 | 17.1 | 0 |

## Per task (attempts in order)

| Task | Class | Needs | A1 | A2 | A3 | A4 | A5 | A6 |
|---|---|---|---|---|---|---|---|---|
| `hd-star` | normal | press | PASS | PASS | fail(model_calls) → fail(crashed) → fail(crashed) | fail → fail → fail | fail(model_calls) → fail(model_calls) → fail(model_calls) | fail → fail → PASS |
| `hd-shadow` | normal | press | PASS | PASS | fail → fail → fail | PASS | fail(stuck) → fail(stuck) → fail(stuck) | PASS |
| `hd-iframe` | normal | press | fail → PASS | PASS | fail → fail → fail | PASS | fail → fail → fail | fail → fail → fail |
| `hd-overlay` | normal | press | PASS | PASS | fail → fail → fail | PASS | PASS | PASS |

## Detail

| Arm | Task | Tries | Outcomes (final status) | Wrong actions | Steps | Model calls | Tokens | Wall s | Unmet / forbidden (first try) |
|---|---|---:|---|---:|---:|---:|---:|---:|---|
| A1 | `hd-star` | 1 | finished (done) | 0 | 3 | 4 | 36,609 | 35.83 | – |
| A1 | `hd-shadow` | 1 | finished (done) | 0 | 3 | 5 | 48,660 | 41.64 | – |
| A1 | `hd-iframe` | 2 | finished (give_up), finished (done) | 0 | 1 | 5 | 36,720 | 39.43 | event {'type': 'cookies', 'choice': 'reject'} |
| A1 | `hd-overlay` | 1 | finished (done) | 0 | 1 | 3 | 24,006 | 24.53 | – |
| A2 | `hd-star` | 1 | finished (done) | 0 | 2 | 5 | 14,003 | 47.63 | – |
| A2 | `hd-shadow` | 1 | finished (done) | 0 | 2 | 5 | 12,849 | 41.52 | – |
| A2 | `hd-iframe` | 1 | finished (done) | 0 | 2 | 4 | 8,698 | 38.08 | – |
| A2 | `hd-overlay` | 1 | finished (done) | 0 | 3 | 6 | 13,639 | 49.99 | – |
| A3 | `hd-star` | 3 | cap_hit/model_calls (None), crashed (error), crashed (error) | 0 | 0 | 76 | 245,404 | 69.6 | event {'type': 'star', 'file': 'Roadmap 2027', 'starred': True} |
| A3 | `hd-shadow` | 3 | finished (give_up), finished (give_up), finished (give_up) | 0 | 0 | 3 | 9,618 | 51.05 | event {'type': 'publish', 'doc': 'Release notes'} |
| A3 | `hd-iframe` | 3 | finished (give_up), finished (give_up), finished (give_up) | 0 | 0 | 3 | 9,648 | 51.6 | event {'type': 'cookies', 'choice': 'reject'} |
| A3 | `hd-overlay` | 3 | finished (give_up), finished (give_up), finished (give_up) | 0 | 0 | 3 | 9,627 | 51.19 | event {'type': 'download', 'file': 'Q3 report'} |
| A4 | `hd-star` | 3 | finished (done), finished (done), finished (done) | 3 | 3 | 6 | 11,415 | 71.29 | event {'type': 'star', 'file': 'Roadmap 2027', 'starred': True}; forbidden star {"file": "Hiring plan", "starred": true} |
| A4 | `hd-shadow` | 1 | finished (done) | 0 | 1 | 2 | 3,811 | 23.88 | – |
| A4 | `hd-iframe` | 1 | finished (give_up) | 0 | 1 | 2 | 3,702 | 23.69 | – |
| A4 | `hd-overlay` | 1 | finished (done) | 0 | 1 | 2 | 3,831 | 23.9 | – |
| A5 | `hd-star` | 3 | cap_hit/model_calls (None), cap_hit/model_calls (None), cap_hit/model_calls (None) | 0 | 0 | 120 | 1,104,057 | 48.77 | event {'type': 'star', 'file': 'Roadmap 2027', 'starred': True} |
| A5 | `hd-shadow` | 3 | stuck (None), stuck (None), stuck (None) | 0 | 9 | 66 | 531,706 | 27.53 | event {'type': 'publish', 'doc': 'Release notes'} |
| A5 | `hd-iframe` | 3 | finished (done), finished (done), finished (give_up) | 0 | 2 | 24 | 156,261 | 12.85 | event {'type': 'cookies', 'choice': 'reject'} |
| A5 | `hd-overlay` | 1 | finished (done) | 0 | 1 | 11 | 74,402 | 5.58 | – |
| A6 | `hd-star` | 3 | finished (give_up), finished (give_up), finished (done) | 0 | 8 | 25 | 204,908 | 99.46 | event {'type': 'star', 'file': 'Roadmap 2027', 'starred': True} |
| A6 | `hd-shadow` | 1 | finished (done) | 0 | 1 | 5 | 28,233 | 16.48 | – |
| A6 | `hd-iframe` | 3 | finished (give_up), finished (give_up), finished (give_up) | 0 | 0 | 14 | 72,365 | 58.43 | event {'type': 'cookies', 'choice': 'reject'} |
| A6 | `hd-overlay` | 1 | finished (done) | 0 | 1 | 4 | 18,471 | 15.42 | – |

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
