# hyper-use

hyper-use is a persistent spatial-memory, target-resolution, action-execution,
and verification subsystem for computer-use capabilities. It sits under an
existing agent / JEV loop. It is not an agent. It does not choose goals, and
it does not navigate.

HGRA is the name of one matcher (the hyperdimensional ranker). It is not the
product name. A crate or binary named `hgra` is a bug.

## Already outside this repository

These exist before hyper-use is called. This repository does not implement them.

- primary model
- JEV
- capability router
- delegation
- execution loop
- journal

A host turns one computer-use step into a [`ComputerTask`](../crates/hyper-use-protocol/src/contract.rs)
and reads a [`ComputerResult`](../crates/hyper-use-protocol/src/contract.rs).
Intent is a locate or a single act. It is not a workflow goal. There is no
method here that plans multi-step navigation.

## Operations

Only these:

1. observe
2. locate
3. inspect
4. act
5. diff
6. verify

No navigate command, no navigate intent, no goal runner.

## What a turn does

Observe builds an interaction manifold for one viewport. Locate ranks regions
for a structured query. Inspect is required when more than one candidate is
still plausible. Act names a region id. The press preference on a browser
session is DOM semantic click, then a CDP element action (`DOM.focus`), then
a coordinate click. Diff is id-based. Verify checks expected text or that a
region disappeared.

If the matcher confidence used for the act is below 0.55 (550 millis), act
returns `ConfidenceBelowThreshold` and does not click. The host can journal
that as a `ComputerResult` with `executed = false` and fallback
`low-confidence`. hyper-use does not call CUA, and it does not call the
Browser Use transport, to paper over that miss.

The default act backend is still the CDP browser press. `--executor browser-use`
(or MCP `executor: "browser-use"`) is opt-in. It sends the already located
region id, role, label, and click to a replay transport. It is not in the
default policy order, so a missing CDP session does not delegate to it. It
does not take a goal and it does not navigate. macOS and CUA stay unimplemented.

An operator who names the region (`act <id> press` with no confidence) is
treated as inspected. The gate does not apply. A host that just located
should pass the matcher total as `--confidence`.

## Matchers

Both implement `RegionMatcher::rank`.

- `WeightedMatcher` is the default. Semantic match (text and role combined by
  minimum), geometry, actionability, then the versioned penalties. No
  hypervectors.
- `HgraMatcher` is the existing hyperdimensional ranker behind the same trait.
  Select it with `--matcher hgra`.

Both rank the sidebar Settings control first on `fixtures/sidebar.manifold`.
That is not a benchmark. Neither matcher is claimed to have won.


## Fixture agreement

`cargo run -p hyper-use-cli --example fixture_compare` ranks
`sidebar.manifold`, `sign-in.cdp.json`, `welcome.cdp.json`, and
`sign-in-press.cdp.json` with `WeightedMatcher`. Only the press fixture is
acted, through the replay executor, and only when locate confidence clears
0.55. Stdout is one JSON document: region id, confidence, and `executed` on
that press. Unmeasured keys are omitted. This is not a Browser Use score and
it is not a `ComputerResult` (that type always sets `executed` and `verified`).

`typesafe-sdk` 0.2 is an optional `jev` feature of the CLI, off in the default
build. With `HYPER_USE_JEV=1` it asks one choice per case. `agree` is whether
that choice equals the region id, or `press` / `do-not-press` against
`executed`. It is not a win. That example does not call the Browser Use executor.

## Identity

A region id is not a coordinate and not an enabled bit. The same id remains
after a move and after an enabled change. A press addresses that id; it does
not mint a new one. On a browser snapshot the id is `n{backendNodeId}` from
the DOM node, so a later observation of the same Chrome node keeps the id
when the backend id does.

## Roadmap

1. State foundation. Done. Manifold, geometry, weighted matcher, HGRA matcher.
2. Browser integration. This tree. CDP client, DOM and accessibility fusion,
   press, refresh, verify. Fixture-proven.
3. JEV contract types. This tree. Task and result only. No JEV runtime.
4. MCP, CLI, and skill surfaces. `hyper-use mcp` serves the six tools over stdio JSON-RPC. No navigate tool.
5. Fixture id and action agreement only. `WeightedMatcher` on four local fixtures, plus an optional System One choice. That example does not call Browser Use and is not a score.
6. Browser Use semantic executor. Opt-in replay of one region id, role, label, and click. Not a benchmark. The CDP press path is unchanged. macOS and CUA stay unimplemented.
7. Hyper matcher experiment. The ranker is selectable. No bake-off yet.
8. CUA fusion. Not started. The stub still refuses.
9. macOS. Last. The accessibility crate and the Mac app stay stubs.
