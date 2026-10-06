# hyper-use

The product, crates, and binary are `hyper-use`. HGRA is an experimental
matcher frozen under `experiments/hgra/`. A crate or binary named `hgra` on
the product path is a bug.

**Product:** Hyper-Use is a **Rust-native browser-agent runtime**
([`docs/PRD.md`](docs/PRD.md), [ADR 0001](docs/adr/0001-agent-runtime-pivot.md)).
The library `Agent` **owns the loop**: it observes the page, builds a finite
`ActionSpace`, lets a policy choose (PUA first, optional explicit escalation),
hard-gates the choice, executes only through a one-shot `ActionTicket`, then
re-observes, diffs, and verifies.

MCP (`hyper-use mcp`) is an **optional adapter**, not the orchestration
surface. Nothing on the product path requires MCP, Browser Use, CUA, or Jev.

Public API is 0.1 and unstable until 1.0. Toolchain pin: Rust 1.99.0
(`rust-toolchain.toml`). `publish = false`. `#![forbid(unsafe_code)]` on every
crate.

## Anti-drift (read before changing anything on the agent path)

> **Anti-drift block — Agent + PUA + ticket path (impeccable audit, baseline `87ffc2d`).**
>
> 1. **The agent owns the loop.** `observe → ActionSpace (front layer applied)
>    → policy → gate → ActionTicket → execute_ticketed → settle → observe →
>    diff / value check → history`. Do not reintroduce "Hyper-Use is not an
>    agent / does not click" framing, and do not make MCP or a host the loop.
> 2. **PUA owns HOW, Hyper-Use owns WHAT.** PUA picks among finite offered
>    actions (scores, threshold / margin, abstain). Hyper-Use supplies browser
>    evidence and builds the action space. No browser concepts in PUA. No
>    second float confidence gate on the agent path. Abstain is never turned
>    into "top candidate wins".
> 3. **Hard invalidity is guard evidence, not a score.** Disabled, readonly
>    (TYPE / SELECT), hidden / zero-area, occluded, front-layer, offscreen,
>    missing target, unsupported action → `hyper_use_guard::gate` refuses. The
>    ranked `guard()` + `0.55` / `0.05` float gate exists only for the MCP / CLI
>    preflight surface (historical A5 / A6 arms).
> 4. **One executor boundary, one staleness barrier.** `execute_ticketed` is
>    the only path from a ticket to page input. Order: ledger (one-shot) →
>    input kind == `ticket.action` → **fresh observe** → `revalidate`
>    (target-scoped world fingerprint + target role / label / fingerprint) →
>    hard gate on the fresh region → **mark consumed before dispatch** →
>    dispatch to `ticket.target_id`. The host helper `consume_ticket_once` uses
>    the same consume-before-press order. `Predicted` carries no fingerprint of
>    its own; do not add a "pre-ticket check" that is not wired into a refusal.
> 5. **Stale ≠ spent.** `WorldChanged` / `TargetChanged` / `TargetGone` are
>    stale → discard the prediction, re-observe, decide again (bounded by
>    `max_consecutive_stale`). `TicketConsumed` / `TicketMismatch` / gate
>    refusal / page rejection / dispatch failure are **not** stale and are
>    never silently retried with the same lease.
> 6. **Ticket worlds are target-scoped.** `WorldSnapshot::of_target`: front
>    layer and focus are global; clickable / occluded sets are the target
>    neighborhood (ancestors, same-parent siblings, children, root peers within
>    `NEIGHBOR_RADIUS_PX` = 160, inclusive). MCP `seen_world` stays whole-page
>    (`WorldSnapshot::of`).
> 7. **Text is not PUA.** `TYPE_TEXT` / `SELECT` payloads come from a
>    `TextResolver`, checked against their context fingerprint, before the
>    ticket is consumed, so executor revalidation always runs after resolver
>    latency. Payloads are CDP arguments, never spliced into script source;
>    there is no coordinate tier for text.
> 7a. **Model text is payload-only and grounded** (feature `model-text`,
>    ADR 0006). The model fills the payload of an action PUA already chose;
>    replies must echo the context fingerprint, pass shape checks, and occur
>    in the goal clause, else deterministic fallback, else abstain. CI uses
>    scripted models only; no provider SDK or key handling in the tree.
> 7b. **Harder page types** (ADR 0007). Observe pierces open shadow roots and
>    same-origin iframes (`pierce: true`). ComboBox / ListBox / Option are
>    first-class roles. Virtualized lists are the visible window only (scroll
>    + re-observe). Autocomplete is TYPE then ticketed option act after
>    re-observe. Do not claim cross-origin iframe or closed-shadow coverage.
> 8. **Remote escalation is explicit and closed.** Feature `remote`;
>    reply is exactly a choice id + kind from the offered menu or abstain.
>    Selectors, coordinates, scripts, extra fields → hard error.
> 9. **HGRA stays frozen.** No matcher tuning on the agent path.
> 10. **Formal tools stay unjustified** for this path (no `unsafe`, no
>    atomics shared across threads, single-threaded blocking CDP, no recovery
>    protocol): Loom / Kani / TLA+ / Miri / Lean are **NOT JUSTIFIED**. The
>    owners are unit tests, `proptest`, adversarial fixtures, replay fixtures,
>    cargo-mutants (nightly), and cargo-fuzz on the untrusted-input parsers
>    (nightly).
>
> Any change to observable semantics on this path names the boundary it
> affects (gate, ticket, executor, world, policy, text) and the test that owns
> it.

