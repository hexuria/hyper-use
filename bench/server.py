"""Loopback scenario server: static sites plus the ground-truth journal.

Serves ``examples/`` at ``/`` (so ``/acme-mail/site/index.html``) and
``examples/_shared`` at ``/_shared``. Pages post to:

- ``POST /_bench/act``   page-observed actions (click, fill, select, submit, key)
- ``POST /_bench/event`` semantic outcomes (``BENCH.emit``)
- ``POST /_bench/state`` current page state (``BENCH.set``) plus url and title

``GET /_bench/journal`` returns everything since the last ``POST /_bench/reset``.
The harness resets before every attempt. Nothing here trusts an agent.
"""

from __future__ import annotations

import hashlib
import json
import threading
import time
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


class Journal:
    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.reset("")

    def reset(self, attempt: str) -> None:
        with self.lock:
            self.attempt = attempt
            self.acts: list[dict] = []
            self.events: list[dict] = []
            self.pages: dict[str, dict] = {}
            self.last_url = ""
            self.last_title = ""
            self.started = time.time()

    def fingerprint_locked(self) -> str:
        body = json.dumps(
            {"events": self.events, "pages": {k: v.get("state") for k, v in self.pages.items()}, "url": self.last_url},
            sort_keys=True,
        )
        return hashlib.sha256(body.encode()).hexdigest()[:16]

    def fingerprint(self) -> str:
        with self.lock:
            return self.fingerprint_locked()

    def add(self, kind: str, body: dict) -> None:
        with self.lock:
            body["received"] = round(time.time() - self.started, 3)
            if kind == "act":
                # The state the action was taken in: a repeat with the same
                # fingerprint means the previous identical action changed nothing.
                body["fingerprint_before"] = self.fingerprint_locked()
                self.acts.append(body)
            elif kind == "event":
                self.events.append(body)
            elif kind == "state":
                self.pages[body.get("page", "")] = body
                self.last_url = body.get("url", self.last_url)
                self.last_title = body.get("title", self.last_title)

    def snapshot(self) -> dict:
        with self.lock:
            merged: dict = {}
            for page in self.pages.values():
                merged.update(page.get("state") or {})
            return {
                "attempt": self.attempt,
                "acts": list(self.acts),
                "events": list(self.events),
                "state": merged,
                "pages": dict(self.pages),
                "url": self.last_url,
                "title": self.last_title,
                "fingerprint": self.fingerprint_locked(),
            }


class Handler(SimpleHTTPRequestHandler):
    journal: Journal

    def log_message(self, *_args) -> None:  # quiet
        pass

    def end_headers(self) -> None:
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def _json(self, code: int, payload: dict) -> None:
        data = json.dumps(payload).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length) if length else b"{}"
        try:
            body = json.loads(raw or b"{}")
        except json.JSONDecodeError:
            body = {}
        path = self.path.split("?")[0]
        if path == "/_bench/reset":
            self.journal.reset(str(body.get("attempt", "")))
            return self._json(200, {"ok": True})
        kind = {"/_bench/act": "act", "/_bench/event": "event", "/_bench/state": "state"}.get(path)
        if kind is None:
            return self._json(404, {"error": "unknown"})
        self.journal.add(kind, body if isinstance(body, dict) else {})
        self._json(200, {"ok": True})

    def do_GET(self) -> None:  # noqa: N802
        if self.path.split("?")[0] == "/_bench/journal":
            return self._json(200, self.journal.snapshot())
        if self.path in ("/", ""):
            self.send_response(302)
            self.send_header("Location", "/acme-mail/site/index.html")
            self.end_headers()
            return
        super().do_GET()


class BenchServer:
    def __init__(self, root: Path, port: int) -> None:
        self.journal = Journal()
        handler = partial(Handler, directory=str(root))
        Handler.journal = self.journal
        self.httpd = ThreadingHTTPServer(("127.0.0.1", port), handler)
        self.port = self.httpd.server_address[1]
        self.thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)

    @property
    def base(self) -> str:
        return f"http://127.0.0.1:{self.port}"

    def start(self) -> "BenchServer":
        self.thread.start()
        return self

    def stop(self) -> None:
        self.httpd.shutdown()
        self.httpd.server_close()


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description="serve the bench scenarios (for looking at them by hand)")
    parser.add_argument("--port", type=int, default=8765)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1] / "examples"
    server = BenchServer(root, args.port).start()
    print(f"serving {root} at {server.base}  (Ctrl-C to stop)")
    try:
        server.thread.join()
    except KeyboardInterrupt:
        server.stop()
