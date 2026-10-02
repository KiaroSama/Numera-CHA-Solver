import logging

from numera_cha_solver.logging import close, initialize


def test_logging_is_utf8_per_run_redacted_and_closed(tmp_path):
    for _ in range(2):
        logger = initialize(tmp_path, debug=True)
        logger.debug("config token=private-value password=secret-value")
        logger.warning("controlled warning")
        logger.error("controlled failure")
        logger.info("cleanup finished")
        close(logger)
    logs = list(tmp_path.glob("*.log"))
    assert len(logs) == 2
    for path in logs:
        text = path.read_text(encoding="utf-8")
        assert "private-value" not in text and "secret-value" not in text
        assert "[DEBUG]" in text and "[WARNING]" in text and "[ERROR]" in text
        assert "UTC]" in text and "[REDACTED]" in text
        assert "cleanup finished" in text
    assert not logging.getLogger("numera").handlers
