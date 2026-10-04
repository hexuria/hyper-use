# hyper-use

hyper-use resolves an interface target before anything acts on it. Phase 1 is a
static interaction manifold and a deterministic locate: the same snapshot and
the same query always produce the same ranking. Browser control, macOS
accessibility, and computer-use clicks are not implemented.

## Phase 1

Given regions in one viewport, `locate` ranks every region. The score is
hypervector resonance (0.35), semantic match (0.20), source agreement (0.15),
geometric zone (0.10), actionability (0.10), temporal stability (0.05), and
contextual consistency (0.05), minus versioned penalties for disabled, hidden,
occluded, offscreen, stale, ambiguous, detached, and zero-size regions. Ties
break by region id. No model weights are learned and no random source is used.

```bash
cargo test --workspace
cargo run -p hyper-use-cli -- locate \
  --fixture fixtures/sidebar.manifold \
  --text Settings --role button --position left --json
```

The binary is `hyper-use`. Crates live under `crates/hyper-use-*`.

## Later, not in this tree

Live observation, CDP, the macOS host, AXUIElement, CUA actuation, and an MCP
server runtime are named so the policy order exists (browser, then macOS, then
CUA) and so every stub refuses to act. Do not treat a stub status string as a
working backend.

The toolchain is pinned to Rust 1.99.0 in `rust-toolchain.toml`.
