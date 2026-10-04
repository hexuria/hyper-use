# Decisions

## Product name

The technique may be discussed as HGRA. The shipped product, crates, binary,
and skill folder are `hyper-use`. A crate named `hgra` is a bug.

## Hypervector algebra

Symbols are bipolar vectors in `{-1,+1}^d`, `d` one of 512, 1024, 2048, 4096.
The default is 2048. Bits are SplitMix64 expanded from FNV-1a of
`namespace || 0x1f || symbol || 0x1f || version || "hyper-use-hv1"`.
`std` `DefaultHasher` is not used because its keys are per-process.

Binding is element-wise multiply (commutative on bipolar values).
Bundling is a weighted sum then sign. A component that sums to exactly 0
becomes `+1`.
Permutation is a left rotation. Relation shifts are in `1..d`.
Similarity is cosine, which for these vectors is `dot / dims` in `[-1, 1]`.
Self-similarity is 1.

Region signatures bind role, label tokens, position zones, shape, actions,
a permuted parent, neighborhood relations, content state, and source bits.
Penalty flags are not mixed into the vector. They subtract after the weighted
sum so a disabled twin drops by exactly the versioned penalty (detached also
drops contextual consistency).

The hypervector term is the mean cosine of the query probes against that
signature, not a second learned model. Probe resonance keeps a single field
visible after bundling. Weights are [`ResonanceModel::V1`] basis points.

## Geometry

Normalization is `(x - viewport.x) / viewport.width` and the same for `y` and
extents. `y` grows downward. Zones split at 1/3 and 2/3. Center is the closed
middle third on both axes. Small area is `< 0.01`, medium `< 0.08`, else large.
Near means a gap of at most 0.08 viewport units. Aligned means centers within
0.05 on that axis.

## Identity versus diff

`diff` is id-based (added, removed, changed). `match_regions` pairs identical
ids first, then greedy similarity at or above 0.85. A rename is therefore both
an id-level remove+add and a similarity pair. Same label on opposite sides of
the viewport scores 0.8 and does not merge.

## Dependencies

No `unsafe`. No LLM, CDP, or macOS bindings.
The library crates depend only on each other. `proptest` 1.11 is a
dev-dependency of `hyper-use-hyper` and `hyper-use-resonance` (the lockfile
also pulls its `rand` stack). It is not a public dependency and is not linked
into the `hyper-use` binary. Fixture syntax is a small line format so serde
is not required. JSON output from the CLI is written by hand.

Crate versions are 0.1.0 and `publish = false`. The public API is unstable
until 1.0. There is no `From` for `RegionId` or for a float weight table:
both would make an illegal value easy to build.

## Verifiers skipped

Deterministic ranking, encoding, penalties, and diffs are owned by unit tests
and proptest (32 cases on the algebra, 16 on ranking): encoder stability at
512/1024/2048/4096, commutative self-inverse bind, self-similarity, id
tie-break, and penalty drops. That is the owner for this failure class.
There is no second formal model of the ranker.

- Miri: not run. The workspace forbids `unsafe` (`#![forbid(unsafe_code)]`).
  Miri would not check a different risk.
- Loom: not run. There are no threads, locks, or atomics.
- Kani, Lean, TLA+: not justified. There is no concurrent protocol, no crash
  recovery, and no kernel whose theorem would say more than the tests.
- Fuzz: not run. The fixture parser is small and covered by error-path tests.
  A later untrusted-input phase should add a fuzzer. That absence is accepted
  for Phase 1 because fixtures are local files the operator wrote.
- Benchmarks: one closed-load smoke test ranks 2000 regions on a single
  thread (`hyper-use-resonance` `tests/capacity.rs`). Success is the golden
  top-1 id `target` plus completion. The statistic is one-shot debug latency,
  not a mean or a histogram. Measured about 2.0s on this shared box after
  neighbor coalescing, and about 99.6s before it. The ceiling is 10s (5× the
  measurement) so a busy machine does not flake. It is not a regression gate.
  The PRD p95 under 20ms is not enforced. A 500ms ceiling fails this debug
  build, so it is not the CI check. `iai-callgrind` is deferred: valgrind is
  not assumed, and wall-clock on this box is not a trustworthy regression
  signal.
- cargo-vet / cargo-deny / RUSTSEC: skipped this pass. `cargo-deny` is not
  installed (`~/.cargo/bin` has no `cargo-deny`) and was not installed.
  USEFUL later. Not justified as a gate while nothing is network-facing or
  `unsafe`. `cargo-semver-checks` is NOT JUSTIFIED until a crate is published.
- Miri, sanitizers, Loom, Kani, TLA+, and Lean stay NOT JUSTIFIED. See
  `docs/IMPECCABLE-AUDIT.md`.

Anti-drift: any change to ranking, penalties, geometry thresholds, or the
encoder version updates the Rust tests that pin them. Do not add a second
model of the same score without a conformance fixture.

## What this code cannot do

- No browser, no macOS accessibility, no computer-use clicks, no MCP transport.
  Those crates return a status string or `ExecutorError::NotImplemented`.
  They do not panic.
- No learned embeddings and no LLM. Symbols are the fixed encoder.
- A bundle component that sums to exactly 0 becomes `+1`. There is no other
  tie-break inside the vector. Equal locate scores break by `RegionId`
  ascending, not by insertion order.
- The fixture line format cannot store a viewport origin other than `(0, 0)`.
  `write_fixture` returns `FixtureError::UnsupportedViewportOrigin` instead of
  dropping it. There is one parser. The writer is the roundtrip owner, not a
  second grammar.
- Signature bundle weights (`role` 2, `label` 3, and the rest) are relative.
  They are not a probability and must not be normalized to 1. Doing so would
  change vectors. `ResonanceModel` positive weights are a different table and
  must sum to exactly 100 basis points (epsilon 0; no float gate).

## Neighbor contributions

Identical `(relation, role)` neighbor vectors are counted and bundled once
with weight `0.25 * count`. `0.25` is dyadic, the running sum stays an exact
multiple of `0.25` at the sizes we rank, and order does not change the sign.
A unit test checks that two `0.25` parts match one `0.5`, in either order.
If `NEIGHBOR_WEIGHT` stops being a multiple of `1/4`, delete the collapse.
The region store stays an array of structs (`BTreeMap` of whole records).
Each rank reads the whole record and N is aimed at 2000. No arena and no
struct-of-arrays until a profile says the hot path is memory-bound.

## Error litmus

On 2026-10-05 the empty-symbol return in `Encoder::encode` was temporarily
skipped (`let _skipped = HyperError::EmptySymbol` instead of `return Err`).
`cargo test -p hyper-use-hyper --lib -- --exact tests::empty_symbol_and_empty_bundle_are_errors`
failed:

```
assertion `left == right` failed
  left: Ok(BipolarVector { dims: 512, head: [1, -1, 1, -1, -1, -1, -1, 1] })
 right: Err(EmptySymbol)
```

The return was restored. The test asserts `Err(HyperError::EmptySymbol)`, not
`is_err()`.
