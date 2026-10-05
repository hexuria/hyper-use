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

## Benchmark scenarios

Six local scenario sites for the uniform agent benchmark in `bench/` (results in
`RESULTS.md` at the repo root and in each scenario's `RESULTS.md`). Every page
takes `?seed=N` (reorders rows and twins, never changes what a task means) and
reports ground truth to the bench server through `/_shared/bench.js`.

| Scenario | What it stresses |
|---|---|
| `acme-mail/` | Gmail-like inbox: twin Send buttons, hidden/offscreen Archive twins, disabled Save, star per row |
| `shop-checkout/` | product grid with near-twin names, cart steppers, address form, gated Place order |
| `admin-table/` | users table, per-row menus, confirm dialogs, role filters, paging, near-duplicate names |
| `booking-calendar/` | month grid with long day labels, time slots, confirm step, native select, toggles |
| `hard-dom/` | icon-only twins, open shadow roots, a same-origin iframe banner, a promo overlay |
| `travel-search/` | adapted from jev-ultrafast's fixture (MIT, Browser Use): search, select, checkbox, detail view |

Look at them by hand with `bench/.venv/bin/python bench/server.py --port 8765` and
open `http://127.0.0.1:8765/<scenario>/site/index.html`.

## World-context smoke

`world-context/` — live Chrome observe → guard checks (modal, twin Suspend,
cookie backdrop). No JEV. See [world-context/README.md](world-context/README.md).

```sh
./examples/world-context/smoke.sh
# optional forced HGRA:
HYPER_USE_MATCHER=hgra ./examples/world-context/smoke.sh
```

## Live drive

`live-drive/` — hand-run JEV + `hyper-use mcp` against the Acme Mail page
(same site as the `acme-mail/` bench scenario). Not CI and not a benchmark.
See [live-drive/README.md](live-drive/README.md).

```sh
python3 bench/server.py --port 8765 &
# throwaway Chrome on CDP 9333, then:
cargo build --release -p hyper-use-cli --bin hyper-use
cargo build --release -p hyper-use-cli --features jev --example live_drive
TYPESAFE_API_KEY=... HYPER_USE_JEV=1 \
  ./target/release/examples/live_drive --bin target/release/hyper-use \
    --site http://127.0.0.1:8765/acme-mail/site
```
