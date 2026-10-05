# Live drive (manual, opt-in)

A hand-run check that drives a real Chrome tab through `hyper-use mcp`, with
JEV (typesafe.ai) choosing each next tool and its arguments. It is not part of
CI and it is not a benchmark: seven or eight tasks on one static page say
nothing statistical. Use it to read transcripts and find bugs.

## What is here

- `site/`: a static, Gmail-style test page ("Acme Mail", inline SVG, no
  network). `index.html` has a hash-routed inbox list (`#inbox`) and thread
  view (`#thread/<id>`), a docked Compose sheet with "Add Cc", and a quick
  reply box whose "Send" twins the Compose "Send". The thread toolbar has a
  hidden and an offscreen "Archive" twin. `settings.html` has a disabled and
  an enabled "Save". `help.html` is the link target that changes URL and title.
- `crates/hyper-use-cli/examples/live_drive.rs`: the harness. It needs the
  `jev` feature.

## Run

```sh
python3 -m http.server 8765 --bind 127.0.0.1 --directory examples/live-drive/site &
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  --remote-debugging-port=9333 --remote-debugging-address=127.0.0.1 \
  --user-data-dir=/tmp/hyper-use-live/chrome-profile --window-size=1280,800 &
cargo build --release -p hyper-use-cli --bin hyper-use
cargo build --release -p hyper-use-cli --features jev --example live_drive
TYPESAFE_API_KEY=... HYPER_USE_JEV=1 \
  ./target/release/examples/live_drive --bin target/release/hyper-use
```

Flags: `--site` (default `http://127.0.0.1:8765`), `--cdp` (default
`http://127.0.0.1:9333`), `--out` (default `/tmp/hyper-use-live`), `--only
<task id>`, `--screenshot-only` (inbox and thread at 1280 and 1440 wide,
Compose open, settings, help).

Each task gets a fresh tab and a fresh `hyper-use mcp` child attached to that
tab's page websocket. The harness writes one JSONL transcript per task to
`<out>/transcripts/` and a `summary.json`, and records page ground truth
(URL, title, snackbar text, Compose/thread/Cc visibility) read straight from
the page after the run.

## What JEV is shown

The state JEV answers from includes the observed regions (with their
`state`), the last locate's top three candidates and its `signals`, and a
history of call summaries. When the last locate carried `repeated_query`,
the position options name the candidate each suggested position would pick.
The option order does not change, and the harness does not pick for JEV.

## Limits

- The caller is the harness plus JEV, not a full agent. JEV only picks from
  options the harness offers: strings quoted in the task and labels from the
  last observe. A run stops after 14 steps.
- The test page is ours, so it can be tuned to pass. Read failures as hints,
  not as rates.
- Use a throwaway Chrome profile bound to 127.0.0.1. Shut it down afterwards.
