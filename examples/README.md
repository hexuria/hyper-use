# Examples

Phase 1 example: rank the sidebar fixture.

```bash
cargo run -p hyper-use-cli --example sidebar_locate
cargo run -p hyper-use-cli -- locate \
  --fixture fixtures/sidebar.manifold \
  --text Settings --role button --position left --json
```

The compiled example lives at `crates/hyper-use-cli/examples/sidebar_locate.rs`.
It prints the top region id and checks that it is `nav-settings`.
