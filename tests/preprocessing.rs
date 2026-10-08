use numera_cha_solver::{config::Settings, engines::preprocess::preprocess};
use std::path::PathBuf;

#[test]
fn tensor_matches_pinned_pillow_source_bytes() {
    let settings = Settings::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    let (actual, shape) =
        preprocess(include_bytes!("fixtures/ocr/ascii-smoke.png"), &settings).unwrap();
    let expected = include_bytes!("fixtures/parity/ascii-preprocess.f32");
    assert!(!expected.is_empty());
    assert_eq!(shape, [1, 3, 48, 320]);
    let bytes: Vec<u8> = actual.iter().flat_map(|v| v.to_le_bytes()).collect();
    assert_eq!(bytes.len(), expected.len());
    assert!(
        bytes.iter().zip(expected).all(|(a, b)| a == b),
        "Pillow bilinear/channel/FP32 normalization must match source bytes."
    );
}
