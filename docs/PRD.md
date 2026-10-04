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
`low-confidence`. hyper-use does not call CUA to paper over that miss.

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
5. Evaluation against the current JEV to Browser Use path. Not started.
6. Hyper matcher experiment. The ranker is selectable. No bake-off yet.
7. CUA fusion. Not started. The stub still refuses.
8. macOS. Last. The accessibility crate and the Mac app stay stubs.
