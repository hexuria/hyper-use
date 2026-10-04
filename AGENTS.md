# hyper-use

The product, crates, and binary are `hyper-use`. HGRA is the name of the technique in docs only. A crate or binary named `hgra` is a bug.

Phase 1 ranks a static manifold. It does not drive a browser, macOS, or a pointer.

Public API is 0.1 and unstable until 1.0. Toolchain pin: Rust 1.99.0.

## Verification

Deterministic ranking and the bipolar algebra are owned by unit tests and `proptest`. Do not add a second model of `locate_with` in TLA+, Lean, or a fixture interpreter.

`write_fixture` / `parse_fixture` own the fixture grammar. `structural_similarity` is a different metric, not a second ranker.

Miri, Loom, Kani, TLA+, and Lean are not justified: there is no `unsafe`, no atomics, no threads, and no recovery protocol. `#![forbid(unsafe_code)]` is on every crate.

> Any change to observable semantics names the verification boundary it affects.

- Concurrency, interleaving, scheduling, retry, cancellation, recovery, ownership, or liveness updates the system model, or the change states why that model is unaffected.
- Executable Rust behavior updates the Rust verification layer. A theorem-owned kernel updates its proof. Workflow or DSL semantics update conformance or differential tests.
- Do not clone one state machine across Rust, TLA+, Lean, and a DSL for symmetry. Passing independent suites does not establish equivalence.

```
Verification impact

[ ] Pure Rust deterministic behavior
[ ] Concurrency / interleaving
[ ] System model
[ ] Crash-recovery / replay
[ ] Persistence
[ ] TLA+
[ ] Proof kernel
[ ] Workflow / DSL
[ ] Unsafe / memory
[ ] Property-test / fuzz surface
[ ] No verification architecture impact

Reason:
Affected invariants:
Tests or proofs updated:
```
