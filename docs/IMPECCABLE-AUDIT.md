# Impeccable audit — hyper-use

Audit date: 2026-10-05 (Asia/Manila). Tree: `/workspace/hyper-use`. Toolchain pin: `rust-toolchain.toml` channel `1.99.0`. Product name is `hyper-use`. "HGRA" is the technique name in docs only.

This file is the read-only audit. The validated plan below is what implementation was allowed to change. Items marked drop were already true in the tree before this pass.

## Validated plan

Confirmed against the source before production edits. Quotes are the pre-change text.

### Drop — already satisfied

- **Allowed widths are not a bare `usize`.** `crates/hyper-use-hyper/src/lib.rs`: `pub enum Dims { D512 = 512, D1024 = 1024, D2048 = 2048, D4096 = 4096 }` and `try_from_usize` returns `HyperError::UnsupportedDims`. A width of 7 cannot be constructed.
- **`RegionId` rejects empty and whitespace.** `crates/hyper-use-core/src/id.rs`: empty returns `CoreError::EmptyId`; whitespace or control returns `CoreError::InvalidId`. Colliding ids return `CoreError::DuplicateRegion` from `InteractionManifold::try_new` (`manifold.rs`).
- **Hot records are already private.** `InteractionRegion`, `Rect`, `Point`, `BipolarVector`, `ResonanceScore`, and `ResonanceModel` keep fields private and expose accessors. `LocateQuery::text_ref` already returns `Option<&str>` via `as_deref`.
- **`RegionFlags` stays a set of independent bools.** `vocab.rs` documents that disabled and offscreen may both be set. That is the first state-machine rung. Do not collapse penalties into one enum.
- **No `unsafe`, no atomics, no threads, no FFI.** Every crate has `#![forbid(unsafe_code)]`. Search found no `todo!`, `unimplemented!`, `Atomic`, or foreign fn. `spawn` appears only as a test process for the `hyper-use` binary.
- **Stubs do not panic.** `StubExecutor::execute` returns `ExecutorError::NotImplemented`. Browser, macOS, and CUA crates return a `STATUS` string. No silent `todo!()`.
- **Ranking has one implementation.** `locate` / `locate_with` in `hyper-use-resonance`. The CLI calls `locate_with`. `structural_similarity` in `hyper-use-observe` is Jaccard plus center distance, not a second copy of the resonance score.
- **Many error paths already assert the exact variant**, including `EmptyId`, `EmptySymbol`, `EmptyBundle`, `NonFiniteWeight`, `DuplicateRegion`, `FixtureError::MissingViewport`, `ExecutorError::NotImplemented`, and `CliError::MissingFixture`.

### Execute — real gaps

- **Property tests are a fixed corpus, not proptest.** `hyper-use-hyper/src/lib.rs` `property_binding_commutes_across_a_fixed_corpus` loops a hand-written list and never builds `Dims::D4096`. `Cargo.lock` has no crates.io packages. Add `proptest` as a dev-dependency only.
- **Two assertions only check `is_err()`.** `hyper-use-hyper/src/lib.rs` around the bad-component test (`try_from_bipolar(vec![2; 512]).is_err()`) and `cosine(&a, &b).is_err()`.
- **No fallible constructor rejects a weight table that does not sum to 1.** `ResonanceModel` is a private-field const `V1` whose basis points sum to 100, guarded only by a runtime test. A future table could skip that check. Add an integer basis-point gate (exact sum 100, epsilon 0) and a const assert on `V1`.
- **The fixture parser has no writer and no roundtrip.** `parse_fixture` is the only codec. A second parser would duplicate the grammar. The owner is a production writer plus parse roundtrip, plus exact `FixtureError` variants the suite does not yet name (`DuplicateViewport`, unclosed quote).
- **Error litmus has not been run.** Exact `Err` tests exist, but nobody has temporarily skipped a `return Err` and watched the suite fail.
- **No 2000-region outcome check.** `docs/DECISIONS.md` says benchmarks were not run. Add one closed-load smoke test: correct top-1 plus a loose completion ceiling. Not a wall-clock regression gate.
- **CI does not check rustfmt.** `.github/workflows/ci.yml` runs `cargo test` and clippy only.
- **No `AGENTS.md` anti-drift paragraph.**

### Explicitly not justified (do not add)

Confirmed: no `unsafe`, no atomics, no locks, no channels, no `spawn` in library code, no FFI, no crash journal, no multi-actor protocol.

