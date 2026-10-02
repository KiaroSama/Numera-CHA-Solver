"""Generate the README family summary from the machine-readable inventory."""

import json
import logging
from pathlib import Path

from numera_cha_solver.logging import close, initialize

ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    rows = json.loads((ROOT / "reports/COVERAGE.json").read_text(encoding="utf-8"))["capabilities"]
    groups = {}
    for row in rows:
        groups.setdefault(row["catalog_id"], []).append(row)
    table = ["| ID | Family | Declared scope | Evidence / blockers |", "| --- | --- | --- | --- |"]
    for identifier, entries in groups.items():
        state = (
            "Limited local building blocks; provider accuracy unverified"
            if any(row["implementation_state"] != "blocked" for row in entries)
            else "Blocked: model/data/owned lifecycle unavailable"
        )
        table.append(
            f"| {identifier} | {entries[0]['provider']} | "
            f"{len(entries)} subtype decisions | {state} |"
        )
    path = ROOT / "README.md"
    text = path.read_text(encoding="utf-8")
    before, rest = text.split("<!-- COVERAGE:START -->", 1)
    _, after = rest.split("<!-- COVERAGE:END -->", 1)
    path.write_text(
        before + "<!-- COVERAGE:START -->\n" + "\n".join(table) + "\n<!-- COVERAGE:END -->" + after,
        encoding="utf-8",
    )


if __name__ == "__main__":
    initialize(ROOT / "logs", "render-coverage")
    try:
        main()
        logging.getLogger("numera").info("README coverage summary generated")
    finally:
        close()