## Crates (product graph)

| Crate | Owns |
|---|---|
| `hyper-use-core` | `InteractionManifold`, regions, `ActionSpace`, fixture grammar |
| `hyper-use-browser` | CDP observe (DOM/AX fusion, identity, stacking), raw CDP inputs, replay |
| `hyper-use-observe` | Observation history and id diff |
| `hyper-use-geometry` | Geometry helpers |
| `hyper-use-policy` | `BrowserPolicy`, `PuaPolicy` (pinned `hexuria/pua` rev), `TextResolver`, `ModelTextResolver` (feature `model-text`), `RemotePolicy` (feature `remote`), multi-step clause split |
| `hyper-use-guard` | Hard `gate`, `ActionTicket` issue / `revalidate`, `TicketLedger`, `consume_ticket_once`, front layer + `WorldSnapshot`; ranked `guard()` for MCP preflight |
| `hyper-use-agent` | `Agent` state machine, `execute_ticketed`, `BrowserRuntime`, verification mapping, `MockBrowser` |
| `hyper-use-protocol` | Guard / ticket / verify wire types |
| `hyper-use-resonance` | `WeightedMatcher` (MCP / CLI locate + ranked guard only) |
| `hyper-use-mcp` | Optional JSON-RPC adapter (observe / guard / verify) |
| `hyper-use-cli` | `hyper-use run` (agent loop, `--policy pua|jev`); `observe` / `guard` / `verify` / `locate` / `inspect` / `diff` preflight helpers; `mcp` (stdio); `TypesafeTransport` (feature `jev`: JEV-primary over the closed remote wire) |

PUA is pinned by git rev in the workspace `Cargo.toml`; bump only with a
deliberate eval. HGRA lives in `experiments/hgra/` and the resonance `hgra`
feature, not the default product path.

## Verification owners

- Agent loop, stale discards, multi-step clauses: `hyper-use-agent` unit tests,
  `tests/mock_loop.rs`, `tests/adversarial.rs`, `tests/replay_cdp.rs`.
- Executor boundary: `executor.rs` unit tests, `tests/props.rs`
  (substitution, staleness, consumed-beats-stale, `is_stale` classification).
- Gate / ticket / world: `hyper-use-guard` unit tests,
  `tests/ticket_props.rs` (radius boundary 159 / 160 / 161, focus-only change,
  consumed never stale), `tests/world_context.rs`.
- PUA evidence: `hyper-use-policy` unit tests. Do not add a second model of
  PUA scoring or of `WeightedMatcher::rank`.
- `write_fixture` / `parse_fixture` own the manifold fixture grammar. CDP
  replay is a different grammar. Fusion is the only DOM/accessibility merge.
  Region identity across observations is owned by the browser `IdentityMap`.
- Untrusted input (fixture grammar, CDP replay scripts, MCP stdin lines,
  remote-model replies, model-text replies): `fuzz/` cargo-fuzz targets on a
  dated nightly (`scripts/fuzz-smoke.sh`, `.github/workflows/nightly.yml`),
  backed by the structured/garbage proptests at small case counts.
- Observe protocol cost: `crates/hyper-use-browser/tests/call_budget.rs`
  pins the CDP call count per observe as a regression tripwire.
- Repo rules (toolchain pin, `forbid(unsafe_code)`, crate ceilings, pinned
  nightlies): `scripts/check_repo_rules.sh`. Dependency direction and the
  no-async/no-model-SDK ban below `hyper-use-cli`:
  `scripts/architecture.txt` + `scripts/check_architecture.py`. Both run in
  `ci.yml`'s `repo-rules` job.
- Workflow integrity: `zizmor --persona=auditor` in `ci.yml`. Dependency
  stagnation: `.github/dependabot.yml`.
- Mutation testing: `.github/workflows/mutants.yml` (nightly + manual) on
  `gate.rs`, `ticket.rs`, `executor.rs`, and `world.rs` (`of_target` /
  `neighborhood_of` / `nearby`). How to run locally:
  [`docs/IMPECCABLE-AUDIT.md`](docs/IMPECCABLE-AUDIT.md#mutation-testing).

Live Chrome tests stay `#[ignore]`; CI uses CDP replay fixtures.

```
Verification impact

[x] Pure Rust deterministic behavior
[ ] Concurrency / interleaving
[ ] System model
[ ] Crash-recovery / replay
[ ] Persistence
[ ] TLA+
[ ] Proof kernel
[ ] Workflow / DSL
[ ] Unsafe / memory
[x] Property-test / fuzz surface
[ ] No verification architecture impact

Reason: <which boundary — gate / ticket / executor / world / policy / text>
Affected invariants: <from the anti-drift block>
Tests or proofs updated: <owner tests>
```

## MCP (optional adapter)

`hyper-use mcp` is a newline-delimited JSON-RPC server: observe, guard,
verify (locate / inspect / diff remain as deprecated helpers; `act` is a
deprecated alias of guard that never clicks). It owns a 16-entry snapshot
ring and up to four live CDP sessions; a failed live call drops its session,
no retry. Ranked guard keeps the `0.55` / `0.05` float gate for the historical
A5 / A6 bench arms, runs `gate::check` before Allow, and issues `of_target`
tickets like the agent. MCP never bypasses tickets and is never required to
run the agent.

## Signals

Signals (`no-op`, `loop-detected`, `repeated_query`) are data for the MCP host
journal. They do not retry and do not select an executor.
