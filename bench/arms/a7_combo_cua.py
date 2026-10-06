"""A7: GPT 6 Luna + JEV + ultra-instinct + CUA driver, all on one driver-launched isolated Chrome.

cua-driver launches its own isolated Chrome (browser_prepare isolated_new), same window
float/size routine as A2/A4. The arm reads that Chrome's loopback DevTools port (lsof on
the driver-owned pid) and points ultra-instinct at it, so ultra-instinct and CUA act on the same
tab. Luna plans, ultra-instinct observes/locates/presses, JEV breaks ties, and CUA's typed
browser tools execute type/select/scroll/read and the gated fallback click by semantic_v2
ref. Protocol: ``bench/arms/COMBO.md``. Runs in the main bench env (the tiny stdio MCP
client talks to ``cua-driver mcp``; no mcp SDK needed).
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
import time
import urllib.request
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from arms.combo import Combo  # noqa: E402
from arms.common import Trace, load_spec  # noqa: E402
from arms.mcp_stdio import McpStdio  # noqa: E402

CUA = os.path.expanduser(os.environ.get("CUA_DRIVER_BIN", "~/.local/bin/cua-driver"))
TOOLBAR_H = 87
TEXT_ROLES = {"textbox", "searchbox", "textarea"}
SELECT_ROLES = {"combobox", "listbox", "popupbutton", "menulist"}
VISIBLE = {"in_viewport", "near_viewport", None}


class CuaError(RuntimeError):
    pass


class Driver:
    def __init__(self) -> None:
        self.mcp = McpStdio([CUA, "mcp"], env={k: v for k, v in os.environ.items() if k != "TYPESAFE_API_KEY"})
        self.label = f"hub-{uuid.uuid4().hex[:8]}"

    def call(self, name: str, args: dict) -> dict:
        raw = self.mcp.call(name, {**args, "session": self.label})
        err = raw.startswith("ERROR: ")
        try:
            data = json.loads(raw.removeprefix("ERROR: "))
        except json.JSONDecodeError:
            data = None
        if err or not isinstance(data, dict) or data.get("status") == "refused" or data.get("refusal"):
            raise CuaError(f"{name}: {raw[:600]}")
        return data


def page_viewport(server: str):
    try:
        with urllib.request.urlopen(server + "/_bench/journal", timeout=3) as resp:
            pages = json.loads(resp.read())["pages"]
        sizes = [p.get("viewport") for p in pages.values() if p.get("viewport")]
        return max(sizes, key=lambda v: v[0] * v[1]) if sizes else None
    except Exception:
        return None


def devtools_port(pid: int) -> int | None:
    """Loopback DevTools port of the driver-owned isolated Chrome (it starts with --remote-debugging-port=0)."""
    for _ in range(40):
        out = subprocess.run(["lsof", "-nP", "-a", "-p", str(pid), "-iTCP", "-sTCP:LISTEN"], capture_output=True, text=True).stdout
        for port in re.findall(r"127\.0\.0\.1:(\d+) \(LISTEN\)", out):
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{port}/json/version", timeout=2) as resp:
                    if "webSocketDebuggerUrl" in resp.read().decode():
                        return int(port)
            except Exception:
                continue
        time.sleep(0.25)
    return None


class CuaExecutor:
    name = "CUA driver"

    def __init__(self, driver: Driver, target: dict) -> None:
        self.d = driver
        self.target = target

    def snapshot(self) -> dict:
        return self.d.call("get_browser_state", {**self.target, "snapshot_format": "semantic_v2"})

    @staticmethod
    def desc(r: dict) -> str:
        states = r.get("states") or {}
        flags = ",".join(f"{k}={v}" for k, v in states.items() if k in {"checked", "disabled", "expanded", "selected", "pressed", "editable"})
        val = r.get("value")
        return f"{r.get('role')} {json.dumps(r.get('name'))}{' value=' + json.dumps(val) if val not in (None, '') else ''}{' [' + flags + ']' if flags else ''}"

    def elements(self, kind: str) -> list[dict]:
        out = []
        for r in self.snapshot().get("refs") or []:
            if not r.get("ref") or r.get("visibility") not in VISIBLE:
                continue
            role, states, actions = r.get("role"), r.get("states") or {}, r.get("actions") or []
            editable = role in TEXT_ROLES or (states.get("editable") and role not in {"generic"})
            k = "type" if editable and role not in SELECT_ROLES else "select" if role in SELECT_ROLES else "click" if "click" in actions else None
            if role == "combobox" and editable:
                k = "type"
            if k == kind:
                out.append({"id": r["ref"], "desc": self.desc(r), "context": ""})
        return out

    def _do(self, name: str, args: dict) -> str:
        try:
            res = self.d.call(name, {**self.target, **args})
            return json.dumps({k: res.get(k) for k in ("effect", "status", "route", "escalation") if k in res})[:300]
        except CuaError as error:
            return f"driver refused: {str(error)[:300]}"

    def type(self, el: dict, text: str) -> str:
        return self._do("browser_type", {"ref": el["id"], "text": text, "replace": True})

    def select(self, el: dict, option: str) -> str:
        # cua-driver 0.23 has no select tool. Per-character key events (mode=keystrokes, CDP
        # Input.dispatchKeyEvent) into the focused <select> drive Chrome's own type-to-select.
        # Reported as a limitation in COMBO.md.
        return self._do("browser_type", {"ref": el["id"], "text": option, "replace": False, "mode": "keystrokes"})

    def click(self, el: dict) -> str:
        return self._do("browser_click", {"ref": el["id"], "input_route": "dom_event"})

    def scroll(self, down: bool) -> str:
        return self._do("browser_pointer", {"action": "scroll", "x": 640, "y": 400, "delta_y": 600 if down else -600})

    def read(self) -> str:
        snap = self.snapshot()
        page = snap.get("page") or {}
        lines = [f"URL: {page.get('url')}  TITLE: {page.get('title')}"]
        for r in (snap.get("refs") or [])[:220]:
            lines.append(f"{r.get('ref')} {self.desc(r)} {{{r.get('visibility')}}} {','.join(r.get('actions') or [])}")
        outline = snap.get("outline")
        if isinstance(outline, str):
            lines.append("OUTLINE:\n" + outline[:6000])
        return "\n".join(lines)


def main() -> None:
    spec = load_spec()
    trace = Trace(spec["trace"])
    driver = None
    pid = None
    try:
        driver = Driver()
        prepared = driver.call("browser_prepare", {"allow_launch": True, "profile": {"mode": "isolated_new"}})
        pid = int(prepared["prepared_pid"])
        trace.write("browser", pid=pid)
        window = None
        for _ in range(60):
            windows = driver.call("list_windows", {"pid": pid}).get("windows", [])
            visible = [w for w in windows if w.get("is_on_screen")]
            if visible:
                window = max(visible, key=lambda w: w["bounds"]["width"] * w["bounds"]["height"])
                break
            time.sleep(0.25)
        if window is None:
            raise CuaError("isolated browser window did not appear")
        vp = spec["viewport"]
        if shutil.which("aerospace"):
            subprocess.run(["aerospace", "layout", "floating", "--window-id", str(window["window_id"])], capture_output=True, timeout=5)
            time.sleep(0.3)
        frame = {"width": vp["width"], "height": vp["height"] + TOOLBAR_H}

        def set_frame() -> None:
            try:
                driver.call("set_window_frame", {"pid": pid, "window_id": window["window_id"], "x": 40, "y": 40, **frame})
            except CuaError as error:
                trace.write("note", text=f"set_window_frame refused: {error}"[:300])

        set_frame()
        bound = driver.call("get_browser_state", {"pid": pid, "window_id": window["window_id"]})
        tabs = bound.get("tabs") or []
        target = {"target_id": bound["target_id"], "tab_id": next((t for t in tabs if t.get("active")), tabs[0])["tab_id"]}
        driver.call("browser_navigate", {**target, "url": spec["start_url"]})
        time.sleep(0.8)
        for _ in range(3):
            seen = page_viewport(spec["server"])
            if not seen or seen == [vp["width"], vp["height"]]:
                break
            frame = {"width": frame["width"] + vp["width"] - seen[0], "height": frame["height"] + vp["height"] - seen[1]}
            set_frame()
            time.sleep(0.6)
        trace.write("viewport", value=page_viewport(spec["server"]))
        port = devtools_port(pid)
        if port is None:
            raise CuaError("could not find the isolated Chrome's DevTools port")
        cdp = f"http://127.0.0.1:{port}"
        trace.write("note", text=f"ultra-instinct attached to the CUA browser at {cdp}")
        Combo(spec, trace, CuaExecutor(driver, target), cdp).run()
    except Exception as error:  # noqa: BLE001
        trace.final("error", f"{type(error).__name__}: {error}"[:400])
        raise
    finally:
        if driver is not None:
            if pid:
                try:
                    driver.call("kill_app", {"pid": pid})
                except Exception:
                    pass
            driver.mcp.close()


if __name__ == "__main__":
    main()
