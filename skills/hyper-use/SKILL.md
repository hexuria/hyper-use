---
name: hyper-use
description: Resolve one computer target with hyper-use before acting on it.
---

# hyper-use

hyper-use is the computer capability under an existing agent loop. It does not
choose the next capability. JEV does. hyper-use does not navigate and it does
not accept a multi-step goal.

Never guess coordinates. Act on a region id from locate or inspect. A tool
argument named `x`, `y`, or `coordinates` is rejected.

## Tools

The MCP server is `hyper-use mcp` (newline-delimited JSON-RPC on stdio). The
tool names are exactly:

1. `observe` reads a fixture (or an optional live CDP endpoint) into regions.
2. `locate` ranks one query. The default matcher is `weighted`. `matcher: "hgra"` selects the hyperdimensional ranker. That selection is not a benchmark and not a measured win. The result sets `benchmark` to false.
3. `inspect` returns one region, including its rectangle. The rectangle is descriptive. Do not click it.
4. `act` presses one region id. The default executor is the CDP browser press: DOM click comes before coordinates. `executor: "browser-use"` instead hands that region id, role, and label to a replay transport. It is not a goal, not navigation, and not a benchmark. Omit `confidence` only after inspect. If confidence is below 0.55, the result has `executed: false` and `fallback: "low-confidence"`. That is not a click. Hand control back. Do not retry with a guessed point.
5. `diff` returns `state_delta` (`added`, `removed`, `changed`) between two observations.
6. `verify` checks one postcondition: `expect_text` appeared, or `expect_absent` is gone.

There is no `navigate` tool. Pass a CDP fixture path in `fixture`. Live `cdp` is optional. Tests do not need Chrome.

## Confidence escape hatch

High confidence, or a region you already inspected: `act` may press.

Low confidence: do not click. The tool result is the hand-back. JEV decides whether to inspect, ask, or use another capability. hyper-use will not make that choice.

## Commands

```bash
hyper-use locate --fixture fixtures/sidebar.manifold --text Settings --role button --position left --json
hyper-use locate "Sign in" --fixture fixtures/sign-in.cdp.json
hyper-use act n100 press --fixture fixtures/sign-in-press.cdp.json
hyper-use verify --fixture fixtures/welcome.cdp.json --expect-text Welcome
hyper-use mcp
```

The command is `hyper-use`. Do not rename it. macOS and CUA are not available. `--executor browser-use` is a semantic replay, not a live Browser Use process.
