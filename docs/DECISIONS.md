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

No `unsafe`. No LLM. No macOS bindings. No browser framework.

`proptest` 1.11 is a dev-dependency of `hyper-use-hyper`, `hyper-use-resonance`,
and `hyper-use-browser`. It is not linked into the `hyper-use` binary.
Manifold fixtures stay a small line format. CLI JSON is still written by hand.

Phase 2 adds two crates, only on `hyper-use-browser`, not on the ranker:

- `serde_json` 1. Parses CDP result JSON. The public transport trait takes
  and returns JSON strings, so `serde_json::Value` is not part of the public
  signature. Hand-rolling a JSON parser was discarded: CDP documents are real
  JSON, and a private parser would be a second grammar with worse errors.
- `tungstenite` 0.26, default features off, `handshake` on. Blocking
  `ws://` client. `wss://` is refused so a TLS stack is not pulled in.
  A from-scratch websocket was discarded: masking and the upgrade handshake
  are easy to get wrong, and this crate is not the product. Chromium,
  headless_chrome, and fantoccini were discarded as too large.

The ranker crates still do not depend on either.

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

- No macOS accessibility. macOS returns `ExecutorError::NotImplemented`.
  A browser stub with no session does too. `BrowserExecutor` does not.
  `BrowserUseExecutor` and `CuaExecutor` are each a replay of one semantic act,
  not a live agent and not a fusion benchmark. The pixel CUA driver is still
  `CuaStub`. MCP stdio exists. It is not a second browser and it does not navigate.
- No navigation and no JEV runtime. `ComputerTask` is one locate or one act.
- The weighted matcher and HGRA are not calibrated to each other. A confidence
  of 0.55 is the act gate for a scored total. It is not a probability.
- Live CDP does not launch Chrome. `wss://` is refused. Occlusion is not
  detected. Unlabeled generic DOM containers are not regions unless they are
  a known control tag or carry a label or an explicit role.
- Fusion can merge two same-label controls whose centers are within 8px even
  when IoU is low. A duplicate accessibility node for one DOM node is left
  as its own region.
- CDP snapshots store `captured_at_ms = 0`. The ranker does not read a clock.
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

## Phase 2 browser and matchers

The product default locate path is `WeightedMatcher` (semantic 50, geometric
30, actionability 20, basis points, sum 100). Text and role combine by
minimum so a role hit cannot hide a text miss. Penalties are
`ResonanceModel::V1`'s penalty table. The hypervector weight is not applied.
`HgraMatcher` calls `locate_with` and nothing else. `locate` / `locate_with`
remain the HGRA entry points so existing penalty tests keep their meaning.
The CLI default is weighted. `--matcher hgra` selects the other. No benchmark
says which is better. Do not add a third score that averages them.

Act confidence gate: scored totals below 550 millis (0.55) return
`ExecutorError::ConfidenceBelowThreshold` and do not touch the transport.
`ActConfidence::Inspected` is the operator naming a region id. Those two
states are an enum, not a bool plus an optional score. The refusal is also
representable as `ComputerResult::refused` with `executed = false`. That
value is a journal record, not a second executor. CUA is not invoked.

CDP methods, in order, for one observation: `Page.getLayoutMetrics`,
`DOM.getDocument` depth -1, `Accessibility.getFullAXTree`, then
`DOM.getBoxModel` per DOM node id, then `DOM.getBoxModel` per accessibility
backend id. Press, when a DOM node id exists: `DOM.resolveNode` then
`Runtime.callFunctionOn` of `function(){this.click()}`. A CDP error on that
call falls through to `DOM.focus`, then to `Input.dispatchMouseEvent`.
A missing script entry is `CdpError::NoScriptedResponse` and does not fall
through, so a short fixture cannot become a silent coordinate click.
`press` in the CLI is `Action::Click`. Other actions return
`BrowserError::UnsupportedAction`.

Fusion v1: label Jaccard >= 0.5 or either label empty; roles equal or either
generic; IoU >= 0.5 or centroid distance <= 8px. Greedy, highest IoU. The
stored rect is the DOM rect. The id is `n{backendNodeId}`.

