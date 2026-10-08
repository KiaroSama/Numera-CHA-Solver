use numera_cha_solver::{
    benchmark::{benchmark, read_manifest},
    config::Settings,
    core::{
        contracts::Input,
        geometry::{Transform, validate_revision},
    },
    sdk::Numera,
    verification::{VerificationContext, validate_assessment},
};
use serde_json::{Value, json};

#[test]
fn provider_context_geometry_and_input_bounds_are_preserved() {
    let expected = VerificationContext {
        provider: "turnstile".into(),
        hostname: "localhost".into(),
        action: "test".into(),
        session_id: "session-a".into(),
        test_keys: true,
    };
    let valid = json!({"provider":"turnstile","hostname":"localhost","action":"test","session_id":"session-a","success":true});
    let result = validate_assessment(&valid, &expected);
    assert_eq!(result.outcome, "challenge_completed");
    assert!(!result.server_verified);
    let mut real = expected.clone();
    real.test_keys = false;
    assert!(validate_assessment(&valid, &real).server_verified);
    for field in ["provider", "hostname", "action", "session_id"] {
        let mut wrong = valid.clone();
        wrong[field] = json!("other");
        assert!(!validate_assessment(&wrong, &real).server_verified);
        assert_eq!(validate_assessment(&wrong, &real).outcome, "failed");
    }
    for assessment in [
        json!({"token":"not-evidence"}),
        json!({"success":"true"}),
        json!({"success":true,"hostname":"localhost","action":"test","session_id":"session-a"}),
        json!({"provider":null,"success":true}),
    ] {
        assert!(!validate_assessment(&assessment, &real).server_verified)
    }
    assert!(serde_json::from_value::<VerificationContext>(json!({"provider":"turnstile","hostname":"localhost","action":"test","session_id":"session-a","test_keys":"false"})).is_err());
    for field in ["provider", "hostname", "session_id"] {
        let mut wrong = serde_json::to_value(&real).unwrap();
        wrong[field] = json!("");
        let wrong: VerificationContext = serde_json::from_value(wrong).unwrap();
        assert_eq!(
            validate_assessment(&valid, &wrong).error.unwrap().code,
            "verification_context"
        );
    }
    let transform = Transform {
        width: 300.0,
        height: 300.0,
        scale_x: 1.5,
        scale_y: 2.0,
        offset_x: 17.0,
        offset_y: 80.0,
    };
    let viewport = transform.to_viewport(100.0, 150.0).unwrap();
    assert_eq!(
        transform.to_image(viewport.0, viewport.1).unwrap(),
        (100.0, 150.0)
    );
    assert!(transform.to_viewport(300.0, 0.0).is_err());
    assert!(validate_revision("tile-1", "tile-2").is_err());
    assert!(validate_revision("", "").is_err());
    assert!(Input::from_value(json!({"kind":"text","text":"é".repeat(4096)})).is_ok());
    assert!(Input::from_value(json!({"kind":"text","text":"é".repeat(4097)})).is_err());
}

#[tokio::test]
async fn benchmarks_validate_failures_duplicates_and_denominators() {
    let root = tempfile::tempdir().unwrap();
    let manifest = root.path().join("samples.json");
    let sdk = Numera::new(Settings::new(root.path().into()));
    for document in [
        Value::Null,
        json!([]),
        json!({"license":[],"samples":[{}]}),
        json!({"license":"owned","samples":[{"label":"x","session_id":"a","path":"../x.png"}]}),
        json!({"license":"owned","samples":[{"label":"x","session_id":"a","path":"x.png"},{"label":"y","session_id":"a","path":"y.png"}]}),
        json!({"license":"owned","samples":[null]}),
        json!({"license":"owned","samples":[{"label":"x","session_id":[],"path":"x.png"}]}),
    ] {
        std::fs::write(&manifest, document.to_string()).unwrap();
        assert_eq!(
            read_manifest(&manifest).unwrap_err().code,
            "invalid_benchmark"
        );
    }
    let oversized = vec![b' '; 1024 * 1024 + 1];
    for bytes in [b"{".as_slice(), b"\xff".as_slice(), oversized.as_slice()] {
        std::fs::write(&manifest, bytes).unwrap();
        assert_eq!(
            read_manifest(&manifest).unwrap_err().code,
            "invalid_benchmark"
        );
    }
    std::fs::write(root.path().join("invalid.png"), b"not an image").unwrap();
    let document = json!({"license":"owned","kind":"synthetic","samples":[{"label":"x","session_id":"one","path":"missing.png"},{"label":"y","session_id":"two","path":"invalid.png"}]});
    std::fs::write(&manifest, document.to_string()).unwrap();
    let result = benchmark(&sdk, &manifest).await.unwrap();
    assert_eq!(result["sample_count"], 2);
    assert_eq!(result["correct"], 0);
    assert_eq!(result["limited_evidence"], true);
    assert_eq!(result["exact_accuracy"], 0.0);
    assert_eq!(result["live_acceptance_measured"], false);
    assert_eq!(result["confidence_interval95"][0], 0.0);
    assert_eq!(result["rows"][0]["outcome"], "failed");
    assert_eq!(result["rows"][1]["outcome"], "failed");
    std::fs::write(root.path().join("copy.png"), b"not an image").unwrap();
    std::fs::write(&manifest,json!({"license":"owned","samples":[{"label":"x","session_id":"one","path":"invalid.png"},{"label":"x","session_id":"two","path":"copy.png"}]}).to_string()).unwrap();
    assert_eq!(
        benchmark(&sdk, &manifest).await.unwrap_err().code,
        "invalid_benchmark"
    );
    std::fs::write(&manifest,json!({"license":"owned","samples":[{"label":"x","session_id":"one","path":"invalid.png"},{"label":"x","session_id":"two","path":"./invalid.png"}]}).to_string()).unwrap();
    assert_eq!(
        read_manifest(&manifest).unwrap_err().code,
        "invalid_benchmark"
    );
    sdk.close().await;
}

#[tokio::test]
async fn missing_model_is_not_fake_image_recognition_and_shared_interfaces_exist() {
    let root = tempfile::tempdir().unwrap();
    let sdk = Numera::new(Settings::new(root.path().into()));
    let input = Input {
        kind: "image".into(),
        asset: include_bytes!("fixtures/ocr/ascii-smoke.png").to_vec(),
        text: String::new(),
        subtype: "auto".into(),
        allowed_characters: String::new(),
        revision: String::new(),
    };
    let result = sdk.recognize(input, None).await;
    assert_eq!(result.outcome, "unsupported");
    assert_eq!(result.error.unwrap().code, "model_missing");
    assert!(result.answer.is_none());
    assert_eq!(
        sdk.capabilities().await.unwrap()["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["catalog_id"].to_string())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        26
    );
    assert_eq!(
        sdk.models().await.unwrap()[0]["availability"],
        "model_missing"
    );
    assert_eq!(sdk.doctor().await.unwrap()["gpu_execution_verified"], false);
    let input = Input::from_value(json!({"kind":"text","text":"7+5"})).unwrap();
    assert_eq!(sdk.detect(&input).unwrap()["provider"], "unknown");
    assert_eq!(sdk.solve(input, None).await.answer.as_deref(), Some("12"));
    sdk.close().await;
}
