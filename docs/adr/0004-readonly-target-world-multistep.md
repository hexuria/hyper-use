# ADR 0004: Readonly observe, target-scoped world, multi-step `then`

- Status: accepted
- Date: 2026-10-06 (Asia/Manila)
- Relates: ADR 0002, ADR 0003

## Context

PR #16 left honest gaps: readonly only failed inside the CDP input function,
ticket world fingerprints hashed the whole page (unrelated banners forced
stale discards), PUA was single-intent only, MCP `guard` still floated at
0.55 while the agent used hard `gate`, and `hyper-use run --fixture` lacked
checked-in type/select/click replays.

## Decision

1. **Readonly is observed.** `RegionFlags::readonly` from HTML `readonly` /
   `aria-readonly`. Hard `gate` refuses TYPE / SELECT before any input; action
   space omits those claims on readonly regions. Click remains allowed.
2. **Target-scoped ticket worlds.** `WorldSnapshot::of_target` fingerprints
   front layer (global) plus the target neighborhood (ancestors, same-parent
   siblings, children, geometrically nearby root peers). MCP `seen_world`
   comparison stays whole-page (`WorldSnapshot::of`).
3. **Multi-step without an LLM planner.** `split_sequential_clauses` splits on
   `then` / `and then` outside quotes. The agent runs one PUA clause at a time
   and advances on DONE. Limits are documented on that function.
4. **MCP alignment without breaking A5/A6.** Ranked `guard` still uses 0.55 /
   0.05. Before Allow it calls `gate::check`, and Allow tickets use
   `of_target` like the agent path.
5. **Checked-in run fixtures.** `fixtures/agent-{type-search,click-go,select-cabin}.cdp.json`.

## Out of scope

Full A/B/D live parity, model-backed TextResolver, iframes / shadow DOM /
virtualized lists / autocomplete.
