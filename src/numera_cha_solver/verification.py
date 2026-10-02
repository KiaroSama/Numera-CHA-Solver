"""Assess owner-side evidence; test-key success is always labeled separately."""

from dataclasses import dataclass
from typing import Any

from .core.contracts import Error, Result


@dataclass(frozen=True)
class VerificationContext:
    provider: str
    hostname: str
    action: str
    session_id: str
    test_keys: bool = True


def validate_assessment(assessment: dict[str, Any], expected: VerificationContext) -> Result:
    if (
        any(
            not isinstance(value, str) or not value.strip()
            for value in (expected.provider, expected.hostname, expected.session_id)
        )
        or not isinstance(expected.action, str)
        or type(expected.test_keys) is not bool
        or not isinstance(assessment, dict)
    ):
        return Result(
            "failed",
            mode="browser_protocol",
            error=Error(
                "verification_context",
                "Verification requires a valid provider, hostname and session context.",
            ),
        )
    if assessment.get("success") is not True:
        return Result(
            "failed",
            mode="browser_protocol",
            error=Error("server_rejected", "Owner-side verification did not accept the response."),
        )
    for field, value in (
        ("provider", expected.provider),
        ("hostname", expected.hostname),
        ("action", expected.action),
        ("session_id", expected.session_id),
    ):
        if assessment.get(field) != value:
            return Result(
                "failed",
                mode="browser_protocol",
                error=Error(
                    "verification_context",
                    "Owner-side evidence does not match the expected context.",
                ),
            )
    if expected.test_keys:
        return Result(
            "challenge_completed",
            mode="browser_protocol",
            provider=expected.provider,
            evidence={"test_key_integration": True, "production_acceptance_measured": False},
        )
    return Result(
        "server_verified",
        server_verified=True,
        mode="browser_protocol",
        provider=expected.provider,
        evidence={"owner_assessment": True},
    )
