# ADR 0008: Instinct rename + habituation via `arbitrate`

- Status: accepted
- Date: 2026-10-06 (Asia/Manila)
- Relates: ADR 0001, ADR 0002, ADR 0011 in hexuria/instinct

## Context

PUA was renamed Instinct: crates are `instinct-*` and the repo is
`hexuria/instinct`. `instinct-core` gained `arbitrate` (instinct ADR 0011):
consumer-supplied drives multiplied by option affinities modulate evidence
before the single existing `decide` gate. `arbitrate` never adds candidates
and never bypasses the gate; a near-tie still abstains (freeze).

## Decision

1. **Pin** `hexuria/instinct` at git rev `a42d16b6f5ccc3273939c8e3d3f462d78765bfea`,
   the first rev containing `arbitrate` on the renamed crates.
2. `PuaPolicy` is renamed `InstinctPolicy`; `PuaPolicy` remains a deprecated
   type alias for one release. `PUA_GIT_REV` is renamed `INSTINCT_GIT_REV`.
   `--policy instinct` is the CLI default; `--policy pua` remains a deprecated
   alias for the same variant.
3. **Habituation.** Both operation and target heads call `arbitrate` with
   exactly one drive, `("goal", 1000)`, and affinity
   `W[i] = 1000 - min(1000, HABITUATION_STEP * n)` where
   `HABITUATION_STEP = 250` and `n` is the number of trailing consecutive
   history entries whose `action_id == i` and whose verification is
   `no-effect` or `wrong-effect`. Incumbent is always `None`. Evidence scores
   are unchanged (`score_operation` / `score_action`).

## Consequences

- With no failed-repeat streak every `W = 1000`, so `urge == evidence` and
  the answer equals `decide` — identical behavior to before this change.
- Each repeated failure damps the action: ×0.75, ×0.5, ×0.25, then 0. A
  fourth consecutive no-effect can only freeze (abstain → the existing
  escalation path) or shift to a different offered choice; it can never
  invent an action or bypass the gate.
- No incumbent persistence for discrete browser actions: persistence would
  encourage repeating a click that just failed.
- Threat / danger stays in the guard and front layer (absolute), not a drive.
- More drives (threat, satiety, boredom) need committed benchmark evidence
  first; documented as future work, not built.

## Rejected alternatives

- Score-penalty habituation on the evidence directly (changes what evidence
  means); rejected in favor of the affinity tier so evidence stays pure.
- Multi-drive browser arbitration without benchmark data.
- Per-kind instead of per-action-id streaks: a repeated *id* is what the
  agent actually pressed.
