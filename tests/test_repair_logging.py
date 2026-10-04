import json

from numera_cha_solver.logging import close, initialize


def test_quoted_nested_and_exception_secrets_never_reach_handlers(tmp_path, capsys):
    sentinel = "owned-synthetic-diagnostic-value"
    logger = initialize(tmp_path, debug=True)
    try:
        logger.info(json.dumps({"password": sentinel, "nested": {"api_key": sentinel}}))
        logger.debug("context=%s", {"authorization": sentinel, "cookie": sentinel})
        logger.warning('password="multi line\n%s"', sentinel)
        logger.error("Bearer %s", sentinel)
        logger.info("password=%s number=%d", sentinel, "wrong-type")
        try:
            raise ValueError(json.dumps({"token": sentinel}))
        except ValueError:
            logger.exception("controlled diagnostic")
    finally:
        close(logger)
    assert sentinel not in capsys.readouterr().err
    assert all(sentinel not in p.read_text(encoding="utf-8") for p in tmp_path.glob("*.log"))
