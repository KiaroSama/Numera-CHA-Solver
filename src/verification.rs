use crate::core::contracts::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationContext {
    pub provider: String,
    pub hostname: String,
    pub action: String,
    pub session_id: String,
    pub test_keys: bool,
}
pub fn validate_assessment(assessment: &Value, expected: &VerificationContext) -> Result {
    let failure = |code: &str, message: &str| Result {
        mode: "browser_protocol".into(),
        error: Some(Error::new(code, message)),
        ..Result::new("failed")
    };
    if [&expected.provider, &expected.hostname, &expected.session_id]
        .iter()
        .any(|s| s.trim().is_empty())
        || !assessment.is_object()
    {
        return failure(
            "verification_context",
            "Verification requires a valid provider, hostname and session context.",
        );
    }
    if assessment.get("success") != Some(&Value::Bool(true)) {
        return failure(
            "server_rejected",
            "Owner-side verification did not accept the response.",
        );
    }
    for (name, value) in [
        ("provider", &expected.provider),
        ("hostname", &expected.hostname),
        ("action", &expected.action),
        ("session_id", &expected.session_id),
    ] {
        if assessment.get(name).and_then(Value::as_str) != Some(value.as_str()) {
            return failure(
                "verification_context",
                "Owner-side evidence does not match the expected context.",
            );
        }
    }
    Result {
        server_verified: !expected.test_keys,
        mode: "browser_protocol".into(),
        provider: expected.provider.clone(),
        evidence: if expected.test_keys {
            json!({"test_key_integration":true,"production_acceptance_measured":false})
        } else {
            json!({"owner_assessment":true})
        },
        ..Result::new(if expected.test_keys {
            "challenge_completed"
        } else {
            "server_verified"
        })
    }
}