`ComputerTask` has constructors `locate` and `act` only. Constraints and
expected outcome are data. There is no planner.

Verifiers for this phase: unit tests with exact `Err` variants
(`ExpectedTextMissing`, `RegionStillPresent`, `ConfidenceBelowThreshold`,
`WeightsDoNotSum`, `NoScriptedResponse`). proptest (16 cases) owns "garbage
CDP scripts do not panic". No Miri, Loom, Kani, TLA+, or Lean. The websocket
is one blocking reader. That is not a concurrent protocol. No second model
of fusion or of either ranker.

Live Chrome: a read-only `Browser.getVersion` may be run against an
already-open debugging port. Clicks in tests use the replay transport.

## MCP stdio

The server is `hyper-use mcp`. Tool names are exactly `observe`, `locate`,
`inspect`, `act`, `diff`, and `verify`. The product name is the server name,
not a prefix on each tool. A prefixed name would be a second vocabulary.
`navigate` is not a tool. Arguments named `goal`, `steps`, or `navigate` return
`GoalNotAccepted`. Arguments named `x`, `y`, or `coordinates` return
`CoordinatesNotAccepted`. Those states are not representable as a successful
click.

Transport is newline-delimited JSON-RPC 2.0 on stdin and stdout. A notification
(no `id`) gets no response. Batches are `InvalidRequest`. There is no
`Content-Length` framing, no resources, no prompts, and no sampling. The
official `rmcp` stack was discarded: it pulls an async runtime and a protocol
surface this host does not call. A blocking read loop is the whole transport.
Downside: clients that only speak the older header framing cannot connect.
Accepted, because the MCP stdio spec delimits messages with newlines.

`serde_json` 1 is a dependency of `hyper-use-mcp` only, plus the browser crate
that already had it. It parses JSON-RPC. `serde_json::Value` is not a type in
the ranker crates. `hyper-use-core`, `hyper-use-hyper`, `hyper-use-geometry`,
and `hyper-use-resonance` do not depend on this crate. No other dependency was
added. The lockfile already contained `serde_json`.

`locate` defaults to `WeightedMatcher`. `matcher: "hgra"` selects
`HgraMatcher`. Every locate result sets `benchmark` to false. That flag is not
a measurement. Do not read it as a win.

`act` calls `BrowserExecutor`. DOM semantic click stays ahead of coordinates.
A scored confidence below 550 millis returns a tool result with
`executed: false`, `fallback: "low-confidence"`, and no mechanism. It does not
return a JSON-RPC error and it does not press. The proof is the
`sign-in.cdp.json` fixture, which has no press responses: a click would be
`Browser`, and the test expects `executed: false`. An inspected act (no
confidence) still presses. `act` against a manifold fixture is
`ActNeedsCdp`, because that file has no DOM node. The result does not include
a fresh state delta. Call `diff` or `verify` for that. Live `cdp` is accepted
and is not required by tests.

Tool failures are `isError: true` with a JSON object whose `variant` matches
`ToolError`. Protocol failures (`ParseError`, `InvalidRequest`,
`MethodNotFound`, `MissingToolName`) are JSON-RPC errors with the same
`variant` field. Tests compare the enum with `assert_eq!`, not `is_err()`.

Error litmus, 2026-10-05: `resolve_origin`'s missing-source arm was temporarily
`Err(ToolError::MissingExpect)`. `cargo test -p hyper-use-mcp --test server --
error_variants_are_exact -- --exact` failed:

```
assertion `left == right` failed
  left: MissingExpect
 right: MissingFixture
```

The arm was restored to `ToolError::MissingFixture`.

Verifiers: unit and in-process JSON-RPC tests own the dispatch and the exact
variants. A 16-case proptest owns "random stdin lines do not panic". That is
not a fuzz campaign and not a proof. The stdio subprocess test owns "the
binary is not a name-only stub". No Miri, Loom, Kani, TLA+, or Lean. One
blocking reader is not a concurrent protocol. No second model of the ranker
or of fusion was added.

`Ranker` is the escape if `locate_with` returns `Err`. The default weighted
matcher and `HgraMatcher` with `ResonanceModel::V1` do not return it on a
manifold this crate just parsed. There is no fixture for it. A click is not
substituted.

