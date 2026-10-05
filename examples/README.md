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
