# Combined arms A7 and A8: protocol and decisions

A7 = GPT 6 Luna + JEV + ultra-instinct + CUA driver. A8 = GPT 6 Luna + JEV + ultra-instinct + Browser Use.
Code: `combo.py` (shared orchestrator), `a7_combo_cua.py`, `a8_combo_bu.py`. Same harness, caps,
goal text, seed, viewport, and scoring as A1 to A6 (`../README.md`).

## Who owns what

| Piece | Owns | Does not do |
|---|---|---|
| Luna (planner) | The tool-calling loop: reads the page, states one intent per call (`press`, `type_text`, `select_option`, `scroll`, `fallback_click`), decides done / give up | Never sees region ids for clicks, never passes confidences |
| ultra-instinct (`ultra-instinct mcp`, CDP) | `observe` (the page view Luna gets after every action), `locate` (ranking), `inspect` (box), `guard` (front-layer check; never clicks) | Typing, select, scroll, and the click itself (harness CDP-clicks after a confidence-gated pick) |
| JEV (TypeSafe systemone, `jev-latest`) | Picks one candidate id when the deterministic ranker cannot separate the top two. Always offered a `NONE` option | Planning, writing text |
| Executor: CUA driver (A7) / Browser Use (A8) | `type_text`, `select_option`, `scroll`, `read_page` (its own page format), and `fallback_click` | Clicking anything ultra-instinct could press |

## The loop

1. The arm opens the page view with ultra-instinct `observe` (regions: id, role, label, non-default state).
2. Luna calls one tool. Every action returns the result plus a fresh ultra-instinct `observe`, so Luna
   does not spend a model call re-observing.
3. `press(target, context?, role?, position?)`:
   1. ultra-instinct `locate(text=target, role, position)`. Candidates scoring >= 0.5 are plausible (a
      text miss is capped at 0.45 since `4ae30d3`).
   2. No plausible candidate: `NO MATCH`, nothing clicked, `fallback_click` unlocked.
   3. One clear winner (top >= 0.55 and margin >= 0.05, the same rule as ultra-instinct's act gate) and
      no disambiguating `context`: ultra-instinct `act` with its own top and runner-up scores.
   4. Otherwise (tie, look-alikes, or `context` given with several plausible candidates): each
      candidate gets its ultra-instinct `inspect` box and the text of its enclosing row / card / dialog
      (one read-only `Runtime.evaluate` at the box centre). JEV picks one id or `NONE`.
   5. Confidence gate on the pick (ultra-instinct totals or JEV p): top >= 0.55 and margin >= 0.05;
      unsure or `NONE` refuses and nothing is clicked. Product `act`/`guard` never clicks, so the
      harness then CDP-clicks the picked region's center (`Input.dispatchMouseEvent`), after a
      `guard` front-layer check. `proposed-not-top` does not block a confidence-gated host pick
      (JEV already separated look-alike labels the matcher cannot).
   6. A press that executes with an empty state delta is reported as "no visible effect" and also
      unlocks `fallback_click`.
4. `type_text(field, text)` / `select_option(field, option)` / `fallback_click(target)`: the executor
   lists its own elements of that kind (A7: CUA `get_browser_state` semantic_v2 refs; A8: Browser Use
   `selector_map` nodes with their enclosing container text). A single exact label match is used
   directly; otherwise JEV picks among the label-matching elements (or `NONE`), with the same 0.55 /
   0.05 floor. The executor then acts: A7 `browser_type` (replace) / `browser_click`
   (`dom_event` route, as in A4) / `browser_pointer` scroll; A8 Browser Use `Tools` actions `input`,
   `select_dropdown`, `click`, `scroll` by index (no Browser Use LLM agent).
5. `fallback_click` is refused unless the previous press failed (no match, refused, or no visible
   effect). This keeps ultra-instinct the primary clicker and the executor a fallback.
6. Verification: ultra-instinct's state delta on every press plus the fresh observe; `read_page` when
   observe lacks detail (values, checked state, iframes). Ground truth stays the page journal.

## Wiring details

- A8: one harness Chrome (headless, 1280x800). ultra-instinct and Browser Use attach to the same CDP
  endpoint; Browser Use's session starts lazily on the first executor call.
- A7: CUA driver launches its own isolated Chrome (`browser_prepare isolated_new`), floated and sized
  to a 1280x800 viewport exactly like A2/A4. That Chrome starts with `--remote-debugging-port=0`; the
  arm finds its loopback DevTools port with `lsof` on the driver-owned pid and points ultra-instinct at
  it, so both engines act on the same tab. Setup (launch, size, bind) is ~15 s per attempt and
  counts toward wall time, as it does for A2 and A4.
- Both arms run in the main bench env and talk to `ultra-instinct mcp` and `cua-driver mcp` with the
  same tiny stdio JSON-RPC client (`mcp_stdio.py`).
- JEV calls go through the counting proxy (`/typesafe/v1/systemone`), Luna through `/luna/v1`, so
  the 40-call cap counts both. ultra-instinct and cua-driver are started without `TYPESAFE_API_KEY`, so
  every JEV call is made (and counted) by the orchestrator.
- Trace: each acting tool is a step; `jev`, `press`, and `combo_stats` rows record which picker
  decided each press, every JEV choice with its candidates, and how often each piece ran.

## Alternatives discarded

- **JEV on every step (as in A5 / A3 / A4).** A5 hit the model-call cap on 21 of 69 attempts.
  JEV here only decides what the ranker cannot, which keeps calls and wall time down.
- **Luna picks region ids itself (as in A6).** A6 failed look-alike rows (`am-star`, `hd-star`,
  `shop-add-qty`) because labels alone could not separate them. The combo routes those to JEV with row
  context instead.
- **ultra-instinct's built-in `browser-use` / `cua` executors.** `crates/ultra-instinct-browser-use` and
  `crates/ultra-instinct-cua` are replay transports with no live process, so the executor sits in the
  orchestrator.
- **A Browser Use sub-agent for typing.** It would add Luna calls per field. Direct `Tools`
  actions by index are deterministic and cost no model call.
- **Exposing the full executor toolset to Luna.** Luna would bypass ultra-instinct and the arm would just
  be A1 or A2 with overhead. `fallback_click` is gated instead.

## Downsides accepted and limits

- A7 select: cua-driver 0.23.2 has no select tool and its default (trusted) input route is refused
  for a background window. The arm types the option with `mode=keystrokes` into the `<select>`
  (Chrome's type-to-select). If that does not take, the select fails, as it did for A2.
- Candidate context is read with one `Runtime.evaluate` per ambiguous candidate (read-only). It is
  part of the orchestrator, not ultra-instinct's API.
- ultra-instinct's role vocabulary is small (button, link, checkbox, menuitem, tab, text_field, slider,
  generic, image, heading, navigation, text); the `role` enum shows only those, and an unknown role
  retries without it.
- n = 1 per task (up to 3 tries), like A1 to A6. One flipped task moves an arm by about 4 points.
