"""A8: GPT 6 Luna + JEV + hyper-use + Browser Use, all on the harness Chrome.

Luna plans, hyper-use observes/locates/presses over CDP, JEV breaks ties, and Browser Use
0.13 (a ``BrowserSession`` on the same CDP endpoint plus its ``Tools`` registry, no Browser
Use LLM agent) executes type/select/scroll/read and the gated fallback click by its own
element index. Protocol: ``bench/arms/COMBO.md``.
"""

from __future__ import annotations

import asyncio
import sys
import threading
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from arms.combo import Combo  # noqa: E402
from arms.common import Trace, load_spec  # noqa: E402

TEXT_INPUT_TYPES = {None, "", "text", "email", "search", "tel", "url", "password", "number", "date", "time"}
CONTAINER_TAGS = {"li", "tr", "article", "form", "fieldset", "section", "dialog", "aside", "label"}


class BrowserUseExecutor:
    name = "Browser Use"

    def __init__(self, cdp_http: str) -> None:
        self.cdp = cdp_http
        self.loop = asyncio.new_event_loop()
        threading.Thread(target=self.loop.run_forever, daemon=True).start()
        self.session = None
        self.tools = None

    def run(self, coro, timeout: float = 60):
        return asyncio.run_coroutine_threadsafe(coro, self.loop).result(timeout=timeout)

    async def _ensure(self):
        if self.session is None:
            from browser_use import Browser
            from browser_use.tools.service import Tools

            self.session = Browser(cdp_url=self.cdp, keep_alive=True)
            await self.session.start()
            self.tools = Tools()
        return self.session

    async def _state(self):
        session = await self._ensure()
        return await session.get_browser_state_summary(include_screenshot=False)

    @staticmethod
    def kind_of(node) -> str:
        tag = (node.tag_name or "").lower()
        attrs = node.attributes or {}
        if tag == "select":
            return "select"
        if tag == "textarea" or (tag == "input" and (attrs.get("type") or "").lower() in TEXT_INPUT_TYPES) \
                or (attrs.get("contenteditable") in ("", "true")) or (attrs.get("role") in ("textbox", "searchbox")):
            return "type"
        return "click"

    @staticmethod
    def context_of(node) -> str:
        cur, depth = node.parent_node, 0
        while cur is not None and depth < 8:
            tag = (cur.tag_name or "").lower()
            role = (cur.attributes or {}).get("role")
            if tag in CONTAINER_TAGS or role in ("row", "listitem", "dialog", "gridcell", "group", "region"):
                try:
                    return " ".join(cur.get_all_children_text().split())[:220]
                except Exception:
                    return ""
            cur, depth = cur.parent_node, depth + 1
        return ""

    def describe(self, node) -> str:
        attrs = node.attributes or {}
        name = None
        try:
            name = node.ax_node.name if node.ax_node else None
        except Exception:
            name = None
        keep = {k: attrs[k] for k in ("aria-label", "placeholder", "name", "type", "value", "title", "role", "id", "aria-checked",
                                     "aria-pressed", "aria-selected", "disabled") if k in attrs}
        try:
            text = " ".join(node.get_all_children_text(max_depth=3).split())[:100]
        except Exception:
            text = ""
        return f"<{node.tag_name} {keep}> name={name!r} text={text!r}"

    def elements(self, kind: str) -> list[dict]:
        state = self.run(self._state())
        out = []
        for index, node in (state.dom_state.selector_map or {}).items():
            if self.kind_of(node) != kind:
                continue
            out.append({"id": index, "desc": self.describe(node), "context": self.context_of(node)})
        return out

    def _do(self, action: str, params: dict) -> str:
        async def go():
            await self._ensure()
            res = await self.tools.registry.execute_action(action, params, browser_session=self.session)
            return f"error: {res.error}" if getattr(res, "error", None) else str(getattr(res, "extracted_content", "") or "ok")[:300]
        return self.run(go())

    def type(self, el: dict, text: str) -> str:
        return self._do("input", {"index": el["id"], "text": text, "clear": True})

    def select(self, el: dict, option: str) -> str:
        return self._do("select_dropdown", {"index": el["id"], "text": option})

    def click(self, el: dict) -> str:
        return self._do("click", {"index": el["id"]})

    def scroll(self, down: bool) -> str:
        return self._do("scroll", {"down": down, "pages": 1.0})

    def read(self) -> str:
        state = self.run(self._state())
        try:
            text = state.dom_state.llm_representation()
        except Exception as error:  # noqa: BLE001
            text = f"(no representation: {error})"
        return f"URL: {state.url}  TITLE: {state.title}\n" + text[:14000]

    def close(self) -> None:
        if self.session is not None:
            try:
                self.run(self.session.stop(), timeout=10)
            except Exception:
                pass
        self.loop.call_soon_threadsafe(self.loop.stop)


def main() -> None:
    spec = load_spec()
    trace = Trace(spec["trace"])
    ex = BrowserUseExecutor(spec["cdp_http"])
    try:
        Combo(spec, trace, ex, spec["cdp_http"]).run()
    except Exception as error:  # noqa: BLE001
        trace.final("error", f"{type(error).__name__}: {error}"[:400])
        raise
    finally:
        ex.close()


if __name__ == "__main__":
    main()