- **Miri** — nothing for it to run. `forbid(unsafe_code)` is the owner.
- **Sanitizers (ASan/TSan)** — no threading and no unsafe memory access.
- **Loom** — no concurrent implementation.
- **Kani** — no unsafe and no kernel whose inputs tests cannot close.
- **TLA+** — no interleaving, retry, crash, or liveness property.
- **Lean / Verus** — bipolar bind, cosine, and ranking are deterministic functions. A theorem would restate the same steps the property tests own. That is a second model of the ranker. Do not add one.
- **cargo-semver-checks** — `publish = false`, version `0.1.0`. Nothing is published.
- **cargo-vet / cargo-deny install** — `cargo-deny` is not on `PATH` (`~/.cargo/bin` has no `cargo-deny`). Closure before this pass was std only. After this pass the only new crate is `proptest` as a dev-dependency. Do not install a tool pile. cargo-deny/RUSTSEC is USEFUL and skipped this pass: no network-facing crate and no unsafe crate.
- **Arena / SoA rewrite** — not justified. Each rank reads the whole region record, and N is small (target 2000). AoS stays. No `size_of` assertion: the public enums (`Role`, `Action`, `HyperError`, `Request`) are not sized by one rare fat variant worth boxing.
- **Instruction-count benchmarks (`iai-callgrind`)** — DEFERRED. Valgrind is not assumed on this shared box, and wall-clock here is not a regression gate.

## Architecture map

| Crate | Role |
| --- | --- |
| `hyper-use-hyper` | Deterministic bipolar encoder, bind, bundle, permute, cosine |
| `hyper-use-core` | Validated ids, rects, regions, manifold, fixture codec, locate query |
| `hyper-use-geometry` | Viewport normalization, zones, spatial relations |
| `hyper-use-resonance` | The only ranker (`locate_with`) and `ResonanceModel::V1` |
| `hyper-use-observe` | Id diff and a separate structural similarity. Not the ranker |
| `hyper-use-protocol` | Loop phase names. Does not run the loop |
| `hyper-use-mcp` | Tool name constants |
| `hyper-use-executor` | Policy order. Built-in backends return `NotImplemented` |
| `hyper-use-browser`, `hyper-use-macos`, `hyper-use-cua` | Status stubs |
| `hyper-use-cli` | `hyper-use locate` over a fixture file |

Data flow: fixture or in-memory regions → `InteractionManifold` (id-ordered `BTreeMap`) → per-region bipolar signature → weighted score minus penalties → sort by total, then `RegionId`.

## Risk map

| Failure class | Present? | Owner |
| --- | --- | --- |
| Deterministic ranking and tie-break | Yes | Unit tests + proptest in `hyper-use-resonance`. Not a second formal model |
| Hypervector algebra (determinism, commute, inverse, self-similarity, tie +1) | Yes | Unit tests + proptest in `hyper-use-hyper` |
| Fixture grammar | Yes | `parse_fixture` / `write_fixture` roundtrip and exact `FixtureError` tests |
| Untrusted input at a network boundary | No | Fuzz is OPTIONAL until a live observer accepts foreign bytes. Fixtures are local operator files |
| Unsafe / provenance | No | `forbid(unsafe_code)`. Miri NOT JUSTIFIED |
| Small concurrent implementation | No | Loom NOT JUSTIFIED |
| System interleavings, deadlock, liveness, recovery | No | TLA+ NOT JUSTIFIED |
| Crash persistence | No | No owner required |
| Mathematical kernel beyond tests | No | Lean/Kani NOT JUSTIFIED. Tests close inverse, commute, self-max, penalty drop, id tie-break |
| Several frontends | One fixture syntax | Roundtrip is the conformance link. Do not add a second parser |

`structural_similarity` is a necessary different metric (rename hypotheses), not accidental duplication of `locate`.

## Current verifiers (before this pass)

- `cargo test` unit and integration tests, clippy `-D warnings` in CI.
- Fixed-corpus loops named "property" but not `proptest`.
- No Miri, Loom, Kani, fuzz, cargo-fuzz, cargo-deny, cargo-vet, or semver job.
- `docs/DECISIONS.md` already records skipped verifiers and the tie-break (`0 → +1`).

## Recommendation marks

