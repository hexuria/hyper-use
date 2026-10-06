"""Tiny newline-delimited JSON-RPC MCP client over stdio (no SDK, works in any env)."""

from __future__ import annotations

import json
import subprocess
from itertools import count


class McpStdio:
    def __init__(self, argv: list[str], env: dict | None = None) -> None:
        self.proc = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                     text=True, bufsize=1, env=env)
        self.ids = count(1)
        self.request("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                    "clientInfo": {"name": "ultra-instinct-bench", "version": "1"}})
        self.notify("notifications/initialized")

    def notify(self, method: str, params: dict | None = None) -> None:
        self.proc.stdin.write(json.dumps({"jsonrpc": "2.0", "method": method, "params": params or {}}) + "\n")
        self.proc.stdin.flush()

    def request(self, method: str, params: dict | None = None) -> dict:
        msg_id = next(self.ids)
        self.proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": msg_id, "method": method, "params": params or {}}) + "\n")
        self.proc.stdin.flush()
        while True:
            line = self.proc.stdout.readline()
            if not line:
                raise RuntimeError("MCP server exited")
            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                continue
            if msg.get("id") == msg_id:
                if "error" in msg:
                    raise RuntimeError(json.dumps(msg["error"])[:500])
                return msg.get("result", {})

    def tools(self) -> list[dict]:
        return self.request("tools/list").get("tools", [])

    def call(self, name: str, arguments: dict) -> str:
        result = self.request("tools/call", {"name": name, "arguments": arguments})
        if result.get("structuredContent") is not None:
            text = json.dumps(result["structuredContent"])
        else:
            text = "\n".join(c.get("text", "") for c in result.get("content", []) if c.get("type") == "text")
        return ("ERROR: " if result.get("isError") else "") + text

    def close(self) -> None:
        try:
            self.proc.terminate()
        except Exception:
            pass
