"""Strict UTF-8, source ceilings and allowlisted distribution contents."""

import argparse
import json
import logging
import tarfile
import zipfile
from pathlib import Path

from numera_cha_solver.logging import close, initialize

ROOT = Path(__file__).resolve().parents[1]
FORBIDDEN = {
    ".ai",
    ".specify",
    "specs",
    ".claude",
    ".codex",
    ".kiro",
    ".ignoreme",
    ".codebase-memory",
    "graphify-out",
    ".ci-runner-win",
    ".ci-cache",
    "data",
    "logs",
    "secrets.md",
    "explain-AI.md",
    "CLAUDE.md",
    "AGENTS.md",
}
TEXT = {
    ".py",
    ".ps1",
    ".psm1",
    ".md",
    ".toml",
    ".json",
    ".yaml",
    ".yml",
    ".html",
    ".css",
    ".js",
    ".lock",
    ".txt",
}


def validate_member(name: str) -> None:
    parts = Path(name).parts
    if set(parts) & FORBIDDEN or any(part.startswith(".env") for part in parts):
        raise ValueError("Package contains excluded local material.")
    if any(part in {"..", "."} for part in parts) or Path(name).is_absolute():
        raise ValueError("Package contains unsafe member paths.")
    if name.endswith((".onnx", ".dll", ".exe", ".sqlite3")):
        raise ValueError("Package includes an unapproved binary/runtime/model.")


def source() -> None:
    for folder in ("src", "scripts", "tests", "docs", "reports", ".github"):
        directory = ROOT / folder
        if not directory.exists():
            continue
        for path in directory.rglob("*"):
            if path.is_file() and path.suffix in TEXT:
                content = path.read_text(encoding="utf-8", errors="strict")
                if (
                    path.suffix in {".py", ".ps1", ".psm1", ".js"}
                    and len(content.splitlines()) > 800
                ):
                    raise ValueError(f"Source file exceeds 800 lines: {path.relative_to(ROOT)}")
    report = json.loads((ROOT / "reports/COVERAGE.json").read_text(encoding="utf-8"))
    packaged = json.loads(
        (ROOT / "src/numera_cha_solver/resources/capabilities.json").read_text(encoding="utf-8")
    )
    if report != packaged or len({row["catalog_id"] for row in report["capabilities"]}) != 26:
        raise ValueError("Requested coverage catalog is incomplete or out of sync.")
    tests = list((ROOT / "tests").glob("test_*.py"))
    if not tests or "tests" not in (ROOT / "scripts/ci.ps1").read_text(encoding="utf-8"):
        raise ValueError("CI does not claim the whole test directory.")


def packages(directory: Path) -> None:
    paths = list(directory.glob("*.whl")) + list(directory.glob("*.tar.gz"))
    if len(paths) != 2:
        raise ValueError("Expected one wheel and one source distribution.")
    for path in paths:
        if path.suffix == ".whl":
            with zipfile.ZipFile(path) as archive:
                for name in archive.namelist():
                    validate_member(name)
        else:
            with tarfile.open(path, "r:gz") as archive:
                for member in archive.getmembers():
                    validate_member(member.name)
                    if member.issym() or member.islnk():
                        raise ValueError("Distribution includes links.")


if __name__ == "__main__":
    initialize(ROOT / "logs", "check-artifacts")
    try:
        parser = argparse.ArgumentParser()
        parser.add_argument("--source", action="store_true")
        parser.add_argument("--packages", type=Path)
        args = parser.parse_args()
        if args.source:
            source()
        if args.packages:
            packages(args.packages)
        logging.getLogger("numera").info("artifact validation passed")
    finally:
        close()
