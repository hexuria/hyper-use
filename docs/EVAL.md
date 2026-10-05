# Eval / parity status (agent + PUA runtime)

Date: 2026-10-06 (Asia/Manila). See ADR 0002 and ADR 0003.

## Arms

| Arm | Meaning | Status |
|---|---|---|
| A | upstream `jev-ultrafast` (Python reference) | **not run** here |
| B | Hyper-Use + remote/Jev policy only | wire + transport trait (`remote` feature); no live model wired |
| C | Hyper-Use + PUA only | **offline e2e (mock + CDP replay) + live Chrome smoke** |
| D | Hyper-Use + PUA → escalation | **offline e2e** with `ScriptedRemote`; no live model |

No arm has multi-run, multi-site numbers yet. Do not quote any of this as
general performance evidence (n is tiny and the tasks are single-intent).

## How to run arm C offline (no Chrome, no network, no model)

```bash
# Everything CI runs
cargo test --workspace

# Just the owned loop
cargo test -p hyper-use-agent                    # all agent tests
cargo test -p hyper-use-agent --test adversarial # mock adversarial e2e (16)
cargo test -p hyper-use-agent --test replay_cdp  # real BrowserSession over a CDP replay (6)
cargo test -p hyper-use-agent --test props       # property tests (ticket substitution, stale, abstain)
cargo test -p hyper-use-agent --test mock_loop   # original ADR 0002 loop tests
cargo test -p hyper-use-guard gate               # hard gate unit tests
cargo test -p hyper-use-policy                   # PUA policy, evidence, resolver
```

From the CLI, a CDP replay script runs the full loop offline:

```bash
cargo run -p hyper-use-cli -- run --goal 'Type "rust" into Search' \
  --fixture path/to/script.cdp.json            # ScriptBuilder JSON
cargo run -p hyper-use-cli -- run --goal "Delete" \
  --fixture fixtures/modal-confirm.manifold    # static manifold: predict-only dry run
cargo run -p hyper-use-cli -- run --goal 'Type "rust" into Search' \
  --fixture fixtures/agent-type-search.cdp.json
cargo run -p hyper-use-cli -- run --goal 'Click Go' \
  --fixture fixtures/agent-click-go.cdp.json
cargo run -p hyper-use-cli -- run --goal 'Select "Business" in Cabin class' \
  --fixture fixtures/agent-select-cabin.cdp.json
```

The dry run on `modal-confirm.manifold` predicts `CLICK:confirm-delete`: the
background `Delete project` is behind the modal and is not offered.

## Arm D offline (PUA → remote)

```bash
cargo test -p hyper-use-policy -p hyper-use-agent \
  --features hyper-use-agent/remote,hyper-use-policy/remote
```

`remote_e2e.rs`: twins make PUA abstain → scripted remote picks `CLICK:b` →
same gate / ticket / executor path → only `b` is pressed. Off-menu replies
(`CLICK:#delete-all`) fail without input. `UnconfiguredRemote` keeps the PUA
abstain.

## Live Chrome smoke (arm C, manual)

```bash
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new \
  --remote-debugging-port=9333 '--remote-allow-origins=*' \
  --user-data-dir=/tmp/hu-smoke about:blank &
HYPER_USE_CDP=http://127.0.0.1:9333 \
  cargo test -p hyper-use-agent --test live_smoke -- --ignored --nocapture
```

Or by hand: `hyper-use run --cdp http://127.0.0.1:9333 --url <page> --goal …`.

Last run 2026-10-06 ~00:10 (Asia/Manila) (Chrome headless, macOS, inline `data:` page):

| Goal | Step | Verification | Outcome |
|---|---|---|---|
| `Type "rust ownership" into Search` | TYPE_TEXT `Search` | success (value read back) | done |
| `Select "Business" in Cabin class` | SELECT `Cabin class` | success (option read back) | done |
| `Click Go` | CLICK `Go` | state-changed (result text appeared) | done |
| `scroll down` | SCROLL_DOWN | state-changed | done |
| `Select "Premium" in Cabin class` (CLI) | — | page rejected: 0 matching options | failed, nothing changed |
| `scroll down` at page bottom (CLI) | SCROLL_DOWN ×3 | no-effect | blocked (bound) |

n = 1 per goal, one static page. It proves the paths work end to end on a real
browser; it measures nothing.

## Adversarial coverage (offline)

| Case | Where | Result asserted |
|---|---|---|
| twin buttons / twin rows | `adversarial`, `mock_loop`, `props` | abstain, no input |
| modal appears after prediction | `adversarial`, executor unit | stale, background never pressed |
| background behind open modal | `adversarial`, gate unit | not in action space; gate `front-layer` |
| covered (hit-test) control | `replay_cdp` | marked occluded, not offered, gate `occluded` |
| disabled / hidden / offscreen | `adversarial`, gate unit | never executed |
| rerender replaces node (new id) | `adversarial` | stale, no input |
| rerender relabels target (same id) | executor unit, `props` | stale, no input |
| cookie / toast added (world change) | `replay_cdp`, `props` | stale, no input |
| focus change between predict/act | `adversarial` | stale, no input |
| page changes during text resolution | `adversarial` | stale, nothing typed |
| readonly / rejected input | `adversarial`, `replay_cdp` | `InputRejected`, ticket consumed, nothing typed |
| unresolvable TYPE_TEXT value | `adversarial` | failed, nothing typed |
| wrong effect (page mangles value) | `adversarial` | `wrong-effect` ×3 → blocked |
| repeated no-effect click | `adversarial` | bounded → blocked |
| page never settles | `adversarial` | stale bound → failed, no input |
| off-menu / selector / kind-mismatch policy output | `adversarial`, `remote` unit, `remote_e2e` | hard error, no input |
| ticket reuse / operation swap | executor unit, `props` | refused |
| multi-step `then` / `and then` | `adversarial` | both clauses execute; connective limits documented |
| readonly observed before TYPE | `gate` unit, `adversarial` | hard refuse / no input |
| unrelated far banner vs nearby twin | `world` unit, `ticket`, `adversarial` | far OK; nearby/modal stale |

Not covered yet: iframes, shadow DOM, virtualized lists, autocomplete
suggestion popups, checkbox/radio "already satisfied", navigation between
prediction and execution on a live page.

## Honest gaps / out of scope

- Arms A/B/D have no live runs; no jev-ultrafast Wikipedia / travel parity
  (upstream unpaid path unavailable). Documented as blocked below.
- Multi-step is **only** plain `then` / `and then` outside quotes
  (`split_sequential_clauses`). No branching, conditionals, or LLM planner.
  Each clause remains one PUA single-intent.
- `model-text` TextResolver is still a feature name only; deterministic
  resolver handles quoted / `type X into` / `fill X with` forms.
- iframes / shadow DOM / virtualized lists / autocomplete are not covered.
- MCP `guard` still uses the float `0.55` / `0.05` ranking gate for host
  preflight (A5/A6 / combo benches). Agent path uses hard `gate` only.
  Allow tickets now share `gate::check` + target-scoped world fingerprints.
- RESULTS.md A1–A8 remain historical (pre-pivot); see the agent+PUA section.

## Offline arm C repeats (n = 3)

Scripted: `cargo test -p hyper-use-agent` three times on this tip. All three
runs green (same deterministic offline suite). See RESULTS.md § "Agent + PUA
pivot (offline arm C)".

Arms A / B / D: **blocked / out of scope** here — need upstream
`jev-ultrafast` and/or a paid remote model. Offline D (`ScriptedRemote`) still
passes under `--features remote`.

## PUA pin

`fe3f1fd3818feb452fae1771ff2171b8598f86e6`
