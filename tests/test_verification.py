import pytest

from numera_cha_solver.verification import VerificationContext, validate_assessment


def test_token_or_wrong_context_is_never_server_verified():
    expected = VerificationContext("turnstile", "localhost", "test", "session-a", True)
    for assessment in (
        {"token": "not-evidence"},
        {"success": "true"},
        {"success": True, "hostname": "other", "action": "test", "session_id": "session-a"},
        {"success": True, "hostname": "localhost", "action": "other", "session_id": "session-a"},
    ):
        assert validate_assessment(assessment, expected).server_verified is False
    result = validate_assessment(
        {
            "provider": "turnstile",
            "success": True,
            "hostname": "localhost",
            "action": "test",
            "session_id": "session-a",
        },
        expected,
    )
    assert result.outcome == "challenge_completed"
    assert result.server_verified is False
    assert result.evidence["test_key_integration"] is True


def test_provider_binding_is_required_for_owner_acceptance():
    expected = VerificationContext("turnstile", "localhost", "test", "session-a", False)
    valid = {
        "provider": "turnstile",
        "success": True,
        "hostname": "localhost",
        "action": "test",
        "session_id": "session-a",
    }
    for provider in (None, "recaptcha", ""):
        result = validate_assessment({**valid, "provider": provider}, expected)
        assert result.outcome == "failed"
        assert result.server_verified is False
    assert validate_assessment(valid, expected).server_verified is True


@pytest.mark.parametrize(
    "context",
    [
        VerificationContext("", "localhost", "test", "session-a", False),
        VerificationContext("turnstile", "", "test", "session-a", False),
        VerificationContext("turnstile", "localhost", "test", "", False),
        VerificationContext("turnstile", "localhost", "test", "session-a", "false"),
    ],
)
def test_invalid_verification_context_cannot_claim_acceptance(context):
    result = validate_assessment({"success": True}, context)
    assert result.outcome == "failed"
    assert result.error.code == "verification_context"


def test_geometry_round_trip_and_stale_revision():
    from numera_cha_solver.core.contracts import InputError
    from numera_cha_solver.core.geometry import Transform, validate_revision

    transform = Transform(300, 300, 1.5, 2, 17, 80)
    assert transform.to_image(*transform.to_viewport(100, 150)) == (100, 150)
    with pytest.raises(InputError, match="revision"):
        validate_revision("tile-1", "tile-2")
    with pytest.raises(InputError, match="outside"):
        transform.to_viewport(300, 0)
