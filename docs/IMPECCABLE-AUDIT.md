# Impeccable audit

Current note, 2026-10-05 (Asia/Manila). Baseline reviewed: `59d724b`.
This note includes the exact-error tests and the MCP locate/inspect envelope
change made on that baseline. It is not a proof.

The phase-1 snapshot that used to live in this file is historical and is not
reproduced. At `59d724b` it was already false: it claimed no crates.io
packages in the graph, no rustfmt CI, no network, and status stubs only.
The tree has CDP replay, Browser Use replay, and CUA replay, `tungstenite` in
the default graph, rustfmt in CI, and `publish = false` with Rust pinned to
1.99.0. `#![forbid(unsafe_code)]` stays.

## What is tested

Unit tests own the paths that can fail in-process:

- CDP replay: observe, DOM semantic press, protocol-error fallthrough to
  `DOM.focus`, verify text, and the exact errors `NotObserved`,
  `BadViewport`, `MissingObjectId`, `BadJson`, `ParamsMismatch`, and
  `Transport` (a `wss://` or non-http endpoint, no socket).
- CLI argv: unknown and duplicate flags, bad dims, unknown matcher, missing
  region, missing verb, non-finite confidence, region still present, and a
  CDP file handed to `browser-use` or `cua` (`BrowserUseScript`, `CuaScript`).
  `CliError::MissingCommand` was unused and was removed.
- MCP JSON-RPC: the six verbs, act receipts, and the 0.55 refusal.
  `locate` and `inspect` omit `executed` and `verified`. `act` and `verify`
  still set them. A locate does not press and is not a `ComputerResult`.
- Opt-in replay handoffs send region id, role, label, and action. A score
  below 0.55 does not submit. macOS is `NotImplemented`. `CuaStub` pixel
  actuation is still unimplemented. Neither handoff is a benchmark.

Property tests (proptest, not a proof):

- `hyper-use-hyper` tests/algebra.rs, 32 cases: encoder determinism and
  maximal self-similarity; random symbols do not panic; a symbol of length
  0..=8 is `EmptySymbol` when either side is empty, otherwise cosine of the
  vector with itself is 1.
- `hyper-use-resonance` tests/rank_props.rs, 16 cases: equal scores follow
  region id under insertion shuffle; one penalty strictly lowers that region.
- `hyper-use-browser`, 16 cases: garbage CDP scripts do not panic.
- `hyper-use-browser-use` and `hyper-use-cua`, 16 cases each: generated
  labels keep the semantic wire keys.
- `hyper-use-executor`, 16 cases each for Browser Use and CUA: integer millis
  in `0..550` do not submit.
- `hyper-use-mcp` tests/chaos.rs, 16 cases: random JSON-RPC lines do not panic.

## Deliberately skipped

- Miri, sanitizers, Loom, Kani, TLA+, Lean: no `unsafe` and no concurrent
  core. Not run.
- cargo-fuzz: not added. Fixtures and replay scripts are local.
- cargo-vet / cargo-deny / RUSTSEC: not a CI job. Useful later because
  `tungstenite` is in the default graph and the optional `jev` feature locks
  an HTTP stack that default `cargo test` does not compile.
- `iai-callgrind` and wall-clock regression gates: not added. The 2000-region
  test is a smoke check, not a benchmark of Browser Use or CUA.
- Live Chrome stays `#[ignore]`. Browser Use and CUA stay replay fixtures.
- Region storage stays array-of-structs (`BTreeMap`). No struct-of-arrays.
- The 0.55 gate, the default weighted matcher, and the default browser
  executor policy are unchanged.
- macOS accessibility is not implemented.
