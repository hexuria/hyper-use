# Impeccable audit

Current note, 2026-10-05 (Asia/Manila). Baseline reviewed: `9baa21e`
(179 passed, 1 ignored). This note covers the fixes on
`gol/serene-cray-7dwros` after that baseline. It is not a proof.

The phase-1 snapshot that used to live in this file is historical and is not
reproduced. `#![forbid(unsafe_code)]` stays. Rust is pinned to 1.99.0 and
every crate is `publish = false`.

## Failure class and owner

| Failure class | Owner | Status |
| --- | --- | --- |
| Gate rounding (0.5496 clicked as 550 millis) | executor unit tests, 64-case gate proptest | fixed: raw `f64` compare |
| Caller confidence outside `[0, 1]` | MCP exact-error test, tools/call proptest | fixed: `ConfidenceOutOfRange` |
| Stale `before` after `observe_after: false` | browser session test, MCP stale test | fixed: `fresh_manifold()` |
| History failure read as empty URL (false delta / false NoEffect) | `page_delta` and `verify_delta` unit tests | fixed: `Option` page state |
| Two AX-only nodes on one backend id fail observe | fusion unit test | fixed: `ax{id}-{k}` |
| Error variants with no exact assertion | exact-error tests per crate | covered, except two unreachable |
| Malformed CDP / JSON-RPC input panics | 16-case garbage proptests | unchanged |
| Structurally valid but random CDP / tools/call | 16- and 32-case structured proptests | added |
| Dependency advisories | `cargo deny check advisories` CI job | added |
| Optional `jev` feature rot | `cargo check --features jev` CI step | added |
| Concurrency, crash recovery, unsafe | none needed | not applicable (one thread, no unsafe, no recovery) |

## What was run

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` before every commit.
- `cargo check -p hyper-use-cli --features jev`: ok.
- `cargo deny check advisories` (cargo-deny 0.20.2, all features): `advisories ok`.
- cargo-mutants 27.1.0 on `observe/src/history.rs`, `observe/src/identity.rs`,
  and `executor/src/lib.rs` (`--timeout 120 --jobs 2`, output outside the tree).

## Mutants, before and after

| Run | Mutants | Caught | Missed | Unviable | Timeout |
| --- | --- | --- | --- | --- | --- |
| Before (at `9baa21e`) | 181 | 111 | 19 | 49 | 2 |
| After | 143 | 97 | 2 | 44 | 0 |

The count dropped because the dead `signature_jaccard` (11 of the 19 misses)
was deleted. The two remaining misses are equivalent mutants, not gaps:

- `executor/src/lib.rs` `margin < MIN_ACT_MARGIN - MARGIN_EPSILON` to `<=`:
  with `top >= 0.55`, `top - runner_up` is a multiple of an ulp near 1e-16 and
  cannot equal `0.05 - 1e-9` exactly.
- `select_act_executor` filter `&&` to `||`: `DEFAULT_POLICY_ORDER` already
  omits Browser Use and CUA, so the filter is defensive.

The "after" row is the full run on the final tests, with the last two kills
confirmed by a targeted re-run of `gate_ranked_confidence`.

## Unreachable error variants

- `CompareError::TransportCalledBelowThreshold`: the gate runs before press,
  so no test can reach it without changing the product.
- `ToolError::Ranker`: `WeightedMatcher` never errors and HGRA dimensions are
  validated before ranking.

These have no exact test. A fake test would not prove anything.

## Deliberately skipped

- Miri, sanitizers, Loom, Kani, TLA+, Lean: no `unsafe` and no concurrent
  core.
- cargo-fuzz: not a CI job. The structured proptests and the garbage
  proptests own parse and tools/call shapes at small case counts.
- cargo-public-api and valgrind: not installed on the audit machine; not
  installed for this audit.
- cargo-semver-checks: not justified until a crate is published.
- `iai-callgrind` and wall-clock regression gates: the 2000-region test is a
  smoke check, not a benchmark of Browser Use or CUA.
- Live Chrome stays `#[ignore]`. Browser Use and CUA stay replay fixtures.
- Region storage stays array-of-structs. macOS accessibility stays
  `NotImplemented`.

## Known limits

See "Known limits, not fixed" in `docs/DECISIONS.md`: identity map growth,
no loopback check, no CDP connect timeout, O(n*m) `match_regions`, and the
identity step that does not check role on a reused backend id.
