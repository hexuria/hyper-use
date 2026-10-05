# hyper-use

Hyper-Use is an independent action-verification layer for browser agents. It
resolves what an agent is about to interact with, refuses ambiguous or unsafe
actions, and verifies the resulting state change.

It is not an agent. It does not choose goals, navigate, click, type, or route
executors. Browser Use (or another host executor) performs the trusted action
after a `GuardDecision::Allow`.

HGRA is the name of one experimental matcher. It is not the product name and
is not on the default dependency graph. A crate or binary named `hgra` in the
product path is a bug.

## Already outside this repository

These exist before hyper-use is called. This repository does not implement them.

- primary model / planner
- JEV (optional escalation arbiter only)
- capability router
- Browser Use / CUA / other executors
- execution loop
- journal

## Product operations

1. **observe** — build an interaction manifold for one viewport (DOM + AX fusion,
   stable region identity, visibility and enabled state).
2. **guard** — resolve the proposed target, compare candidates, check visibility /
   enabled / occlusion / ambiguity, return `Allow`, `Refuse`, or `Escalate`.
3. **verify** — after the host acts, observe again, diff, and check the expected
   postcondition (`SUCCESS` / `NO-EFFECT` / `WRONG`).

Locate, inspect, and diff remain internal primitives. They are not the external
product workflow.

## What Hyper-Use deliberately does not do

- Click, type, select, scroll, or navigate.
- Own an executor router (Browser Use / CUA / macOS).
- Plan multi-step goals.
- Depend on JEV in the core path.
- Ship HGRA as the default matcher.

## Guard decision

```text
Allow    { target, confidence, margin, evidence }
Refuse   { reason, candidates }
Escalate { reason, candidates }
```

Reasons include low confidence, ambiguous twins, missing target, disabled /
hidden / occluded control, and (on verify) no effect or wrong postcondition.

## Matchers

- `WeightedMatcher` is the product default.
- HGRA lives under `experiments/hgra/` and stays there until it beats
  `WeightedMatcher` on a large adversarial target-resolution suite.

## Acceptance bar (product)

Normal actions: low false-refusal rate.
Missing / ambiguous / occluded / hidden targets: refuse; never wrong-action.
No-effect and wrong postcondition: detect on verify.

Competitive accuracy at "browser use" is not the goal. Never making Browser Use
less safe, and reducing tokens / retries / wrong clicks when composed with it, is.
