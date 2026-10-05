# Eval / parity status (agent + PUA pivot)

Date: 2026-10-05 (Asia/Manila). Main tip at merge time supersedes this draft.

## Arms (target)

| Arm | Meaning | Status |
|---|---|---|
| A | upstream jev-ultrafast | not run in this change (Python reference) |
| B | Hyper-Use + remote/Jev policy | not implemented (feature `remote` stub only) |
| C | Hyper-Use + PUA only | **offline mock e2e** in `hyper-use-agent` tests |
| D | Hyper-Use + PUA → escalation | unit-tested composition; no remote |

## What passes offline now

- PUA exact label → Choice
- Twin identical labels → Abstain (no press)
- Fast profile low-margin near-ties → Abstain
- Candidate order independence
- Ticket one-shot consume
- Stale world since prediction → discard (Ready)
- DONE control terminates without press
- Deterministic TextResolver quoted / `type … into …`

## Honest gaps

- Full jev-ultrafast Wikipedia / travel live suite: **not run**
- Live CDP `TYPE_TEXT` / `SELECT` / scroll actuation: Agent maps kinds but
  `BrowserSession::press` is Click-centric
- B0/B1 invisible Browser Use interceptor ablation: harness docs remain;
  Agent path is the product proof going forward
- RESULTS.md historical A1–A8: do not treat as agent+PUA evidence
- Remote/Jev escalation: interface only (`EscalatingPolicy` + `remote` feature)

## PUA pin

`fe3f1fd3818feb452fae1771ff2171b8598f86e6`
