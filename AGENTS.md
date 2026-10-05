# hyper-use

The product, crates, and binary are `hyper-use`. HGRA is the name of one matcher. A crate or binary named `hgra` is a bug.

hyper-use is not an agent. Operations are observe, locate, inspect, act, diff, verify. No navigate.

The product default matcher is `WeightedMatcher`. `HgraMatcher` is selectable. Do not claim one won without a benchmark.

Phase 2 speaks CDP through one transport trait. Replay fixtures and a live websocket share that trait. macOS stays unimplemented. The CUA pixel driver stays unimplemented. Opt-in `cua` is a semantic replay, not fusion. A low-confidence act does not call CUA.

Public API is 0.1 and unstable until 1.0. Toolchain pin: Rust 1.99.0. `publish = false`.

## Verification

Deterministic ranking and the bipolar algebra are owned by unit tests and `proptest`. Do not add a second model of `locate_with` or of `WeightedMatcher::rank`.

`write_fixture` / `parse_fixture` own the manifold fixture grammar. CDP replay parsing is a different grammar. `structural_similarity` is a different metric, not a second ranker. Fusion is the only DOM/accessibility merge.

Diff semantics on a given id (a move, an enabled change, a press) are owned by the observe id-diff test. Region identity across observations is owned by the browser session's `IdentityMap`, the only identity service; it pairs with `match_regions` and does not add a second similarity metric.

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
Affected invariants: default locate is weighted; HGRA remains selectable; fusion joins a DOM/AX pair with the same backend id, merges a 1px pair, and refuses different labels on different nodes; press prefers a DOM click; verify fails with ExpectedTextMissing; confidence below 550 millis does not click; a region id survives move, enabled change, and press.
Tests or proofs updated: resonance matcher tests, browser fusion and session tests, executor confidence test, CLI command tests, observe identity test, protocol contract test. No second formal model.
```

## MCP

`hyper-use mcp` is a newline-delimited JSON-RPC server. Tools are observe, locate, inspect, act, diff, verify. No navigate. The ranker crates do not depend on `hyper-use-mcp`.

The server owns a 16-entry in-memory snapshot ring and up to four live CDP sessions. A failed live call drops its session; there is no retry or reconnect. Act can observe after the press, diff, and verify in one call.

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


## Browser Use executor

`browser-use` is an opt-in act backend. It is not in `DEFAULT_POLICY_ORDER` and it does not navigate. The semantic request is region id, role, label, and action. The CDP press path is unchanged. macOS stays unimplemented. A scored confidence below 550 millis does not call the replay transport.

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

Reason: the Browser Use path is one blocking replay script. Recording a request is not crash recovery, and choosing the backend is not a concurrent protocol, so no system model was added.
Affected invariants: default act stays the CDP browser press; Browser Use is selected only when named; the wire object has four semantic keys and no goal or coordinate; confidence below 550 millis does not submit; a scripted rejection is a typed error; macos still returns NotImplemented. The CUA stub executor still returns NotImplemented; the opt-in replay is a later section.
Tests or proofs updated: browser-use replay tests, a 16-case proptest of wire keys, executor gate and receipt tests, a 16-case proptest of the act gate, CLI and MCP act tests. No second formal model.
```


## CUA semantic handoff

`cua` is an opt-in act backend. It is not in `DEFAULT_POLICY_ORDER` and it does not navigate. The semantic request is region id, role, label, and action. This is not a CUA fusion benchmark. The CDP press path and the Browser Use path are unchanged. macOS stays unimplemented. A scored confidence below 550 millis does not call the CUA transport. `CuaStub` still says pixel actuation is a later phase.

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

