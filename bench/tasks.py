"""Load scenario tasks from ``examples/<scenario>/tasks.yaml``."""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path

import yaml

EXAMPLES = Path(__file__).resolve().parents[1] / "examples"
SCENARIOS = ["acme-mail", "shop-checkout", "admin-table", "booking-calendar", "hard-dom", "travel-search"]
CLASSES = {"normal", "target-missing", "no-op"}
NEEDS = {"type", "select"}


@dataclass
class Task:
    scenario: str
    id: str
    cls: str
    goal: str
    start: str
    needs: list[str]
    success: list[dict]
    forbidden: list[dict]
    oracle: list[dict] = field(default_factory=list)
    saboteur: list[dict] = field(default_factory=list)

    @property
    def press_only(self) -> bool:
        return not self.needs

    def spec(self) -> dict:
        return {"scenario": self.scenario, "id": self.id, "class": self.cls, "goal": self.goal,
                "start": self.start, "needs": self.needs}


def load_scenario(name: str) -> list[Task]:
    data = yaml.safe_load((EXAMPLES / name / "tasks.yaml").read_text())
    tasks = []
    for raw in data["tasks"]:
        task = Task(
            scenario=name,
            id=raw["id"],
            cls=raw.get("class", "normal"),
            goal=" ".join(str(raw["goal"]).split()),
            start=raw["start"],
            needs=list(raw.get("needs") or []),
            success=list(raw.get("success") or []),
            forbidden=list(raw.get("forbidden") or []),
            oracle=list(raw.get("oracle") or []),
            saboteur=list(raw.get("saboteur") or []),
        )
        assert task.cls in CLASSES, (task.id, task.cls)
        assert set(task.needs) <= NEEDS, (task.id, task.needs)
        assert task.start.startswith("/" + name + "/"), (task.id, task.start)
        if task.cls == "target-missing":
            assert any("final" in p for p in task.success), task.id
        tasks.append(task)
    ids = [t.id for t in tasks]
    assert len(ids) == len(set(ids)), f"duplicate ids in {name}"
    return tasks


def load_all(names: list[str] | None = None) -> list[Task]:
    out: list[Task] = []
    for name in names or SCENARIOS:
        out.extend(load_scenario(name))
    return out
