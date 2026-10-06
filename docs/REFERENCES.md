# References

## browser-use/jev-ultrafast (MIT)

Architectural and behavioral reference for the finite action-space agent loop.
Not a mechanical translation target. License: MIT (preserve attribution when
substantially derived fixtures or flows are copied).

Upstream: <https://github.com/browser-use/jev-ultrafast>

| jev-ultrafast | hyper-use |
|---|---|
| `agent.py` | `hyper-use-agent` (`Agent`: predict / act / tick / run) |
| `browser.py` | `hyper-use-browser` |
| `snapshot.js` | browser observation / ActionSpace (existing DOM+AX; not a JS port) |
| `model.py` | `hyper-use-policy` + `ActionSpace` |
| `questions.py` | consumer policy / eval data |
| `fresh()` | fresh observe + `revalidate` + hard `gate` at the executor (ADR 0003) |
| `Browser.act()` | `hyper_use_agent::execute_ticketed` → `BrowserSession::{press, type_text, select_option, scroll}` |
| history | agent history / journal |

Where Hyper-Use intentionally goes further: DOM+AX fusion, stable cross-observation
identity, modal/front-layer and occlusion reasoning, Instinct local decision tier,
deterministic abstention, ActionTickets, postcondition verification
(no-effect / wrong-effect), replay/evals.

## hexuria/instinct

Domain-agnostic decision engine. ADR 0010: Instinct owns HOW; Hyper-Use owns WHAT.
Pinned by git rev `a42d16b` in the workspace; `InstinctPolicy` is the default policy. Repo: <https://github.com/hexuria/instinct>
