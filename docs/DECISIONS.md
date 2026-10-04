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

No crates.io dependencies. No `unsafe`. No LLM, CDP, or macOS bindings.
Fixture syntax is a small line format so serde is not required.
JSON output from the CLI is written by hand.

## Verifiers skipped

Deterministic ranking, encoding, penalties, and diffs are owned by unit tests
and a fixed-corpus property test (encoder stability, commutative binding,
self-similarity). That is the owner for this failure class.

- Miri: not run. The workspace forbids `unsafe` (`#![forbid(unsafe_code)]`).
  Miri would not check a different risk.
- Loom: not run. There are no threads, locks, or atomics.
- Kani, Lean, TLA+: not justified. There is no concurrent protocol, no crash
  recovery, and no kernel whose theorem would say more than the tests.
- Fuzz: not run. The fixture parser is small and covered by error-path tests.
  A later untrusted-input phase should add a fuzzer. That absence is accepted
  for Phase 1 because fixtures are local files the operator wrote.
- Benchmarks: not run. Phase 1 is a correctness surface on snapshots of tens
  of regions, not a hot loop.
- cargo-vet / semver-checks: no third-party crate and nothing is published.

Anti-drift: any change to ranking, penalties, geometry thresholds, or the
encoder version updates the Rust tests that pin them. Do not add a second
model of the same score without a conformance fixture.
