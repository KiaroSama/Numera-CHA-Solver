"""Per-run UTF-8 diagnostics; never record request content or credentials."""

import logging
import re
import time
from datetime import UTC, datetime
from logging.handlers import RotatingFileHandler
from pathlib import Path
from uuid import uuid4

_KEY = r"token|secret|password|authorization|cookies?|api[_-]?key|response"
_SENSITIVE = re.compile(
    rf"(?i)([\"']?(?:{_KEY})[\"']?\s*[:=]\s*)"
    r"(?:\"(?:\\.|[^\"\\])*\"|'(?:\\.|[^'\\])*'|[^\s,;}]+)"
)
_KEY_MATCH = re.compile(rf"(?i)^(?:{_KEY})$")
_BEARER = re.compile(r"(?i)bearer\s+\S+")
_URL = re.compile(r"https?://\S+")


def redact(value: object) -> str:
    text = _BEARER.sub("Bearer [REDACTED]", str(value))
    text = _SENSITIVE.sub(lambda m: m.group(1) + "[REDACTED]", text)
    return _URL.sub("[URL REDACTED]", text)


def sanitize(value: object, depth: int = 0) -> object:
    if depth > 8:
        return "[DEPTH REDACTED]"
    if isinstance(value, dict):
        return {
            key: "[REDACTED]" if _KEY_MATCH.fullmatch(str(key)) else sanitize(item, depth + 1)
            for key, item in value.items()
        }
    if isinstance(value, (list, tuple)):
        return type(value)(sanitize(item, depth + 1) for item in value)
    return redact(value) if isinstance(value, str) else value


class SafeRecordFilter(logging.Filter):
    def filter(self, record: logging.LogRecord) -> logging.LogRecord:
        import copy

        safe = copy.copy(record)
        try:
            message = sanitize(record.msg)
            arguments = sanitize(record.args)
            safe.msg = redact(str(message) % arguments if arguments else str(message))
            safe.args = ()
            if record.exc_info:
                safe.exc_text = redact(logging.Formatter().formatException(record.exc_info))
            elif record.exc_text:
                safe.exc_text = redact(record.exc_text)
            safe.exc_info = None
            if record.stack_info:
                safe.stack_info = redact(record.stack_info)
        except Exception:
            safe.msg = "Diagnostic formatting failed; sensitive details suppressed."
            safe.args = ()
            safe.exc_info = None
            safe.exc_text = None
            safe.stack_info = None
        return safe


class SafeFormatter(logging.Formatter):
    converter = staticmethod(time.gmtime)

    def format(self, record: logging.LogRecord) -> str:
        return redact(super().format(record))


def initialize(directory: Path, name: str = "numera-cha", debug: bool = False) -> logging.Logger:
    logger = logging.getLogger("numera")
    close(logger)
    logger.setLevel(logging.DEBUG if debug else logging.INFO)
    logger.propagate = False
    formatter = SafeFormatter(
        "[%(asctime)s UTC] [%(levelname)s] [%(name)s] %(message)s", "%Y-%m-%d %H:%M:%S"
    )
    stream = logging.StreamHandler()
    stream.setFormatter(formatter)
    stream.addFilter(SafeRecordFilter())
    logger.addHandler(stream)
    try:
        directory.mkdir(parents=True, exist_ok=True)
        stamp = datetime.now(UTC).strftime("%Y-%m-%d_%H-%M-%S")
        path = directory / f"{name}_{stamp}_UTC.log"
        if path.exists():
            path = directory / f"{name}_{stamp}_UTC_{uuid4().hex[:8]}.log"
        handler = RotatingFileHandler(path, maxBytes=2_000_000, backupCount=2, encoding="utf-8")
        handler.setFormatter(formatter)
        handler.addFilter(SafeRecordFilter())
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