## Fixture comparison, not a Browser Use score

The first comparison is the `fixture_compare` example in `hyper-use-cli`. It
parses `sidebar.manifold`, `sign-in.cdp.json`, `welcome.cdp.json`, and
`sign-in-press.cdp.json`, ranks with `WeightedMatcher`, and acts only on the
press fixture through `BrowserExecutor` and `ReplayTransport`.
`press-only.cdp.json` is omitted because it has no observation. Sign-in is not
diffed against welcome. `HgraMatcher` is not ranked here.

`executed` is true only after an action receipt. A scored confidence below 550
millis does not call the transport, and `executed` is false. A manifold file
cannot act, so the sidebar case has no `executed` field. The JSON is not a
`ComputerResult`: that type always carries `executed` and `verified`, which
would make a locate look like a fake refusal.

`typesafe-sdk` 0.2, feature `blocking`, is an optional `jev` feature of the
CLI crate only. It is not a path dependency, not a ranker dependency, and not
a second matcher. `cargo build` and `cargo test` do not compile it, so the
default `hyper-use` binary does not link Tokio. Enabling `jev` does: the SDK's
blocking client owns a current-thread runtime. That build is not the default.
Live calls run only when the feature and `HYPER_USE_JEV=1` are both set. Each
case is one `Client::from_env()` `system_one` choice. The model and base URL
come from the environment. State is the manifold (viewport, id, role, label,
rect), not a screenshot. The prompt does not contain the 0.55 rule or the
correct action. The client reads `choice` and does not read usage. `agree` is
id equality, or `press` / `do-not-press` against `executed`. It is not a win
and not a benchmark.

Downside accepted: four fixtures and one press. A live choice can disagree and
the run still succeeds, because this slice measures agreement, not a product
winner. That comparison does not call the Browser Use executor and is not a
Browser Use score. No tokens, screenshots, retries, latency, or winner are
recorded.

Verifier: the fixture test owns region ids, `executed` only on the press case,
absence of `jev` and `agree`, and confidence equal to a second `rank` of the
same manifold. The low-confidence arm owns "the transport log stays empty".
No Miri, Loom, Kani, or benchmark harness. The optional feature is typechecked
with `cargo check -p hyper-use-cli --features jev` and is not part of default CI.


## Browser Use semantic executor

Browser Use is an opt-in act backend, not a fallback and not an agent.
`ExecutorKind::BrowserUse` is absent from `DEFAULT_POLICY_ORDER`.
`select_executor` therefore keeps the CDP browser press when both are listed,
and `select_act_executor(None, _)` ignores Browser Use. The host names
`browser-use` with `select_requested`. A missing CDP session does not delegate.

The request is a `SemanticRequest`: region id, role, label, and action.
`to_wire` writes only those four keys. A fixture that carries `goal`, `url`,
`x`, `y`, `coordinates`, `navigate`, `task`, `screenshot`, `tokens`, `latency`,
or `retries` is `BrowserUseError::BadScript`. There is no coordinate click on
this path. The CDP press order (DOM semantic, then `DOM.focus`, then a
coordinate click) is unchanged.

`ReplayTransport` is the only transport. It records every `submit` and then
either returns a `TransportReceipt` or `BrowserUseError::Rejected`. It does
not start a process. A receipt whose id or action differs from the request is
`ParamsMismatch` after the call. Stubs still return `NotImplemented` and do
not panic. macOS and CUA stay unimplemented.

The confidence gate runs after the region id and the action match, and before
`submit`. A scored total below 550 millis returns
`ConfidenceBelowThreshold` and leaves the transport log empty. An unknown
region returns `UnknownRegion` and also does not submit.

Downside accepted: the replay script is the resolved target. This slice does
not observe through Browser Use, and it does not speak to a live Browser Use
process. A host that has not already located the region has nothing to hand
over. That is intentional. This is not a success rate, a token count, a
screenshot comparison, a retry policy, a latency, or a win over CDP.

