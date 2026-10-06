# ADR 0007 — Harder page types: iframe, shadow, virtualized, autocomplete

## Status

Accepted (2026-10-06)

## Context

Offline Agent + Instinct + ticket coverage did not include iframes, shadow DOM,
virtualized lists, or autocomplete suggestion popups ([EVAL](../EVAL.md)).
CDP observe used `DOM.getDocument` with `pierce: false`, so open shadow roots
and same-origin iframe documents never entered the DOM walk. ARIA `combobox` /
`option` / `listbox` collapsed to weak roles (Generic + Focus only), so
ActionSpace could not offer TYPE on a combobox or CLICK on a suggestion.

## Decision

1. **Pierce open trees.** `BrowserSession::observe` calls
   `DOM.getDocument` with `pierce: true`. `extract::walk_dom` follows
   `children`, then open `shadowRoots`, then same-origin `contentDocument`.
2. **Roles.** Add `Role::ComboBox`, `Role::ListBox`, `Role::Option` with
   ActionSpace claims: ComboBox → Type (+ Click / Select), Option → Click /
   Select, ListBox → Focus / Click / Select. Native `<select>` stays
   `Generic` + tag-level Select (Chrome AX often reports `combobox`; Generic
   keeps fusion compatible).
3. **Virtualized lists.** Only the **visible window** is observed (nodes with
   a box). After `SCROLL_*`, the agent re-observes; recycled rows appear as
   new regions (identity rematch by label when possible). No off-screen
   inventory.
4. **Autocomplete.** TYPE into combobox → re-observe → ticketed CLICK /
   SELECT on an option. Multi-step `then` remains the planner; tickets still
   revalidate on a fresh observe. After ticketed TYPE, a read-only option
   signature poll runs at most 10 times at 25 ms intervals, ends early when
   nonempty results stabilize, and never re-dispatches input.
5. **Fixtures.** `ScriptBuilder` / `DomSpec` emit `shadowRoots` and
   `contentDocument`; box-model scripting matches kept-only extract order.

## Limits (honest)

| Case | Behavior |
|------|----------|
| Cross-origin iframe | No `contentDocument`; contents invisible |
| Closed shadow root | Not in `shadowRoots`; invisible |
| Infinite / windowed scroll | Only current visible rows; must scroll + re-observe |
| Autocomplete without option nodes | Cannot SELECT from a page option list the manifold never saw |
| AX-only / DOM-only mismatch for new roles | Same fusion rules (`Generic` wildcards; equal roles) |

## Consequences

- Observe manifolds can include controls inside open shadow and same-origin
  frames without a second CDP session.
- EVAL / AGENTS document the remaining true gaps above.
- Live A/B/D and paid remote remain out of scope.

## Verifiers

- `extract::pierce_tests`, `script::pierce_script_tests`
- `adversarial::{autocomplete_type_then_click_option_with_ticket_revalidate,virtualized_list_scroll_then_click_newly_visible_row}`
- `replay_cdp::autocomplete_type_then_option_click_over_cdp`
