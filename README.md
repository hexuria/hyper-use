# hyper-use

hyper-use resolves an interface target and can act on it. It is not an agent
and it does not navigate. The product name is hyper-use. HGRA is only the
name of one matcher.

The toolchain is pinned to Rust 1.99.0. Versions are 0.1.0 and `publish = false`.
The public API is unstable until 1.0.

## Roadmap

1. State foundation. Done.
2. Browser integration. This tree. Fixture-proven CDP observe, locate, act, diff, and verify. Live Chrome was exercised read-only (`Browser.getVersion` on an already-running debugging port). No click was sent to that browser. hyper-use does not launch or install Chrome.
3. JEV contract types. This tree. `ComputerTask` and `ComputerResult` only. JEV itself stays outside this repo.
4. MCP, CLI, and skill. `hyper-use mcp` serves observe, locate, inspect, act, diff, and verify on stdin. There is no navigate tool.
5. Fixture agreement with an optional System One choice. Not a Browser Use score.
6. Browser Use semantic executor. Opt-in replay of one region id, role, label, and click. Not a benchmark. The default act path remains the CDP press.
7. Hyper matcher experiment. `HgraMatcher` is selectable. It is not the default, and it has not been shown to beat `WeightedMatcher`.
8. CUA fusion. Not started.
9. macOS. Last. Accessibility and the Mac app stay stubs.

## Commands

Default matcher is weighted. Default CDP HTTP endpoint, used when `--cdp` is
passed with no URL, is `http://127.0.0.1:9222`.

```bash
cargo test --workspace
cargo run -p hyper-use-cli -- locate \
  --fixture fixtures/sidebar.manifold \
  --text Settings --role button --position left --json
cargo run -p hyper-use-cli -- locate "Sign in" \
  --fixture fixtures/sign-in.cdp.json
cargo run -p hyper-use-cli -- act n100 press \
  --fixture fixtures/sign-in-press.cdp.json
cargo run -p hyper-use-cli -- verify \
  --fixture fixtures/welcome.cdp.json --expect-text Welcome
```

`press` is the CLI verb for `Action::Click`. A scored confidence below 0.55
refuses the act and does not click. `--executor browser-use` sends the region
id, role, and label through a replay fixture. It does not navigate and it is
not a benchmark. The default executor remains the CDP press. macOS and CUA
still return not-implemented.

See `docs/PRD.md` and `docs/DECISIONS.md`.
