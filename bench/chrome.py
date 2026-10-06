"""Throwaway Chrome for one attempt: fresh profile, loopback CDP, 1280x800 viewport."""

from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import tempfile
import time
import urllib.request
from itertools import count

import websocket

CHROME = os.environ.get("BENCH_CHROME", "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")


def http_json(url: str, method: str = "GET", timeout: float = 5):
    req = urllib.request.Request(url, method=method)
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read() or b"null")


class Cdp:
    """Minimal blocking CDP client on one websocket."""

    def __init__(self, ws_url: str) -> None:
        self.ws = websocket.create_connection(ws_url, timeout=15, suppress_origin=True)
        self.ids = count(1)

    def call(self, method: str, session: str | None = None, **params):
        msg_id = next(self.ids)
        msg = {"id": msg_id, "method": method, "params": params}
        if session:
            msg["sessionId"] = session
        self.ws.send(json.dumps(msg))
        while True:
            reply = json.loads(self.ws.recv())
            if reply.get("id") == msg_id:
                if "error" in reply:
                    raise RuntimeError(f"{method}: {reply['error'].get('message')}")
                return reply.get("result", {})

    def close(self) -> None:
        try:
            self.ws.close()
        except Exception:
            pass


class Chrome:
    def __init__(self, port: int, width: int = 1280, height: int = 800, headless: bool = False) -> None:
        self.port = port
        self.width = width
        self.height = height
        self.headless = headless
        self.profile = tempfile.mkdtemp(prefix="aui-bench-chrome-")
        self.proc: subprocess.Popen | None = None

    @property
    def http(self) -> str:
        return f"http://127.0.0.1:{self.port}"

    def launch(self, url: str = "about:blank") -> "Chrome":
        args = [
            CHROME,
            f"--user-data-dir={self.profile}",
            f"--remote-debugging-port={self.port}",
            "--remote-debugging-address=127.0.0.1",
            "--remote-allow-origins=*",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--disable-component-update",
            "--disable-sync",
            "--disable-features=Translate,OptimizationHints,MediaRouter",
            "--password-store=basic",
            "--use-mock-keychain",
            f"--window-size={self.width},{self.height + 120}",
            "--window-position=40,40",
        ]
        if self.headless:
            args.append("--headless=new")
        args.append(url)
        self.proc = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
        deadline = time.time() + 20
        while time.time() < deadline:
            try:
                http_json(f"{self.http}/json/version", timeout=1)
                break
            except Exception:
                time.sleep(0.1)
        else:
            raise RuntimeError("Chrome did not expose CDP")
        self.fit_viewport()
        return self

    def browser_ws(self) -> str:
        return http_json(f"{self.http}/json/version")["webSocketDebuggerUrl"]

    def pages(self) -> list[dict]:
        return [t for t in http_json(f"{self.http}/json/list") if t.get("type") == "page"]

    def first_page(self) -> dict:
        for _ in range(50):
            pages = self.pages()
            if pages:
                return pages[0]
            time.sleep(0.1)
        raise RuntimeError("no page target")

    def fit_viewport(self) -> None:
        """Size the window so the page viewport is exactly width x height (CSS px)."""
        page = self.first_page()
        cdp = Cdp(self.browser_ws())
        try:
            win = cdp.call("Browser.getWindowForTarget", targetId=page["id"])
            pcdp = Cdp(page["webSocketDebuggerUrl"])
            try:
                for _ in range(3):
                    inner = pcdp.call("Runtime.evaluate", expression="[innerWidth, innerHeight]", returnByValue=True)
                    iw, ih = inner["result"]["value"]
                    bounds = cdp.call("Browser.getWindowBounds", windowId=win["windowId"])["bounds"]
                    if iw == self.width and ih == self.height:
                        break
                    cdp.call(
                        "Browser.setWindowBounds",
                        windowId=win["windowId"],
                        bounds={"width": bounds["width"] + self.width - iw, "height": bounds["height"] + self.height - ih},
                    )
                    time.sleep(0.3)
            finally:
                pcdp.close()
        finally:
            cdp.close()

    def viewport(self) -> list[int] | None:
        try:
            page = self.first_page()
            pcdp = Cdp(page["webSocketDebuggerUrl"])
            try:
                return pcdp.call("Runtime.evaluate", expression="[innerWidth, innerHeight]", returnByValue=True)["result"]["value"]
            finally:
                pcdp.close()
        except Exception:
            return None

    def version(self) -> str:
        try:
            return http_json(f"{self.http}/json/version").get("Browser", "")
        except Exception:
            return ""

    def kill(self) -> None:
        if self.proc and self.proc.poll() is None:
            try:
                cdp = Cdp(self.browser_ws())
                cdp.call("Browser.close")
                cdp.close()
                self.proc.wait(timeout=5)
            except Exception:
                try:
                    os.killpg(self.proc.pid, signal.SIGKILL)
                except Exception:
                    pass
        shutil.rmtree(self.profile, ignore_errors=True)