| Item | Mark |
| --- | --- |
| Exact `Err` variant on the two `is_err` asserts | REQUIRED |
| Error litmus, then restore | REQUIRED |
| proptest: dims 512/1024/2048/4096, bind inverse and commutative, self-similarity max, rank stable under insertion permutation, penalty strictly lowers, encode never panics | REQUIRED |
| Fixture writer + roundtrip (not a second parser) | REQUIRED |
| `WeightBasisPoints::try_model` rejects sum ≠ 100; const assert on `V1` | REQUIRED |
| `AGENTS.md` anti-drift + verification-impact template | REQUIRED |
| CI: fmt --check, clippy -D warnings, test, Rust 1.99.0. No Miri/Loom/Kani jobs | REQUIRED |
| 2000-region golden top-1 smoke, loose ceiling, closed load | REQUIRED |
| Document AoS, skipped tools, unstable 0.1 API, what the code cannot do | REQUIRED |
| cargo-deny/RUSTSEC | USEFUL, skipped this pass (tool absent, no unsafe or network crate) |
| Fuzz of the fixture lexer | OPTIONAL until untrusted input is a real boundary |
| cargo-semver-checks | NOT JUSTIFIED until publish |
| Miri, sanitizers, Loom, Kani, TLA+, Lean | NOT JUSTIFIED |
| Arena, SoA, `repr` changes | NOT JUSTIFIED (no profile; each rank reads the whole record; N ≤ 2000) |
| Second formal model of ranking | REMOVE / do not add |
| Wall-clock as a CI regression gate, iai-callgrind | DEFERRED |

## Conformance and anti-drift

One semantics lives in Rust.

- Ranking, penalties, geometry thresholds, and encoder version move only with the Rust tests that pin them.
- Do not clone `locate_with` into TLA+, Lean, or a fixture interpreter.
- Fixture conformance is `write_fixture` → `parse_fixture` → `PartialEq` on the manifold. The writer is not a second grammar.
- `observe`'s structural score is not required to match resonance. Do not add a differential between them.

## CI split

- Pull request: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` on 1.99.0. Property tests ride inside `cargo test`.
- Nightly: no extra job. Fuzz and iai are deferred, not scheduled.
- Release: same as pull request until a failure class in the table above appears.

## Migration order

1. Fixture roundtrip (conformance for the codec).
2. Exact error asserts, weight gate, proptest (Rust checks the risk table already names).
3. Write ownership into `AGENTS.md` and `docs/DECISIONS.md`, including the litmus result.
4. Do not strengthen a formal model.
5. Nothing to delete; do not add a second ranker.
6. CI gains fmt and a comment that Miri/Loom/Kani/cargo-deny are omitted on purpose.

## Verification impact

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

Reason: property tests and the fixture roundtrip now own algebra, ranking ties, penalties, and the fixture codec. Fuzz stays deferred. No system model and no proof kernel exist, on purpose.
Affected invariants: encoder determinism at 512/1024/2048/4096; bind commutative and self-inverse; cosine self is 1 and maximal; bundle tie 0 → +1; equal scores sort by RegionId independent of insertion order; a penalty strictly lowers that region; positive weights sum to exactly 100 basis points; fixture write/parse preserves a zero-origin manifold.
Tests or proofs updated: hyper unit tests, proptest suites, resonance weight and capacity tests, core fixture roundtrip. No proof artifact.
```

## Data layout

AoS stays. `InteractionManifold` stores `BTreeMap<RegionId, InteractionRegion>`. `locate_with` reads role, label, rect, actions, parent, sources, flags, and stability on every region. Neighbors are not a hot field subset that a profile has shown to be cache-bound. N target is 2000. An arena or struct-of-arrays rewrite is NOT JUSTIFIED until a profile says the hot path is memory-bound.

## Execution record

Filled in after the pass. See `docs/DECISIONS.md` for the litmus transcript summary and the smoke-test load model.

## Execution record (after the pass)

Done:

- `write_fixture` plus roundtrip and exact `FixtureError`s, including nonzero viewport origin.
- `WeightBasisPoints::try_model` rejects a sum other than 100. Const assert pins `V1`.
- proptest on algebra (four widths, inverse, commute, self-max, no panic) and ranking (insertion permutation, one penalty strictly lowers).
- The two `is_err()` asserts in `hyper-use-hyper` now name `InvalidComponent` and `DimMismatch`. `Dims::try_from_usize(7)` is `UnsupportedDims(7)`.
- Error litmus failed `tests::empty_symbol_and_empty_bundle_are_errors` when the empty-symbol `return Err` was skipped, then the return was restored. See `docs/DECISIONS.md`.
- 2000-region smoke test. First measurement was 99.6s because every neighbor allocated a bind. Identical `(relation, role)` vectors are now counted once. Re-measure was about 2.03s. Ceiling is 10s, not 500ms, because 500ms fails this debug build. Golden top-1 is `target`.
- CI runs fmt, clippy `-D warnings`, and `cargo test` on 1.99.0.
- `AGENTS.md` has the anti-drift paragraph and the verification-impact template.

Still deferred, on purpose: Miri, sanitizers, Loom, Kani, TLA+, Lean, fuzz, cargo-deny, cargo-vet, cargo-semver-checks, iai-callgrind, arena/SoA.
