"""Historical Python witness; native Rust owns rollback process and HTTP orchestration."""

import argparse
import asyncio
import hashlib
import json
import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE_ROOT = Path(os.environ["NUMERA_ORACLE_SOURCE"]).resolve()
sys.path.insert(0, str(SOURCE_ROOT / "src"))
from numera_cha_solver.config import Settings  # noqa: E402
from numera_cha_solver.core.contracts import Input  # noqa: E402
from numera_cha_solver.logging import close, initialize  # noqa: E402
from numera_cha_solver.sdk import Numera  # noqa: E402
from numera_cha_solver.store import TaskStore  # noqa: E402


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--operation", choices=("seed", "check"), required=True)
    parser.add_argument("--work", type=Path, required=True)
    args = parser.parse_args()
    work = args.work.resolve()
    if not work.is_relative_to(ROOT / ".ci-work") or SOURCE_ROOT == ROOT:
        raise ValueError("Rollback requires isolated hosted state and the pinned source snapshot.")
    logger = initialize(work / "logs")
    sdk = None
    try:
        witness = work / "source-witness.json"
        database = work / "data/tasks.sqlite3"
        if args.operation == "seed":
            sdk = Numera(Settings(root=work))
            source = asyncio.run(sdk.recognize({"kind": "text", "text": "7+5"})).to_dict()
            request = Input("text", text="7+5")
            encoded = json.dumps({"kind": request.kind, "text": request.text,
                "asset_sha256": hashlib.sha256(request.asset).hexdigest(), "subtype": request.subtype,
                "allowed_characters": request.allowed_characters, "revision": request.revision}, sort_keys=True)
            digest = hashlib.sha256(encoded.encode("utf-8")).hexdigest()
            with TaskStore(database, 60) as store:
                task = store.create("rollback-fixture", digest, 120)
                assert store.finish(task["id"], source)
            witness.write_text(json.dumps({"source": source, "digest": digest, "source_id": task["id"]}), encoding="utf-8")
        else:
            expected = json.loads(witness.read_text(encoding="utf-8"))
            native_id = json.loads((work / "native-witness.json").read_text(encoding="utf-8"))["id"]
            with TaskStore(database, 60) as restored:
                assert restored.get("copied-unfinished")["result"]["error"]["code"] == "interrupted"
                for identity, key in ((expected["source_id"], "rollback-fixture"), (native_id, "native-rollback-fixture")):
                    assert restored.get(identity)["result"] == expected["source"]
                    assert restored.create(key, expected["digest"], 120)["id"] == identity
            print("Copied-state source/native rollback verified.")
        logger.info("historical rollback witness operation=%s passed", args.operation)
        return 0
    finally:
        if sdk is not None:
            sdk.close()
        logger.info("cleanup rollback historical resources closed")
        close()


if __name__ == "__main__":
    raise SystemExit(main())
