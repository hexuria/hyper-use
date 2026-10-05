"""Shared cua-driver setup for A2 and A4 (runs in bench/envs/cua, mcp<2).

Launch a driver-owned isolated Chrome (browser_prepare), try to size its window
for a 1280x800 viewport, bind it exactly, and open the task's start URL.
"""

from __future__ import annotations

import asyncio
import json
import os
import shutil
import subprocess
import urllib.request
import uuid
from contextlib import asynccontextmanager

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import get_default_environment, stdio_client

CUA = os.path.expanduser(os.environ.get("CUA_DRIVER_BIN", "~/.local/bin/cua-driver"))
TOOLBAR_H = 87  # Chrome tab strip + toolbar on macOS; the harness records the real viewport


def page_viewport(spec: dict):
    """Viewport the start page reported to the bench server (setup only, never task state)."""
    try:
        with urllib.request.urlopen(spec["server"] + "/_bench/journal", timeout=3) as resp:
            pages = json.loads(resp.read())["pages"]
        sizes = [p.get("viewport") for p in pages.values() if p.get("viewport")]
        return max(sizes, key=lambda v: v[0] * v[1]) if sizes else None
    except Exception:
        return None


class CuaError(RuntimeError):
    pass


class Driver:
    def __init__(self, session: ClientSession, label: str) -> None:
        self.session = session
        self.label = label

    async def call(self, name: str, args: dict) -> dict:
        result = await self.session.call_tool(name, {**args, "session": self.label})
        data = result.structuredContent if isinstance(result.structuredContent, dict) else None
        if result.isError or not data or data.get("status") == "refused" or data.get("refusal"):
            text = json.dumps(data) if data else " ".join(getattr(c, "text", "") for c in result.content)
            raise CuaError(f"{name}: {text[:800]}")
        return data


@asynccontextmanager
async def cua_browser(spec: dict, trace):
    env = dict(get_default_environment())
    env.update({k: v for k, v in os.environ.items() if k.startswith("CUA_DRIVER_")})
    params = StdioServerParameters(command=CUA, args=["mcp"], env=env)
    async with stdio_client(params) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            driver = Driver(session, f"hub-{uuid.uuid4().hex[:8]}")
            prepared = await driver.call("browser_prepare", {"allow_launch": True, "profile": {"mode": "isolated_new"}})
            pid = int(prepared["prepared_pid"])
            trace.write("browser", pid=pid)
            window = None
            for _ in range(60):
                windows = (await driver.call("list_windows", {"pid": pid})).get("windows", [])
                visible = [w for w in windows if w.get("is_on_screen")]
                if visible:
                    window = max(visible, key=lambda w: w["bounds"]["width"] * w["bounds"]["height"])
                    break
                await asyncio.sleep(0.25)
            if window is None:
                raise CuaError("isolated browser window did not appear")
            vp = spec["viewport"]
            # The Mac runs AeroSpace (tiling WM), which resizes new windows. Float only this throwaway
            # window (no config change), then size it so the page viewport is 1280x800.
            if shutil.which("aerospace"):
                subprocess.run(["aerospace", "layout", "floating", "--window-id", str(window["window_id"])],
                               capture_output=True, timeout=5)
                await asyncio.sleep(0.3)
            frame = {"width": vp["width"], "height": vp["height"] + TOOLBAR_H}

            async def set_frame() -> None:
                try:
                    await driver.call("set_window_frame", {"pid": pid, "window_id": window["window_id"], "x": 40, "y": 40, **frame})
                except CuaError as error:
                    trace.write("note", text=f"set_window_frame refused: {error}"[:300])

            await set_frame()
            bound = await driver.call("get_browser_state", {"pid": pid, "window_id": window["window_id"]})
            target_id = bound["target_id"]
            tabs = bound.get("tabs") or []
            tab_id = next((t for t in tabs if t.get("active")), tabs[0])["tab_id"]
            await driver.call("browser_navigate", {"target_id": target_id, "tab_id": tab_id, "url": spec["start_url"]})
            await asyncio.sleep(0.8)
            for _ in range(3):  # correct the frame from the viewport the page itself reports
                seen = page_viewport(spec)
                if not seen or seen == [vp["width"], vp["height"]]:
                    break
                frame = {"width": frame["width"] + vp["width"] - seen[0], "height": frame["height"] + vp["height"] - seen[1]}
                await set_frame()
                await asyncio.sleep(0.6)
            trace.write("viewport", value=page_viewport(spec))
            try:
                yield driver, {"pid": pid, "window_id": window["window_id"], "target_id": target_id, "tab_id": tab_id}
            finally:
                try:
                    await driver.call("kill_app", {"pid": pid})
                except Exception:
                    pass


def compact_snapshot(snap: dict, limit_refs: int = 220) -> str:
    """Model-facing view of a semantic_v2 snapshot: page, actionable refs, outline."""
    page = snap.get("page") or {}
    lines = [f"URL: {page.get('url')}  TITLE: {page.get('title')}"]
    refs = snap.get("refs") or []
    lines.append('ACTIONABLE REFS (ref role "name" [value] [states] {visibility} actions):')
    for r in refs[:limit_refs]:
        name = r.get("name")
        val = r.get("value")
        states = r.get("states") or {}
        flags = ",".join(f"{k}={v}" for k, v in states.items() if k in {"checked", "disabled", "expanded", "selected", "pressed"})
        lines.append(f"{r.get('ref')} {r.get('role')} {json.dumps(name) if name else '-'}"
                     f"{' value=' + json.dumps(val) if val not in (None, '') else ''}"
                     f"{' [' + flags + ']' if flags else ''} {{{r.get('visibility')}}} {','.join(r.get('actions') or [])}")
    if len(refs) > limit_refs:
        lines.append(f"... {len(refs) - limit_refs} more refs not shown")
    outline = snap.get("outline")
    if isinstance(outline, str):
        lines.append("OUTLINE:\n" + outline[:6000])
    return "\n".join(lines)
