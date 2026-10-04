# hyper-use

The product, crates, and binary are `hyper-use`. HGRA is the name of one matcher. A crate or binary named `hgra` is a bug.

hyper-use is not an agent. Operations are observe, locate, inspect, act, diff, verify. No navigate.

The product default matcher is `WeightedMatcher`. `HgraMatcher` is selectable. Do not claim one won without a benchmark.

Phase 2 speaks CDP through one transport trait. Replay fixtures and a live websocket share that trait. macOS and CUA stay unimplemented. A low-confidence act does not call CUA.

Public API is 0.1 and unstable until 1.0. Toolchain pin: Rust 1.99.0. `publish = false`.

## Verification

Deterministic ranking and the bipolar algebra are owned by unit tests and `proptest`. Do not add a second model of `locate_with` or of `WeightedMatcher::rank`.

`write_fixture` / `parse_fixture` own the manifold fixture grammar. CDP replay parsing is a different grammar. `structural_similarity` is a different metric, not a second ranker. Fusion is the only DOM/accessibility merge.

Region identity across a move, an enabled change, and a press is owned by the observe id-diff test. It is not a second identity service.

Miri, Loom, Kani, TLA+, and Lean are not justified: there is no `unsafe`, no atomics, no threads, and no recovery protocol. `#![forbid(unsafe_code)]` is on every crate. The CDP client is blocking and single-threaded. A websocket read is not a concurrent protocol.

Fuzz of CDP JSON is USEFUL later. A 16-case proptest that garbage scripts do not panic is the owner for now. Fixtures are local; a live socket is Chrome on loopback.

> Any change to observable semantics names the verification boundary it affects.

- Concurrency, interleaving, scheduling, retry, cancellation, recovery, ownership, or liveness updates the system model, or the change states why that model is unaffected.
- Executable Rust behavior updates the Rust verification layer. A theorem-owned kernel updates its proof. Workflow or DSL semantics update conformance or differential tests.
- Do not clone one state machine across Rust, TLA+, Lean, and a DSL for symmetry. Passing independent suites does not establish equivalence.

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

Reason: Phase 2 adds weighted ranking, CDP parse/fusion/press/verify, and the JEV task types. Replay is a fixture, not crash recovery. The websocket client is one blocking call stream, so no system model was added.
Affected invariants: default locate is weighted; HGRA remains selectable; fusion merges a 1px DOM/AX pair and refuses different labels; press prefers a DOM click; verify fails with ExpectedTextMissing; confidence below 550 millis does not click; a region id survives move, enabled change, and press.
Tests or proofs updated: resonance matcher tests, browser fusion and session tests, executor confidence test, CLI command tests, observe identity test, protocol contract test. No second formal model.
```

## MCP

`hyper-use mcp` is a newline-delimited JSON-RPC server. Tools are observe, locate, inspect, act, diff, verify. No navigate. The ranker crates do not depend on `hyper-use-mcp`.

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

Reason: MCP dispatch is one blocking stdin reader. A notification has no reply. That is not a concurrent protocol, so no system model was added.
Affected invariants: tool names are the six verbs; a goal or coordinate argument cannot succeed; locate defaults to weighted and sets benchmark false; a scored act below 550 millis returns executed false and does not press; verify failures are exact ToolError variants.
Tests or proofs updated: hyper-use-mcp server tests, a 16-case proptest that random lines do not panic, and a stdio subprocess test of the hyper-use binary. No second formal model.
```

