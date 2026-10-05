# Impeccable audit — Agent + PUA + ticket path

Current note, 2026-10-06 (Asia/Manila). Baseline reviewed: `87ffc2d` (main
after PR #17). Read-only impeccable-rust audit of the post-pivot path
(ADR 0001–0004), then remediation R1–R6 on `feat/audit-r1-r6`. After
remediation: `cargo test --workspace` 287 passed, 0 failed (36 ignored: live
Chrome / paid remote). It is not a proof.

This supersedes the pre-pivot note (baseline `9baa21e`, observe / MCP
preflight / legacy executor), which is in git history. Its findings on the
MCP preflight surface (gate rounding, confidence range, text-miss cap,
`repeated_query`, CDP endpoint resolution) still hold and still have their
owner tests; they are not repeated here.

`#![forbid(unsafe_code)]` stays on every crate. Rust is pinned to 1.99.0 and
every crate is `publish = false`.

## Scope

```text
observe → ActionSpace (with_front_layer) → PuaPolicy / RemotePolicy
  → TextResolver (TYPE_TEXT / SELECT) → gate → ActionTicket
  → execute_ticketed: ledger → kind == ticket.action → fresh observe
      → revalidate (of_target world + target role/label/fp) → gate
      → mark consumed → dispatch(ticket.target_id)
  → settle → observe → diff / value check → history
```

Files: `hyper-use-agent/src/{agent,executor,runtime,verify_map}.rs`,
`hyper-use-guard/src/{gate,ticket,world}.rs`, `hyper-use-policy/src/{pua_policy,text,remote,goal}.rs`,
`hyper-use-protocol/src/ticket.rs`.

## Findings and remediation

| ID | Class | Finding at `87ffc2d` | Fix | Owner |
| --- | --- | --- | --- | --- |
| R1 | REQUIRED | `consume_ticket_once` pressed **then** marked consumed; a failed press left the lease reusable, contrary to its own doc and to ADR 0003 §1 | consume before press, same order as `execute_ticketed` ([ADR 0005](adr/0005-audit-remediation-consume-order-single-barrier.md)) | `consume_ticket_once_marks_consumed_before_press_so_failed_press_cannot_retry`, `consume_ticket_once_stale_does_not_consume_or_press` |
| R2 | REQUIRED | `Predicted::{observation_fingerprint, text_fingerprint}` were write-only: implied a second pre-ticket barrier that did not exist | removed; `Predicted` doc states the ticket is the single barrier (ADR 0005 §2) | compile-time (fields gone); executor props |
| R3 | REQUIRED | `AGENTS.md` still said "not an agent / no click / MCP is the surface" | rewritten to PRD / ADR 0001 with the anti-drift block | review |
| R4 | USEFUL | this file described the pre-pivot path | superseded by this note | — |
| R5 | USEFUL | no boundary test for `NEIGHBOR_RADIUS_PX`, no focus-only test under `of_target`, stale vs consumed unpinned | property + adversarial tests | `guard/tests/ticket_props.rs`, `agent/tests/props.rs` |
| R6 | USEFUL | no mutation testing on the safety boundary | nightly + manual cargo-mutants workflow | `.github/workflows/mutants.yml` |

Intentional, not a finding: MCP ranked `guard()` keeps the float `0.55` /
`0.05` gate for the historical A5 / A6 arms. The agent path never calls it.
It retires with A5 / A6.

## Failure class and owner (agent path)

| Failure class | Owner |
| --- | --- |
| Executed target not from the decided observation's action space | `agent.rs` off-menu / kind-mismatch guard; `adversarial.rs` |
| Hidden / disabled / readonly / occluded / front-layer / offscreen target executes | `gate.rs` unit tests; executor re-gate on fresh region |
| Target substituted or operation swapped at the executor | `executor.rs` `operation_swap_is_refused_without_input`; `props.rs` substitution prop |
| Stale ticket executes (rerender, modal, target gone, nearby peer) | `props.rs` `stale_never_executes_and_target_cannot_be_substituted`; `executor.rs` modal / mutated tests |
| Ticket replayed after success, page rejection, or failed press | `executor.rs` `executes_exact_ticket_target_once`, `page_rejection_consumes_ticket`; `props.rs` `consumed_beats_stale_at_the_executor`; R1 tests in `ticket.rs` |
| Spent lease misread as stale (silent retry) | `props.rs` `is_stale_is_exactly_world_or_target_drift`; `ticket_props.rs` `consumed_ticket_is_never_classified_stale` |
| Neighborhood radius off-by-one (159 / 160 / 161) | `ticket_props.rs` boundary test + `neighborhood_inclusion_is_distance_le_radius`, `new_peer_is_stale_iff_inside_radius` |
| Parented neighborhood (same-parent sibling far away, foreign row nearby) | `ticket_props.rs` parented tests |
| Focus-only change not detected under `of_target` | `ticket_props.rs` `focus_only_change_is_world_changed_under_of_target` |
| Ticket attribute check weakened (role / label / fingerprint) | `ticket.rs` `revalidate_checks_role_label_and_fingerprint_independently` |
| World fingerprint collisions | `ticket.rs` `world_fingerprint_has_no_collisions_across_distinct_worlds` |
| PUA abstention becomes input | `props.rs` `abstain_never_executes` |
| Text payload from PUA / wrong context | `policy/src/text.rs` tests; agent context-fingerprint check |
| Remote reply carries selector / coordinates / script / off-menu id | `policy/src/remote.rs` tests (feature `remote`) |
| Concurrency, crash recovery, unsafe | none needed (single-threaded blocking CDP, no recovery protocol, no `unsafe`) |

## Mutation testing

Targets (R6): `crates/hyper-use-guard/src/gate.rs`, `ticket.rs`,
`crates/hyper-use-agent/src/executor.rs`, and in
`crates/hyper-use-guard/src/world.rs` only `of_target` / `neighborhood_of` /
`nearby`. cargo-mutants 27.1.0, default test scope (the mutated file's
package).

| Run | Mutants | Caught | Missed | Unviable |
| --- | --- | --- | --- | --- |
| gate + ticket + executor, at `87ffc2d` + R1/R2 | 37 | 25 | 8 | 4 |
| gate + ticket + executor, after R5 tests | 36 (1 excluded) | 32 | 0 | 4 |
| world `of_target` set, at `87ffc2d` | 14 | 8 | 4 | 2 |
| world `of_target` set, after R5 tests | 13 | 11 | 0 | 2 |

Misses closed: `ExecError` / `ConsumeError` `Display` and `source` (exact
string tests); `revalidate` `||` → `&&` (independent role / label /
fingerprint checks); FNV per-byte `^=` → `|=` and separator `^=` → `&=`
(no-collision test); same-parent sibling match guard (parented neighborhood
tests). The redundant `(None, None) => {}` arm in `neighborhood_of` was an
equivalent mutant and was removed.

Excluded as equivalent (`.cargo/mutants.toml`): the `mix` segment separator
`*hash ^= 0xff` → `|= 0xff`. It is still a separator; distinguishing it needs
a 64-bit collision no cheap deterministic test can build.

Run locally (output outside the tree; ~2–3 min on 8 cores):

```sh
cargo install cargo-mutants --locked   # once
cargo mutants --no-shuffle -j 3 --timeout 180 -o /tmp/hu-mut-a \
  -f crates/hyper-use-guard/src/gate.rs \
  -f crates/hyper-use-guard/src/ticket.rs \
  -f crates/hyper-use-agent/src/executor.rs
cargo mutants --no-shuffle -j 3 --timeout 180 -o /tmp/hu-mut-b \
  -f crates/hyper-use-guard/src/world.rs --re 'of_target|neighborhood_of|nearby'
```

Exit code 0 = all caught; 2 = missed mutants (see `mutants.out/missed.txt`);
3 = timeouts. CI: `.github/workflows/mutants.yml` runs both sets nightly at
02:00 Asia/Manila and on `workflow_dispatch`; it is **not** a PR check
(it rebuilds per mutant). Results upload as the `mutants-*` artifacts.

## What was run

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` (287 passed, 36 ignored).
- `cargo test -p hyper-use-resonance --features hgra`,
  `cargo check -p hyper-use-cli --features jev`,
  `cargo test -p hyper-use-policy -p hyper-use-agent --features hyper-use-agent/remote,hyper-use-policy/remote`.
- cargo-mutants as above.

## Deliberately skipped

- Loom, Kani, TLA+, Miri, Lean: **NOT JUSTIFIED** for this path. No `unsafe`;
  the only atomic is the ticket-id counter (`Relaxed`, uniqueness only); the
  CDP client is blocking and single-threaded; there is no recovery protocol.
- cargo-fuzz: not a CI job. Structured and garbage proptests own CDP JSON and
  tools/call shapes at small case counts.
- cargo-semver-checks / cargo-public-api: not justified until a crate is
  published. R2 removes two public fields; API is 0.1.
- Live Chrome and paid remote stay `#[ignore]`; CI uses CDP replay fixtures.

## Deferred (out of scope for R1–R6)

- Live A/B/C/D on pinned main (needs jev-ultrafast / paid remote).
- Model-backed `TextResolver` — done after this audit behind `model-text`
  ([ADR 0006](adr/0006-model-text-resolver.md)).
- cross-origin iframe / closed shadow / off-window virtualized inventory (ADR 0007 covers same-origin pierce, open shadow, visible window, autocomplete).
- Retiring the MCP float gate with A5 / A6.
- Known limits in `docs/DECISIONS.md` ("Known limits, not fixed").
