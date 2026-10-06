#!/usr/bin/env python3
"""Enforce scripts/architecture.txt against `cargo metadata` (spec §3.1).

Exit 0 when every rule holds, 1 with a list of violations otherwise. Pure stdlib so it runs on a
bare CI runner. Self-test: `python3 scripts/check_architecture.py --self-test`.
"""
from __future__ import annotations

import fnmatch
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def parse_rules(text: str) -> dict[str, tuple[set[str], set[str]]]:
    rules: dict[str, tuple[set[str], set[str]]] = {}
    current: str | None = None
    for lineno, raw in enumerate(text.splitlines(), 1):
        line = raw.split("#", 1)[0].rstrip()
        if not line.strip():
            continue
        if line.lstrip().startswith("|"):
            if current is None:
                raise SystemExit(f"architecture.txt:{lineno}: ban line before any crate entry")
            rules[current][1].update(line.lstrip()[1:].split())
            continue
        if ":" not in line:
            raise SystemExit(f"architecture.txt:{lineno}: expected '<crate>: <allowed...>'")
        name, allowed = line.split(":", 1)
        current = name.strip()
        if current in rules:
            raise SystemExit(f"architecture.txt:{lineno}: duplicate entry {current}")
        rules[current] = (set(allowed.split()), set())
    return rules


def rule_for(rules, crate: str):
    if crate in rules:
        return rules[crate]
    matches = [k for k in rules if "*" in k and fnmatch.fnmatchcase(crate, k)]
    if len(matches) == 1:
        return rules[matches[0]]
    return None


def check(metadata: dict, rules) -> list[str]:
    errors: list[str] = []
    pkgs = {p["id"]: p for p in metadata["packages"]}
    members = set(metadata["workspace_members"])
    member_names = {pkgs[m]["name"] for m in members}
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}

    for mid in sorted(members, key=lambda i: pkgs[i]["name"]):
        pkg = pkgs[mid]
        name = pkg["name"]
        rule = rule_for(rules, name)
        if rule is None:
            errors.append(f"{name}: no entry in scripts/architecture.txt")
            continue
        allowed, banned = rule
        # Rule 2: direct non-dev deps on workspace crates must be allowed.
        for dep in nodes[mid]["deps"]:
            dep_name = pkgs[dep["pkg"]]["name"]
            kinds = {k.get("kind") for k in dep["dep_kinds"]}
            if dep_name in member_names and kinds - {"dev"} and dep_name not in allowed:
                errors.append(f"{name}: depends on {dep_name}, not in its allowed list")
        # Rule 3: banned crates anywhere in the closure (all kinds).
        seen, stack = set(), [mid]
        while stack:
            cur = stack.pop()
            if cur in seen:
                continue
            seen.add(cur)
            stack.extend(d["pkg"] for d in nodes[cur]["deps"])
        for cid in sorted(seen - {mid}):
            dep_name = pkgs[cid]["name"]
            for pattern in sorted(banned):
                if fnmatch.fnmatchcase(dep_name, pattern):
                    errors.append(f"{name}: banned crate {dep_name} (matches '{pattern}') in closure")
    return errors


def self_test() -> None:
    rules = parse_rules("a:\n    | tokio x-*\nb: a\n    | tokio\np-*: a\n    | hyper\n")
    def meta(edges, extra=()):
        names = ["a", "b", "p-one", "tokio", "x-y", "hyper", *extra]
        packages = [{"id": n, "name": n} for n in names]
        members = ["a", "b", "p-one", *extra]
        nodes = [{"id": n, "deps": [{"pkg": d, "dep_kinds": [{"kind": k}]} for d, k in edges.get(n, [])]} for n in names]
        return {"packages": packages, "workspace_members": members, "resolve": {"nodes": nodes}}
    assert check(meta({"b": [("a", None)], "p-one": [("a", None)]}), rules) == []
    assert any("not in its allowed" in e for e in check(meta({"a": [("b", None)]}), rules))
    assert check(meta({"a": [("b", "dev")]}), rules) == []  # dev edge to a member is fine
    assert any("banned crate tokio" in e for e in check(meta({"b": [("tokio", "dev")]}), rules))
    assert any("x-y" in e for e in check(meta({"a": [("x-y", None)]}), rules))
    assert any("hyper" in e for e in check(meta({"p-one": [("hyper", "build")]}), rules))
    assert any("no entry" in e for e in check(meta({}, extra=("zzz",)), rules))
    print("check_architecture self-test: ok")


def main() -> int:
    if "--self-test" in sys.argv:
        self_test()
        return 0
    rules = parse_rules((ROOT / "scripts" / "architecture.txt").read_text())
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--all-features", "--locked"],
        cwd=ROOT, check=True, capture_output=True, text=True,
    ).stdout
    errors = check(json.loads(out), rules)
    for e in errors:
        print(f"architecture: {e}", file=sys.stderr)
    if not errors:
        print("architecture: ok")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
