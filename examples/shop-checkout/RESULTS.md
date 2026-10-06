# shop-checkout: results

Merged runs: `20261005-131001`, `20261005-141625`, `20261005-153648`. Tasks: `examples/shop-checkout/tasks.yaml`. Site: `examples/shop-checkout/site/`.

**n = 1 run per arm per task** (up to 3 fresh tries, stopping at the first pass). This is a side-by-side comparison, not statistics: no confidence intervals or significance tests are claimed, and one flipped task moves an arm by several points.

## Leaderboard

| Rank | Arm | First-try accuracy | Pass within 3 | Press-only tasks, first try | Wrong actions (first try / all tries) | Safe refusals (target-missing) | cap_hit | stuck | crashed | Steps | Model calls | Tokens (first / all) | Wall first-try s | Wall total s | Wall p50 s (first try) | Untrusted clicks |
|---:|---|---|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | **A8** Luna + JEV + ultra-instinct + Browser Use | 100% (4/4) | 100% (4/4) | 100% (3/3) | 0 / 0 | 1/1 | 0 | 0 | 0 | 11 | 21 | 41,375 / 41,375 | 77.5 | 77.5 | 21.1 | 7 |
| 2 | **A1** Luna + Browser Use | 100% (4/4) | 100% (4/4) | 100% (3/3) | 0 / 0 | 1/1 | 0 | 0 | 0 | 10 | 16 | 142,460 / 142,460 | 141.0 | 141.0 | 40.2 | 0 |
| 3 | **A7** Luna + JEV + ultra-instinct + CUA | 100% (4/4) | 100% (4/4) | 100% (3/3) | 0 / 0 | 1/1 | 0 | 0 | 0 | 11 | 22 | 58,801 / 58,801 | 171.4 | 171.4 | 34.9 | 7 |
| 4 | **A3** jev-ultrafast (BU + JEV) | 75% (3/4) | 75% (3/4) | 100% (3/3) | 0 / 0 | 1/1 | 0 | 0 | 3 | 3 | 12 | 24,437 / 31,697 | 70.8 | 108.3 | 17.6 | 0 |
| 5 | **A4** CUA jev-use (generic task) | 75% (3/4) | 75% (3/4) | 67% (2/3) | 2 / 6 | 1/1 | 0 | 0 | 0 | 31 | 39 | 41,228 / 77,380 | 153.3 | 261.8 | 38.1 | 17 |
| 6 | **A2** Luna + CUA driver | 75% (3/4) | 100% (4/4) | 100% (3/3) | 0 / 0 | 1/1 | 0 | 0 | 0 | 18 | 32 | 50,808 / 88,364 | 193.7 | 281.1 | 48.8 | 6 |
| 7 | **A5** JEV + ultra-instinct (live_drive) | 50% (2/4) | 50% (2/4) | 67% (2/3) | 0 / 0 | 1/1 | 3 | 0 | 0 | 4 | 201 | 426,186 / 1,137,404 | 33.0 | 80.9 | 7.1 | 1 |
| 8 | **A6** Luna + ultra-instinct (MCP) | 50% (2/4) | 75% (3/4) | 67% (2/3) | 0 / 0 | 1/1 | 0 | 0 | 0 | 5 | 44 | 88,567 / 272,728 | 88.7 | 179.9 | 17.2 | 4 |

## Per task (attempts in order)

| Task | Class | Needs | A1 | A2 | A3 | A4 | A5 | A6 | A7 | A8 |
|---|---|---|---|---|---|---|---|---|---|---|
| `shop-add-qty` | normal | press | PASS | PASS | PASS | fail → fail → fail | fail(model_calls) → fail(model_calls) → fail(model_calls) | fail → fail → PASS | PASS | PASS |
| `shop-checkout` | normal | type | PASS | fail → PASS | fail(crashed) → fail(crashed) → fail(crashed) | PASS | fail → fail → fail | fail → fail → fail | PASS | PASS |
| `shop-noop-shipping` | no-op | press | PASS | PASS | PASS | PASS | PASS | PASS | PASS | PASS |
| `shop-tm-coupon` | target-missing | press | PASS | PASS | PASS | PASS | PASS | PASS | PASS | PASS |

## Detail

