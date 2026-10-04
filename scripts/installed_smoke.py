"""Execute the installed wheel from an isolated directory, not editable source."""

import asyncio
import logging
import os
from pathlib import Path

from numera_cha_solver.config import Settings
from numera_cha_solver.logging import close, initialize
from numera_cha_solver.sdk import Numera


def main() -> None:
    root = Path(os.environ["NUMERA_INSTALL_SMOKE_ROOT"])
    initialize(root / "logs", "installed-smoke")
    sdk = Numera(Settings(root=root, device="cpu"))
    try:
        result = asyncio.run(sdk.recognize({"kind": "text", "text": "7+5"}))
        if result.answer != "12" or result.server_verified:
            raise RuntimeError("Installed SDK smoke failed.")
        import numera_cha_solver

        if "site-packages" not in str(Path(numera_cha_solver.__file__).resolve()):
            raise RuntimeError("Smoke did not import the installed wheel.")
        logging.getLogger("numera").info("installed wheel SDK arithmetic smoke passed")
    finally:
        sdk.close()
        close()


if __name__ == "__main__":
    main()
