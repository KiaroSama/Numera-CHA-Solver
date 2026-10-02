"""Per-run UTF-8 diagnostics; never record request content or credentials."""

import logging
import re
import time
from datetime import UTC, datetime
from logging.handlers import RotatingFileHandler
from pathlib import Path
from uuid import uuid4

_SENSITIVE = re.compile(
    r"(?i)(token|secret|password|authorization|cookie|api[_-]?key|response)\s*[:=]\s*[^\s,;]+"
)
_BEARER = re.compile(r"(?i)bearer\s+\S+")
_URL = re.compile(r"https?://\S+")


def redact(value: object) -> str:
    text = _BEARER.sub("Bearer [REDACTED]", str(value))
    text = _SENSITIVE.sub(lambda m: m.group(1) + "=[REDACTED]", text)
    return _URL.sub("[URL REDACTED]", text)


class SafeFormatter(logging.Formatter):
    converter = staticmethod(time.gmtime)

    def format(self, record: logging.LogRecord) -> str:
        return redact(super().format(record))


def initialize(directory: Path, name: str = "numera-cha", debug: bool = False) -> logging.Logger:
    logger = logging.getLogger("numera")
    close(logger)
    logger.setLevel(logging.DEBUG if debug else logging.INFO)
    formatter = SafeFormatter(
        "[%(asctime)s UTC] [%(levelname)s] [%(name)s] %(message)s", "%Y-%m-%d %H:%M:%S"
    )
    stream = logging.StreamHandler()
    stream.setFormatter(formatter)
    logger.addHandler(stream)
    try:
        directory.mkdir(parents=True, exist_ok=True)
        stamp = datetime.now(UTC).strftime("%Y-%m-%d_%H-%M-%S")
        path = directory / f"{name}_{stamp}_UTC.log"
        if path.exists():
            path = directory / f"{name}_{stamp}_UTC_{uuid4().hex[:8]}.log"
        handler = RotatingFileHandler(path, maxBytes=2_000_000, backupCount=2, encoding="utf-8")
        handler.setFormatter(formatter)
        logger.addHandler(handler)
        logger.info("startup diagnostic logging initialized")
    except OSError:
        logger.warning("startup file logging unavailable; stderr fallback active")
    return logger


def close(logger: logging.Logger | None = None) -> None:
    logger = logger or logging.getLogger("numera")
    for handler in list(logger.handlers):
        handler.flush()
        handler.close()
        logger.removeHandler(handler)