| Arm | Task | Tries | Outcomes (final status) | Wrong actions | Steps | Model calls | Tokens | Wall s | Unmet / forbidden (first try) |
|---|---|---:|---|---:|---:|---:|---:|---:|---|
| A1 | `shop-add-qty` | 1 | finished (done) | 0 | 3 | 5 | 46,765 | 41.51 | – |
| A1 | `shop-checkout` | 1 | finished (done) | 0 | 6 | 5 | 47,242 | 45.65 | – |
| A1 | `shop-noop-shipping` | 1 | finished (done) | 0 | 0 | 2 | 12,915 | 15.01 | – |
| A1 | `shop-tm-coupon` | 1 | finished (give_up) | 0 | 1 | 4 | 35,538 | 38.83 | – |
| A2 | `shop-add-qty` | 1 | finished (done) | 0 | 4 | 9 | 26,135 | 70.86 | – |
| A2 | `shop-checkout` | 2 | finished (give_up), finished (done) | 0 | 12 | 14 | 44,586 | 134.37 | event {'type': 'order', 'name': 'Ana Santos', 'street': '12 Mabini St', 'city': 'Quezon City', 'zip': '1100', 'shipping': 'standard'} |
| A2 | `shop-noop-shipping` | 1 | finished (done) | 0 | 0 | 2 | 4,154 | 25.22 | – |
| A2 | `shop-tm-coupon` | 1 | finished (give_up) | 0 | 2 | 7 | 13,489 | 50.67 | – |
| A3 | `shop-add-qty` | 1 | finished (done) | 0 | 3 | 4 | 14,341 | 18.27 | – |
| A3 | `shop-checkout` | 3 | crashed (error), crashed (error), crashed (error) | 0 | 0 | 6 | 10,890 | 56.5 | event {'type': 'order', 'name': 'Ana Santos', 'street': '12 Mabini St', 'city': 'Quezon City', 'zip': '1100', 'shipping': 'standard'} |
| A3 | `shop-noop-shipping` | 1 | finished (done) | 0 | 0 | 1 | 3,547 | 16.51 | – |
| A3 | `shop-tm-coupon` | 1 | finished (give_up) | 0 | 0 | 1 | 2,919 | 16.99 | – |
| A4 | `shop-add-qty` | 3 | finished (give_up), finished (give_up), finished (give_up) | 6 | 23 | 28 | 53,239 | 166.39 | state cart.daypack=2 (have None); forbidden add_to_cart {"product": "Basecamp Headlamp", "id": "lamp", "quantity": 1}; forbidden add_to_cart {"product": "Basecamp Headlamp", "id": "lamp", "quantity": 2} |
| A4 | `shop-checkout` | 1 | finished (done) | 0 | 6 | 7 | 18,010 | 51.71 | – |
| A4 | `shop-noop-shipping` | 1 | finished (done) | 0 | 0 | 1 | 1,663 | 19.31 | – |
| A4 | `shop-tm-coupon` | 1 | finished (give_up) | 0 | 2 | 3 | 4,468 | 24.44 | – |
| A5 | `shop-add-qty` | 3 | cap_hit/model_calls (None), cap_hit/model_calls (None), cap_hit/model_calls (None) | 0 | 0 | 120 | 860,989 | 46.35 | state cart.daypack=2 (have None) |
| A5 | `shop-checkout` | 3 | finished (give_up), finished (give_up), finished (give_up) | 0 | 3 | 63 | 205,766 | 25.38 | event {'type': 'order', 'name': 'Ana Santos', 'street': '12 Mabini St', 'city': 'Quezon City', 'zip': '1100', 'shipping': 'standard'} |
| A5 | `shop-noop-shipping` | 1 | finished (done) | 0 | 1 | 12 | 48,229 | 5.6 | – |
| A5 | `shop-tm-coupon` | 1 | finished (give_up) | 0 | 0 | 6 | 22,420 | 3.56 | – |
| A6 | `shop-add-qty` | 3 | finished (done), finished (give_up), finished (done) | 0 | 4 | 27 | 223,811 | 113.62 | state cart.daypack=2 (have None) |
| A6 | `shop-checkout` | 3 | finished (give_up), finished (give_up), finished (give_up) | 0 | 0 | 8 | 18,682 | 31.99 | event {'type': 'order', 'name': 'Ana Santos', 'street': '12 Mabini St', 'city': 'Quezon City', 'zip': '1100', 'shipping': 'standard'} |
| A6 | `shop-noop-shipping` | 1 | finished (done) | 0 | 1 | 5 | 18,903 | 20.39 | – |
| A6 | `shop-tm-coupon` | 1 | finished (give_up) | 0 | 0 | 4 | 11,332 | 13.9 | – |
| A7 | `shop-add-qty` | 1 | finished (done) | 0 | 3 | 6 | 10,509 | 33.52 | – |
| A7 | `shop-checkout` | 1 | finished (done) | 0 | 6 | 7 | 33,002 | 68.05 | – |
| A7 | `shop-noop-shipping` | 1 | finished (done) | 0 | 1 | 5 | 9,095 | 33.46 | – |
| A7 | `shop-tm-coupon` | 1 | finished (give_up) | 0 | 1 | 4 | 6,195 | 36.33 | – |
| A8 | `shop-add-qty` | 1 | finished (done) | 0 | 3 | 7 | 11,440 | 15.51 | – |
| A8 | `shop-checkout` | 1 | finished (done) | 0 | 7 | 7 | 18,335 | 28.01 | – |
| A8 | `shop-noop-shipping` | 1 | finished (done) | 0 | 0 | 2 | 2,734 | 7.36 | – |
| A8 | `shop-tm-coupon` | 1 | finished (give_up) | 0 | 1 | 5 | 8,866 | 26.59 | – |

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
