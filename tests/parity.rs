use numera_cha_solver::{config::Settings, core::arithmetic::evaluate, sdk::Numera};
use serde_json::{Value, json};

#[test]
fn arithmetic_matches_generated_source_oracle() {
    let document: Value =
        serde_json::from_str(include_str!("fixtures/parity/arithmetic.json")).unwrap();
    let cases = document["cases"].as_array().unwrap();
    assert!(!cases.is_empty(), "An empty oracle proves no parity.");
    for case in cases {
        let input = case["input"].as_str().unwrap();
        let actual = match evaluate(input) {
            Ok(answer) => json!({"answer": answer, "error": null}),
            Err(error) => json!({"answer": null, "error": error}),
        };
        assert_eq!(actual, case["expected"], "oracle case {}", case["name"]);
    }
}

#[test]
fn geometry_verification_and_ctc_match_source_contracts() {
    use numera_cha_solver::{
        core::geometry::Transform,
        engines::ctc,
        verification::{VerificationContext, validate_assessment},
    };
    let document: Value =
        serde_json::from_str(include_str!("fixtures/parity/contracts.json")).unwrap();
    for case in document["verification"].as_array().unwrap() {
        let context: VerificationContext = serde_json::from_value(case["context"].clone()).unwrap();
        assert_eq!(
            serde_json::to_value(validate_assessment(&case["input"], &context)).unwrap(),
            case["expected"]
        )
    }
    for case in document["geometry"].as_array().unwrap() {
        let d: Vec<f64> = case["dimensions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let point = case["point"].as_array().unwrap();
        let transform = Transform {
            width: d[0],
            height: d[1],
            scale_x: d[2],
            scale_y: d[3],
            offset_x: d[4],
            offset_y: d[5],
        };
        let x = point[0].as_f64().unwrap();
        let y = point[1].as_f64().unwrap();
        let result = if case["operation"] == "to_viewport" {
            transform.to_viewport(x, y)
        } else {
            transform.to_image(x, y)
        };
        let actual = match result {
            Ok((x, y)) => json!({"answer":[x,y],"error":null}),
            Err(error) => json!({"answer":null,"error":error}),
        };
        if actual["error"].is_null() {
            assert!(case["expected"]["error"].is_null());
            for coordinate in 0..2 {
                assert_eq!(
                    actual["answer"][coordinate].as_f64(),
                    case["expected"]["answer"][coordinate].as_f64()
                );
            }
        } else {
            assert_eq!(actual, case["expected"])
        }
    }
    for case in document["ctc"].as_array().unwrap() {
        let rows = case["rows"].as_array().unwrap();
        let values: Vec<f32> = rows
            .iter()
            .flat_map(|r| {
                r.as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap() as f32)
            })
            .collect();
        let result = ctc::decode(
            &values,
            &[1, rows.len() as i64, 3],
            &["".into(), "a".into(), "b".into()],
        );
        let actual = match result {
            Ok((answer, confidence)) => {
                json!({"answer":answer,"confidence":confidence,"error":null})
            }
            Err(error) => json!({"answer":null,"error":error}),
        };
        assert_eq!(actual, case["expected"])
    }
}

#[tokio::test]
async fn sdk_matches_generated_source_results() {
    let document: Value = serde_json::from_str(include_str!("fixtures/parity/sdk.json")).unwrap();
    let cases = document["cases"].as_array().unwrap();
    assert!(!cases.is_empty());
    let root = tempfile::tempdir().unwrap();
    let mut settings = Settings::new(root.path().into());
    settings.device = "cuda".into(); // Frozen source fixture uses CUDA configuration; no inference in this arithmetic tier.
    let sdk = Numera::new(settings);
    for case in cases {
        let actual =
            serde_json::to_value(sdk.recognize_value(case["input"].clone(), None).await).unwrap();
        assert_eq!(actual, case["expected"], "source SDK case {}", case["name"]);
    }
    sdk.close().await;
}
