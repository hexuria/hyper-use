---
name: ultra-instinct
description: Drive a live Chrome browser with a plain-language goal using the ultra-instinct CLI. Use when the user asks to click, type, fill, navigate, or automate anything in a browser — e.g. "/ultra-instinct book a flight" or "submit this form for me".
---

# ultra-instinct — run a browser goal in plain language

The user gives a goal in their own words. You never need them to know CLI flags.
Your job: get the `aui` binary, point it at a Chrome with CDP on, and run ONE
command with their words verbatim.

## 1. Make sure `aui` exists

Inside a checkout of `hexuria/ultra-instinct`:

```bash
command -v aui || bash scripts/install-aui.sh
```

Anywhere else (no checkout):

```bash
command -v aui || curl -fsSL https://raw.githubusercontent.com/hexuria/ultra-instinct/main/scripts/install-aui.sh | bash
```

`install-aui.sh` is idempotent: it no-ops when `aui` is already on PATH,
builds from this repo when present, and otherwise clones
`hexuria/ultra-instinct` into `~/.cache/ultra-instinct` and `cargo install`s it
(features `jev clef model-text` — the full policy surface). It `cd`s into the
source first so the repo's pinned toolchain applies regardless of your cwd.
If it fails on a missing Rust toolchain, tell the user to run rustup. If the
installed `aui` predates a policy you need (`run` errors that the policy is
unavailable), reinstall with `AUI_FORCE=1`.

## 2. Make sure Chrome answers CDP

```bash
curl -s http://127.0.0.1:9222/json/version >/dev/null
```

If nothing answers, launch Chrome yourself:

```bash
google-chrome --remote-debugging-port=9222 &
```

Gotcha: if a Chrome is already running WITHOUT the debug port, a new
`google-chrome` call hands off to the existing process and the port never
opens. Either close that Chrome first, or launch a separate profile:

```bash
google-chrome --remote-debugging-port=9222 --user-data-dir=/tmp/aui-chrome &
```

(Any port works — pass it via `--cdp http://127.0.0.1:<port>`.)

## 3. Run the goal — verbatim

```bash
aui run --goal "<the user's words, unchanged>" --cdp
```

- If the goal names a site or URL, add `--url <url>`: the CLI opens an owned
  tab for the run and closes it after.
- Multi-step goals go in as ONE `--goal` string ("fill X then click Y") — the
  CLI splits clauses itself. Do not pre-split into several `aui` calls.
- Optional bounds: `--max-steps N` (default 20), `--wait-secs N` (default 30).

### Policy — pick by the keys that exist, don't ask

- `TYPESAFE_API_KEY` set → no flag needed; `jev` is the default.
- `CLOUDFLARE_ACCOUNT_ID` + `CLOUDFLARE_API_TOKEN` set → `--policy clef-flash`.
- Neither → `--policy instinct` (fully offline, deterministic, abstains
  rather than guessing).
- Optional `--text-model-cmd <program>` makes a model write TYPE_TEXT/SELECT
  payloads only; the choice of action is never model-written.

## 4. Report back

Each step prints as it acts. The outcome is the line:

```
outcome <done|blocked|abstained|failed>: <reason>
policy_calls N stale_discards N
```

`blocked` and `abstained` are real answers — the agent refusing to guess —
not crashes: say what it couldn't find instead of re-running blindly.
`failed` means the run itself errored (e.g. missing env key, CDP lost) — the
reason says which. There is no wall-time line; wrap in `time aui run ...` if
you want one.

## Hard rules

- Goal text goes to `--goal` verbatim — quote it, don't paraphrase.
- Never pass selectors, coordinates, or element ids from outside; the agent
  picks targets itself and can only choose offered regions (closed wire).
- Never retry a `blocked`/`abstained` outcome by re-running the identical
  command — change the page state or the goal first.
