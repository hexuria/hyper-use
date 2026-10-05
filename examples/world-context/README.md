# World-context smoke (live Chrome)

Deterministic observe → guard checks against a throwaway Chrome. No JEV, no Luna.

## Pages

| Page | Proves |
|---|---|
| `site/modal-confirm.html` | Open modal: buried "Delete project" refuses; dialog "Cancel" allows |
| `site/twin-suspend.html` | Twin "Suspend" escalates without context; `near(focus)` allows the focused row |
| `site/cookie-backdrop.html` | Non-dialog overlay: hit-test marks "Save" occluded and guard refuses |

## Run

```sh
./examples/world-context/smoke.sh
```

Requires Google Chrome at the usual macOS path (override with `HYPER_USE_CHROME`).
Uses ports `8766` (HTTP) and `9334` (CDP) by default.

Forced HGRA remasure (experimental matcher):

```sh
HYPER_USE_MATCHER=hgra ./examples/world-context/smoke.sh
```

## Rank traces

```sh
RUST_LOG=hyper_use_resonance=debug HYPER_USE_MATCHER=hgra ./examples/world-context/smoke.sh
```

Emits one `candidate` debug line per region (semantic, hypervector, total, …) and a `rank_top` summary. Off unless `RUST_LOG` enables `hyper_use_resonance`.
