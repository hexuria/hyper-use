# hyper-use

**Hyper-Use is a Rust-native browser-agent runtime with a deterministic
decision kernel (PUA) and executor-bound action tickets.** It observes the
page (DOM + accessibility fusion, stable identity, stacking / hit-test
occlusion, modal front layer), builds a finite action space, lets PUA choose
or abstain, gates the choice, and executes exactly the ticketed action — or
nothing — then verifies the effect.

```text
goal
 │
 ▼  observe (CDP) ─► ActionSpace (front layer applied; hidden/disabled/covered excluded)
 │
 ▼  policy: PUA ─► abstain? ─► optional remote tier (feature `remote`, same finite menu)
 │
 ▼  TYPE_TEXT / SELECT payload: TextResolver (never PUA)
 │
 ▼  hard gate ─► ActionTicket (action + target + world fingerprint)
 │
 ▼  executor: one-shot ledger → fresh observe → revalidate → gate → consume → input
 │
 ▼  observe → diff / value read-back → success | no-effect | wrong-effect | navigation
 │
 └─► history → next tick (DONE / BLOCKED / abstain / bounds)
```

No LLM and no MCP are required to run the loop. See `docs/PRD.md`,
`docs/adr/0001…0003`, and `docs/EVAL.md` for status and honest gaps.

Toolchain: Rust 1.99.0. Versions are 0.1.0 and `publish = false`. Public API
is unstable until 1.0. HGRA is an experiment under `experiments/hgra/` and is
not on any default path.

## Run the agent

```bash
# Live: attach to a Chrome started with --remote-debugging-port
cargo run -p hyper-use-cli -- run --cdp http://127.0.0.1:9222 \
  --url https://example.com --goal 'Type "rust" into Search'

# Offline: predict-only dry run on a manifold (a --fixture *.cdp.json replay
# runs the full loop, but must script every observe/input/observe CDP call)
cargo run -p hyper-use-cli -- run --goal "Delete" --fixture fixtures/modal-confirm.manifold
# Full offline agent loop (type / click / select CDP replays)
cargo run -p hyper-use-cli -- run --goal 'Type "rust" into Search' \
  --fixture fixtures/agent-type-search.cdp.json
cargo run -p hyper-use-cli -- run --goal "Click Go" \
  --fixture fixtures/agent-click-go.cdp.json
cargo run -p hyper-use-cli -- run --goal 'Select "Business" in Cabin class' \
  --fixture fixtures/agent-select-cabin.cdp.json
```

Optional model payloads (feature `model-text`, ADR 0006): PUA still picks the
target; a model only extracts a TYPE_TEXT / SELECT value that must occur in the
goal, else deterministic fallback, else abstain. Tests use scripted models.

```bash
cargo run -p hyper-use-cli --features model-text -- run --cdp http://127.0.0.1:9222 \
  --goal "type rust ownership in the Search box" --text-model-cmd ./my-text-model.sh
```

```rust
let mut agent = AgentBuilder::new(BrowserSession::new(transport), PuaPolicy::default())
    .max_steps(20)
    .build(r#"Select "Business" in Cabin class"#);
let outcome = agent.run(); // Done | Blocked | Abstained | Failed, with verified steps
```

## MCP tools (optional adapter)

`observe`, `guard`, `verify` for hosts that keep their own executor. `act` is a
deprecated alias of `guard` and never clicks. Locate, inspect, and diff remain
debug helpers.

## Commands

```bash
cargo test --workspace
cargo run -p hyper-use-cli -- run --goal "Delete" --fixture fixtures/modal-confirm.manifold
# Full offline agent loop (type / click / select CDP replays)
cargo run -p hyper-use-cli -- run --goal 'Type "rust" into Search' \
  --fixture fixtures/agent-type-search.cdp.json
cargo run -p hyper-use-cli -- run --goal "Click Go" \
  --fixture fixtures/agent-click-go.cdp.json
cargo run -p hyper-use-cli -- run --goal 'Select "Business" in Cabin class' \
  --fixture fixtures/agent-select-cabin.cdp.json
cargo run -p hyper-use-cli -- observe --fixture fixtures/sign-in.cdp.json
cargo run -p hyper-use-cli -- guard \
  --fixture fixtures/sign-in.cdp.json \
  --action click --target "Sign in" --role button --json
cargo run -p hyper-use-cli -- verify \
  --fixture fixtures/welcome.cdp.json --expect-text Welcome
```

See `docs/PRD.md`, `docs/DECISIONS.md`, and `docs/EVAL.md`. Historical A1–A8 benchmark evidence that motivated
this pivot lives on the `bench/uniform` branch (`RESULTS.md`, PR #2).
