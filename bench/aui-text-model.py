#!/usr/bin/env python3
"""CommandTextModel adapter: stdin JSON line -> Workers AI text model -> stdout reply.

Input : {"goal","field_label","field_role","context_fingerprint","max_chars"}
Output: {"text": "<value>", "context_fingerprint": N} or {"declined": "..."}
"""
import json
import os
import sys
import urllib.request

ACCOUNT = os.environ.get("CLOUDFLARE_ACCOUNT_ID", "")
TOKEN = os.environ.get("CLOUDFLARE_API_TOKEN", "")
MODEL = os.environ.get("TEXT_MODEL", "openai/gpt-6-luna")

SYSTEM = (
    "You generate the exact text value a browser agent must type into a web form field. "
    "Given a natural-language goal and the label and role of the target field, reply with ONLY "
    "a JSON object {\"text\": \"<value>\"}. The value must be the shortest reasonable literal that "
    "satisfies the goal for that field (e.g. a city name, a date like 'Sun, Sep 20' or 'September 20, 2026', "
    "a search term). No prose, no markdown."
)


def main() -> int:
    req = json.loads(sys.stdin.readline())
    fp = req["context_fingerprint"]
    url = f"https://api.cloudflare.com/client/v4/accounts/{ACCOUNT}/ai/v1/chat/completions"
    user = (
        f"Goal: {req['goal']}\n"
        f"Field label: {req['field_label']}\n"
        f"Field role: {req['field_role']}\n"
        f"Reply with the JSON object only."
    )
    body = json.dumps({
        "model": MODEL,
        "messages": [
            {"role": "system", "content": SYSTEM},
            {"role": "user", "content": user},
        ],
        "temperature": 0,
        "max_tokens": 64,
    }).encode()
    http = urllib.request.Request(url, data=body, headers={
        "Authorization": f"Bearer {TOKEN}",
        "Content-Type": "application/json",
    })
    try:
        with urllib.request.urlopen(http, timeout=30) as resp:
            data = json.loads(resp.read())
        content = data["choices"][0]["message"]["content"].strip()
        # Strip code fences if present.
        if content.startswith("```"):
            content = content.split("\n", 1)[1].rsplit("```", 1)[0].strip()
        parsed = json.loads(content)
        text = parsed["text"]
        if req.get("max_chars") and len(text) > int(req["max_chars"]):
            print(json.dumps({"declined": "value exceeds max_chars"}))
            return 0
        print(json.dumps({"text": text, "context_fingerprint": fp}))
    except Exception as exc:  # noqa: BLE001 - reply protocol requires a JSON line
        print(json.dumps({"declined": f"model call failed: {exc}"}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
