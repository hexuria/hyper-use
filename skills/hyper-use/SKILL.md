---
name: hyper-use
description: Resolve an interface target with hyper-use before acting on it.
---

# hyper-use

Never guess coordinates when hyper-use can resolve the target.

## Loop

1. OBSERVE the current interface into an interaction manifold.
2. LOCATE the target with text, role, and position. Do not invent a point.
3. INSPECT when more than one candidate is still plausible.
4. ACT on the resolved region id. If the matcher confidence is below 0.55, do not click.
5. DIFF the manifold.
6. VERIFY that the expected text appeared or the region disappeared.

If locate is ambiguous, stay on INSPECT. Do not click the runner-up.

## Commands

```bash
hyper-use locate --fixture fixtures/sidebar.manifold --text Settings --role button --position left --json
hyper-use locate "Sign in" --fixture fixtures/sign-in.cdp.json
hyper-use act n100 press --fixture fixtures/sign-in-press.cdp.json
hyper-use verify --fixture fixtures/welcome.cdp.json --expect-text Welcome
```

The default matcher is weighted. `--matcher hgra` selects the hyperdimensional
ranker. Do not treat that as a win; there is no benchmark. The command is
`hyper-use`. Do not rename it. Do not navigate. macOS and CUA are not available.

## Ranking

Trust the ranked region id. A disabled, hidden, occluded, offscreen, stale,
ambiguous, detached, or zero-size region is penalized and must not be chosen
over a clean match that satisfies the same query.
