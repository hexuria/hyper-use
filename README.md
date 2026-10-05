# hyper-use

**Hyper-Use is an independent action-verification layer for browser agents.**
It resolves what an agent is about to interact with, refuses ambiguous or
unsafe actions, and verifies the resulting state change.

It is not a browser agent. It is not a Browser Use or CUA replacement. It does
not click, type, navigate, or plan. Browser Use (or another agent executor)
performs the trusted action after Hyper-Use allows it.

```text
Agent / Browser Use
       │ proposes action
       ▼
┌─────────────────────────────┐
│         HYPER-USE           │
│ observe → resolve → gate    │
│ ALLOW / REFUSE / ESCALATE   │
└──────────────┬──────────────┘
               │ allow
               ▼
        Browser Use acts
               │
               ▼
┌─────────────────────────────┐
│         HYPER-USE           │
│ observe → diff → verify     │
│ SUCCESS / NO-EFFECT / WRONG │
└─────────────────────────────┘
```

Toolchain: Rust 1.99.0. Versions are 0.1.0 and `publish = false`. Public API
is unstable until 1.0. `WeightedMatcher` is the default; HGRA is an experiment
under `experiments/hgra/` and is not on the default path.

## MCP tools

`observe`, `guard`, `verify`. Locate, inspect, and diff remain available as
internal or deprecated helpers during the transition; they are not the product
surface.

## Commands

```bash
cargo test --workspace
cargo run -p hyper-use-cli -- observe --fixture fixtures/sign-in.cdp.json
cargo run -p hyper-use-cli -- guard \
  --fixture fixtures/sign-in.cdp.json \
  --action click --target "Sign in" --role button --json
cargo run -p hyper-use-cli -- verify \
  --fixture fixtures/welcome.cdp.json --expect-text Welcome
```

See `docs/PRD.md` and `docs/DECISIONS.md`. Benchmark evidence that motivated
this pivot lives on the `bench/uniform` branch (`RESULTS.md`, PR #2).
