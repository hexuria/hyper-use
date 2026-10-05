# References

## browser-use/jev-ultrafast (MIT)

Architectural and behavioral reference for the finite action-space agent loop.
Not a mechanical translation target. License: MIT (preserve attribution when
substantially derived fixtures or flows are copied).

Upstream: <https://github.com/browser-use/jev-ultrafast>

| jev-ultrafast | hyper-use |
|---|---|
| `agent.py` | `hyper-use-agent` (planned) |
| `browser.py` | `hyper-use-browser` |
| `snapshot.js` | browser observation / ActionSpace (existing DOM+AX; not a JS port) |
| `model.py` | `hyper-use-policy` + `ActionSpace` |
| `questions.py` | consumer policy / eval data |
| `fresh()` | ActionTicket revalidation |
| `Browser.act()` | guarded Rust CDP executor |
| history | agent history / journal |

Where Hyper-Use intentionally goes further: DOM+AX fusion, stable cross-observation
identity, modal/front-layer and occlusion reasoning, PUA local decision tier,
deterministic abstention, ActionTickets, postcondition verification
(no-effect / wrong-effect), replay/evals.

## hexuria/pua

Domain-agnostic decision engine. ADR 0010: PUA owns HOW; Hyper-Use owns WHAT.
Pin by git rev when `PuaPolicy` lands. Repo: <https://github.com/hexuria/pua>
