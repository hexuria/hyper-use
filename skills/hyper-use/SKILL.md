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
4. ACT on the resolved region id.
5. VERIFY that the manifold changed the way the action required.

If locate is ambiguous, stay on INSPECT. Do not click the runner-up.

## Commands

```bash
hyper-use locate --fixture fixtures/sidebar.manifold --text Settings --role button --position left --json
```

Phase 1 reads a static fixture. It does not drive a browser or macOS.
The command is `hyper-use`. Do not rename it.

## Ranking

Trust the ranked region id. A disabled, hidden, occluded, offscreen, stale,
ambiguous, detached, or zero-size region is penalized and must not be chosen
over a clean match that satisfies the same query.