Reason: the CUA path is one blocking replay script. Recording a request is not crash recovery, and choosing the backend is not a concurrent protocol, so no system model was added.
Affected invariants: default act stays the CDP browser press; CUA is selected only when named; the wire object has four semantic keys and no goal or coordinate; confidence below 550 millis does not submit; a scripted rejection is a typed error; macos still returns NotImplemented; a missing CDP session does not select CUA.
Tests or proofs updated: cua replay tests, a 16-case proptest of wire keys, executor gate and receipt tests, a 16-case proptest of the act gate, CLI and MCP act tests. No second formal model.
```


## Locate precision, act margin, and press tiers

The weighted text term is `recall * (0.5 + 0.5 * precision)`, so an exact label outranks a superset label. HGRA's semantic term is unchanged. A ranked act whose top and runner-up differ by less than 50 millis does not press and returns fallback `ambiguous`; an inspected act is not gated. Observe omits a node whose `DOM.getBoxModel` is a CDP error. Press is a DOM click by node id, then by backend node id, then a coordinate click. A focus is not a click, so there is no `DOM.focus` tier.

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

Reason: the weighted text term and the act margin are pure functions of their inputs. Observe and press are one blocking CDP call stream. Skipping a node with no box and dropping a press tier do not add interleaving, so no system model was added.
Affected invariants: an exact label outranks a superset label under weighted; HGRA semantic is unchanged; a ranked act with a margin below 50 millis does not press and returns fallback ambiguous; a runner_up without confidence is an error; an inspected act is not gated; a getBoxModel protocol error omits that node and a missing script step is still fatal; press never reports DOM.focus as a click; a thrown click function is a tier failure.
Tests or proofs updated: matcher unit test, a 16-case weighted proptest, a CLI locate test, executor margin tests and two 16-case margin proptests, MCP and CLI act tests, browser observe tests on hidden-node.cdp.json, press tier tests. No second model of WeightedMatcher::rank.
```


## MCP session and snapshot ring

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

Reason: the server is still one blocking stdin reader. Keeping a session between calls adds ownership but no interleaving; a failed call drops the session and there is no retry, so it is not a recovery protocol and the system model is unaffected. The ring is in memory, so persistence is unaffected.
Affected invariants: snapshot ids strictly increase and are never reused; an evicted id is an exact error; diff by snapshot ids equals diff by the same fixtures; act with an expectation returns a real state_delta and verified; a failed postcondition is executed true and verified false; act with locate fields refuses when region is not first; the free call_tool keeps no state.
Tests or proofs updated: history unit test and a 16-case proptest, MCP closed-loop act tests, diff-by-snapshot tests, and a stdio subprocess test. No second formal model.
```


## Session identity map

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

Reason: identity assignment is a pure function of the previous observation, the fresh observation, and the session map. Observe is one blocking call stream, so no system model was added.
Affected invariants: a first observation keeps fused ids; a re-rendered control with the same role, label, and nearby position keeps its id and the press uses the new node; a distant same-label control gets a new id; a stable id is never reused for another control; re-observing an identical page keeps every id.
Tests or proofs updated: identity unit tests and a 16-case proptest, browser session re-render and distance tests. No second identity service and no second similarity metric.
```


## Fusion v2 and DOM parents

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
[ ] Property-test / fuzz surface
[ ] No verification architecture impact

Reason: fusion and the DOM walk are pure functions of the CDP results. No system model was added.
Affected invariants: a DOM node and an accessibility node with the same backend id are one region even when labels differ; the heuristic pass only sees unjoined nodes; a region's parent is its nearest DOM ancestor that is a region.
Tests or proofs updated: fusion unit tests and a browser observe test with a nested control. Fusion is still the only DOM/accessibility merge.
```

## Page state and delta verify

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
[ ] Property-test / fuzz surface
[ ] No verification architecture impact

Reason: page state is parsed from one extra CDP result and the diff accessors read the existing id diff. No system model was added. No Loom, Kani, Miri, TLA+, or Lean.
Affected invariants: a region that only moves is moved and not relabeled; a URL change is reported when no region id changes; focus comes from the AX focused property; an empty diff with an unchanged page is VerifyError::NoEffect; a protocol error on Page.getNavigationHistory omits url and title; a missing history step is fatal; captured_at_ms stays 0 when the history payload has no time; the field is url_changed, not navigated.
Tests or proofs updated: moved_only_rect_is_moved_not_relabeled, url_change_is_reported_without_a_region_change, focus_moves_to_the_text_field, executed_act_with_no_delta_is_no_effect, navigation_history_protocol_error_omits_url_and_title, missing_navigation_history_step_is_fatal. No second formal model.
```

## Temporal signals and the locate corpus

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
[ ] Property-test / fuzz surface
[ ] No verification architecture impact

Reason: detect reads the in-memory snapshot ring. The corpus ranks local fixtures. No system model was added. No Loom, Kani, Miri, TLA+, or Lean.
Affected invariants: a signature equal to the act's before snapshot is NoOp; a signature equal to an older same-origin snapshot in the previous four, other than before, is LoopDetected; signals do not retry and do not replace VerifyError::NoEffect; eval_corpus reports weighted and hgra top-1 hits, margin, and gate refusal and names no winner; the default matcher stays WeightedMatcher.
Tests or proofs updated: no_op_when_after_equals_before, loop_when_after_equals_an_older_snapshot, eval_corpus_reports_both_matchers_without_a_winner. No second formal model.
```
