# hyper-use

The product, crates, and binary are `hyper-use`. HGRA is an experimental
matcher under `experiments/hgra/`. A crate or binary named `hgra` on the
product path is a bug.

**Product:** Hyper-Use is an independent action-verification layer for browser
agents. It resolves what an agent is about to interact with, refuses ambiguous
or unsafe actions, and verifies the resulting state change.

It is not an agent. It is not a Browser Use or CUA replacement. Public
operations are **observe**, **guard**, **verify**. No navigate. No click on the
product path. Browser Use (or another host) performs the trusted action after
`GuardDecision::Allow`.

The product default matcher is `WeightedMatcher`. HGRA is feature-gated /
quarantined and has not been shown to beat WeightedMatcher.

Public API is 0.1 and unstable until 1.0. Toolchain pin: Rust 1.99.0.
`publish = false`. `#![forbid(unsafe_code)]` on every crate.

## Crates (product graph)

Keep: `hyper-use-core`, `hyper-use-browser`, `hyper-use-observe`,
`hyper-use-geometry`, `hyper-use-resonance` (WeightedMatcher), `hyper-use-guard`,
`hyper-use-protocol` (guard / verify messages), `hyper-use-mcp`, `hyper-use-cli`.

Removed from the product graph: `hyper-use-browser-use`, `hyper-use-cua`,
`hyper-use-macos`, `hyper-use-executor`. HGRA algebra lives under
`experiments/hgra/`, not the default workspace build.

## Verification

Deterministic ranking and the bipolar algebra (when the hgra feature is on)
are owned by unit tests and `proptest`. Do not add a second model of
`WeightedMatcher::rank`.

`write_fixture` / `parse_fixture` own the manifold fixture grammar. CDP replay
parsing is a different grammar. Fusion is the only DOM/accessibility merge.

Diff semantics on a given id are owned by the observe id-diff test. Region
identity across observations is owned by the browser session's `IdentityMap`.

Miri, Loom, Kani, TLA+, and Lean are not justified: there is no `unsafe`, no
atomics, no threads, and no recovery protocol. The CDP client is blocking and
single-threaded.

Fuzz of CDP JSON is USEFUL later. A 16-case proptest that garbage scripts do
not panic is the owner for now. Fixtures are local; a live socket is Chrome on
loopback.

> Any change to observable semantics names the verification boundary it affects.

- Concurrency, interleaving, scheduling, retry, cancellation, recovery,
  ownership, or liveness updates the system model, or the change states why
  that model is unaffected.
- Executable Rust behavior updates the Rust verification layer.
- Do not clone one state machine across Rust, TLA+, Lean, and a DSL for symmetry.

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

Reason: action-firewall pivot removes executor routing and public actuation.
GuardDecision is pure over observe + query (+ optional proposed target).
Verify remains observe/diff/expectation. No system model added.
Affected invariants: product tools are observe/guard/verify; WeightedMatcher
default; text-miss cap below allow threshold; ranked margin refuse; no MCP
click; HGRA not in default graph.
Tests or proofs updated: guard unit tests, MCP guard tests, protocol tests.
No second formal model.
```

## MCP

`hyper-use mcp` is a newline-delimited JSON-RPC server. Product tools:
observe, guard, verify. Locate / inspect / diff may remain during transition
as deprecated helpers. The ranker crates do not depend on `hyper-use-mcp`.

The server owns a 16-entry in-memory snapshot ring and up to four live CDP
sessions. A failed live call drops its session; there is no retry or reconnect.
Guard never clicks. Verify never clicks.

## Signals

Signals (`no-op`, `loop-detected`, `repeated_query`) are data for the host
journal. They do not retry and do not select an executor.

## Act / press (going away)

`BrowserSession::press` and any MCP/CLI `act` that performs a CDP click are
removed from the product path. Transitional fixture tests may still exercise
low-level CDP click helpers until deleted. New code must return
`GuardDecision` instead of clicking.
