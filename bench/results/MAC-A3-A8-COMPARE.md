# Mac live A3 vs A8 — acme-mail (fair parity)

- **Host:** Uriah Mac (`c9f646c1-e243-4545-8e52-3b063cb6c0cd`), tip `acb3a33`, Luna `http://127.0.0.1:8080` HTTP 200
- **Run id:** `20261006-081905` (resumed after mid-suite `restart_daemon` hangs)
- **Raw:** `bench/runs/20261006-081905/results.jsonl` (local/gitignored; not in this commit)
- **Arms:**
  - **A3** = jev-ultrafast (JEV decider + Luna-for-text via monkeypatch) + browser_harness
  - **A8** = Luna planner + JEV tie-break + ultra-instinct (`mcp` observe/locate/guard) + Browser Use tools (type/select/scroll/fallback)
- **Why not A5:** A5 is JEV+ultra-instinct presses only (no Luna). Not parity.
- **Seed / tries:** 42 / max-tries 1; caps steps=20 model_calls=40 wall_s=180
- **Vendor:** `bench/vendor/jev-ultrafast` @ `1231850a0bf1a0c0341fe408ef1668dbbfdfac46`
- **Harness note:** Mac `browser_harness.admin.restart_daemon` hung (20s TimeoutExpired) after early A3 attempts and dropped results. Resilience patches (this PR): `bench/run.py` restart_daemon best-effort timeout 60s; `bench/arms/a3_jev_ultrafast.py` finally restart bounded to 15s. Several A3 outcomes still show `crashed` with `_IPCResponseTimeout` on the daemon even when the page journal scored PASS.

## Per-task

| Task | Class | A3 | A8 |
|---|---|---|---|
| `am-archive` | normal/[] | **PASS** · 26.26s · c=1 · crashed | **PASS** · 9.55s · c=2 · finished |
| `am-compose-send` | normal/['type'] | fail · 22.39s · c=1 · crashed | **PASS** · 56.77s · c=4 · finished |
| `am-enabled-save` | normal/[] | **PASS** · 30.62s · c=2 · crashed | **PASS** · 11.61s · c=3 · finished |
| `am-reply` | normal/['type'] | fail · 22.42s · c=1 · crashed | **PASS** · 26.42s · c=5 · finished |
| `am-star` | normal/[] | **PASS** · 18.3s · c=4 · finished | **PASS** · 14.48s · c=3 · finished |
| `am-tm-print` | target-missing/[] | **PASS** · 16.78s · c=1 · finished | **PASS** · 102.59s · c=7 · finished |
| `am-twin-send` | normal/[] | **PASS** · 39.1s · c=1 · crashed | **PASS** · 13.4s · c=3 · finished |

## Aggregates

| Arm | Full | Press-only | Pass wall mean (all PASS) | Finished-only pass wall mean |
|---|---|---|---|---|
| **A3** jev-ultrafast | **5/7** | **5/5** | 26.21s | full 17.54s [18.3, 16.78] / press 17.54s [18.3, 16.78] |
| **A8** Luna+JEV+ultra-instinct+BU | **7/7** | **5/5** | 33.55s | full 33.55s [9.55, 56.77, 11.61, 26.42, 14.48, 102.59, 13.4] / press 30.33s [9.55, 11.61, 14.48, 102.59, 13.4] |

## Winners

- **Accuracy:** **A8** (7/7 vs A3 5/7). A3 failed both type tasks (`am-compose-send`, `am-reply`) with `_IPCResponseTimeout: Input.dispatchMouseEvent timed out after 5s waiting for the daemon` — harness daemon flakiness, not a Luna HTTP 400 this time (Luna stayed 200).
- **Speed:** Mixed. On finished-only press tasks A3 mean ~17.5s (n=2: star, tm-print) vs A8 press finished mean ~30.3s (skewed by `am-tm-print` 102.59s safe-refusal). Excluding that outlier, A8 press finished walls are ~9.6–14.5s — competitive or faster than A3. A8 also completed both type tasks (compose 56.8s, reply 26.4s); A3 did not.
- **Parity takeaway:** With Luna+JEV on both sides, **ultra-instinct stack (A8) matches or beats jev-ultrafast on accuracy** on this Acme set; A3’s remaining misses look like browser_harness IPC hangs more than decision quality.

## Fail / crash notes

- **A3/am-archive:** pass=True outcome=crashed final='error' note='_IPCResponseTimeout: Input.dispatchMouseEvent timed out after 5s waiting for the daemon' unmet=[]
- **A3/am-compose-send:** pass=False outcome=crashed final='error' note='_IPCResponseTimeout: Input.dispatchMouseEvent timed out after 5s waiting for the daemon' unmet=["event {'type': 'send', 'source': 'compose', 'to': 'dana@acme.test', 'subject': 'Offsite budget', 'body': '~Draft numbers attached'}"]
- **A3/am-enabled-save:** pass=True outcome=crashed final='error' note='_IPCResponseTimeout: Runtime.evaluate timed out after 5s waiting for the daemon' unmet=[]
- **A3/am-reply:** pass=False outcome=crashed final='error' note='_IPCResponseTimeout: Input.dispatchMouseEvent timed out after 5s waiting for the daemon' unmet=["event {'type': 'send', 'source': 'reply', 'thread': 'q3', 'body': '~On it, thanks'}"]
- **A3/am-tm-print:** pass=True outcome=finished final='give_up' note='JEV chose BLOCKED' unmet=[]
- **A3/am-twin-send:** pass=True outcome=crashed final='error' note='_IPCResponseTimeout: Input.dispatchMouseEvent timed out after 5s waiting for the daemon' unmet=[]
- **A8/am-tm-print:** pass=True outcome=finished final='give_up' note='The inbox page does not expose a visible “Print all” button or print control. I checked the inbox toolbar options, but they did not reveal one, so I cannot print all messages using this site.' unmet=[]

## Compare to prior unfair A3 vs A5

Prior Mac A3 vs A5 (`20261006-074743`): A3 4/7, A5 2/7 — A5 could not type. Fair A8 restores typing via Luna+BU and reaches **7/7**.

