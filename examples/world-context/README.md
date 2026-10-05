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
