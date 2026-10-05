# hyper-use

Hyper-Use is a **Rust-native browser-agent runtime**: it observes the page,
builds a finite action space, decides (PUA first, optional escalation),
guards with hard integrity checks, executes only through a one-shot
**ActionTicket**, then re-observes, diffs, and verifies.

It is **not** a voluntary MCP preflight the model must remember to call.
MCP remains an optional adapter. The primary product is a library `Agent`
that owns the loop.

HGRA is one experimental matcher under `experiments/hgra/`. It is **frozen**
and not on the product path. A crate or binary named `hgra` in the product
graph is a bug.

> **Pivot (2026-10-05).** This PRD supersedes the earlier framing that
> Hyper-Use is "not an agent" and "does not click". See
> [`docs/adr/0001-agent-runtime-pivot.md`](adr/0001-agent-runtime-pivot.md).
> ActionTicket + host interceptor (B0/B1) remain a valid ablation until the
> agent fully owns execution.

## Primary API (agent runtime)

```rust
use hyper_use_agent::AgentBuilder;
use hyper_use_policy::PuaPolicy;

let mut agent = AgentBuilder::new(browser, PuaPolicy::default())
    .max_steps(60)
    .build("Click Sign in");
let outcome = agent.run();
```

PUA decides among finite ActionSpace candidates. Guard + ActionTicket bind
execution. MCP is an optional adapter, not required to run the loop.

## Product loop

```text
goal
  ↓
Hyper-Use Agent
  ↓
observe → ActionSpace
  ↓
policy (PUA → optional escalation)
  ↓
guard (hard gates) → ActionTicket
  ↓
executor revalidate + consume + execute
  ↓
observe → diff → verify → history
  ↓
next turn (or DONE / BLOCKED)
```

## Boundaries

| Layer | Owns |
|---|---|
| **PUA** | HOW a finite choice is made (scores, threshold/margin, abstain) |
| **Hyper-Use policy** | WHAT browser evidence each candidate gets; ActionSpace construction |
| **Hyper-Use guard** | Physical/logical executability; ticket issue |
| **Hyper-Use executor** | Exact ticketed action or nothing |
| **TextResolver** | Arbitrary `TYPE_TEXT` strings (not PUA) |
| **Host / MCP** | Optional adapter; must not bypass tickets |

## What Hyper-Use deliberately does not do

- Depend on JEV / TypeSafe in the core path (optional escalation feature only).
- Ship HGRA as the default matcher / policy.
- Accept model-generated CSS selectors or JavaScript for execution.
- Silently turn PUA abstention into "top candidate wins".
- Treat page content as trusted instructions.

## ActionTicket (enforcement boundary)

`Allow` issues an `ActionTicket` (ticket id, snapshot id, world / target
fingerprints, target id, action). The executor **must** revalidate against a
fresh observation, then press the **exact** ticket target (or refuse stale /
world-changed). Tickets are one-shot. Verify should bind ticket + before/after
snapshots (`verify_delta`). See `bench/arms/B01.md` for the interceptor ablation.

## Matchers / policy (HGRA frozen)

- Interim locate/guard ranking: `WeightedMatcher`.
- Target policy: **PUA** (`hexuria/pua`, pin by rev) once Phase 2 lands.
- **HGRA is frozen** under `experiments/hgra/`. No matcher PRs during the pivot.

## Acceptance bar

- All executed targets originate from the current observed ActionSpace.
- Hidden / disabled / covered / background-modal / stale-ticket targets never execute.
- PUA abstention is preserved; escalation is explicit.
- Normal tasks: low false-refusal. Adversarial: refuse, never wrong-action.
- No-effect and wrong postcondition: detect on verify.
- System runs without MCP and without Jev when the task fits PUA + TextResolver.

## Historical notes

`RESULTS.md` A1–A8 tables are **historical** (pre-ticket, planner variables
changed with Hyper-Use). Rewrite only from pinned agent A/B/C/D or B0/B1/B2
runs. B0/B1 remain the interceptor ablation until the agent owns the loop.