Verifier: unit tests own the wire keys, the exact `Rejected` and `BadScript`
and `ParamsMismatch` variants, and "the log stays empty below 550 millis".
A 16-case proptest owns the key set for generated labels, and a 16-case
proptest owns the gate for integer millis in `0..550`. CLI and MCP tests own
selection: high confidence records `browser-use-semantic`, low confidence on
a rejecting script is still `executed: false`, and `macos` is
`NotImplemented`. No Miri, Loom, Kani, or benchmark harness. No second model
of the ranker. No live Browser Use process.

## CUA semantic handoff, not a fusion benchmark

CUA is an opt-in act backend, not a fallback and not an agent.
`ExecutorKind::Cua` is absent from `DEFAULT_POLICY_ORDER`, next to
`ExecutorKind::BrowserUse`. `select_executor` therefore keeps the CDP browser
press when CUA is also listed, and returns `NoneAvailable` when CUA is the
only listed kind. `select_act_executor(None, _)` drops both opt-in kinds
before that walk. The host names `cua` with `select_requested`. A missing CDP
session does not delegate.

The request is a `hyper_use_cua::SemanticRequest`: region id, role, label, and
action. `to_wire` writes only those four keys. A fixture that carries `goal`,
`url`, `x`, `y`, `coordinates`, `navigate`, `task`, `screenshot`, `tokens`,
`latency`, or `retries` is `CuaError::BadScript`. The script kind is
`cua-replay`. A `browser-use-replay` document is rejected, so the two grammars
are not interchangeable. There is no coordinate click on this path. The CDP
press order is unchanged, and the Browser Use path is unchanged.

`ReplayTransport` in `hyper-use-cua` is the only CUA transport. It records
every `submit` and then either returns a `TransportReceipt` or
`CuaError::Rejected`. It does not start a process. A receipt whose id or
action differs from the request is `ParamsMismatch` after the call. The
region id and the action are checked before the confidence gate, so an unknown
region or a different action does not submit. A scored total below 550 millis
returns `ConfidenceBelowThreshold` and leaves the transport log empty.
`StubExecutor` for `cua` still returns `NotImplemented` and does not panic.
macOS stays `NotImplemented`. `CuaStub::status` stays the pixel-driver
sentence. That stub is not a fusion score.

Downside accepted: the replay script is the resolved target. This slice does
not observe pixels, does not fuse a screenshot with the manifold, and does not
speak to a live computer-use process. A host that has not already located the
region has nothing to hand over. Removing CUA from the old last-resort slot
means a host that only lists `cua` as available no longer receives that kind
from `select_executor`; it must name it. That is intentional. This is not a
success rate, a token count, a screenshot comparison, a retry policy, a
latency, or a win over CDP or Browser Use.

`serde_json` parses the replay document inside `hyper-use-cua`, as it does for
Browser Use. `to_wire` returns a `String`. `serde_json::Value` is not part of
the public signature. No ranker crate depends on it.

Verifier: unit tests own the wire keys, the exact `Rejected`, `BadScript`, and
`ParamsMismatch` variants, and "the log stays empty below 550 millis". A
16-case proptest owns the key set for generated labels, and a 16-case proptest
owns the gate for integer millis in `0..550`. CLI and MCP tests own selection:
high confidence records `cua-semantic`, low confidence on a rejecting script
is still `executed: false`, an unnamed executor does not accept a `cua-replay`
script, and `macos` is `NotImplemented`. No Miri, Loom, Kani, or benchmark
harness. No second model of the ranker. No live CUA process. No fusion metric.

Error litmus, 2026-10-05: the empty-label `return Err` in
`hyper_use_cua::SemanticRequest::new` was temporarily replaced with
`let _skipped = CuaError::BadScript { message: "label must not be empty".into() };`.
`cargo test -p hyper-use-cua --lib -- --exact tests::empty_label_and_unknown_role_are_script_errors`
failed:

```
called `Result::unwrap_err()` on an `Ok` value: SemanticRequest { region_id: RegionId("n100"), role: Button, label: "", action: Click }
```

The return was restored. The test then asserts
`Err(CuaError::BadScript { message: "label must not be empty" })`, not `is_err()`.

