use numera_cha_solver::{assets, config::Settings, core::contracts::Input, sdk::Numera};
use std::{path::PathBuf, time::Duration};

#[tokio::test]
async fn owned_real_gpu_then_cpu_answer_and_shape_parity() {
    assert_eq!(
        std::env::var("NUMERA_TEST_GPU").as_deref(),
        Ok("1"),
        "Required GPU tier must be selected; no skipped pass."
    );
    let root = PathBuf::from(
        std::env::var_os("NUMERA_RUNTIME_ROOT").expect("Owned runtime/model root required."),
    );
    assert!(
        std::env::var_os("NUMERA_WORKER_EXECUTABLE").is_none(),
        "SDK consumer tier must exercise native worker resolution, not a masking CI override."
    );
    let mut settings = Settings::new(root.clone());
    settings.dll_directory = std::env::var("NUMERA_DLL_DIRECTORY").unwrap_or_default();
    settings.ort_library = PathBuf::from(
        std::env::var_os("NUMERA_ORT_LIBRARY").expect("Native runtime library required."),
    );
    let content = assets::read_asset(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr/ascii-smoke.png"),
        &settings,
    )
    .unwrap();
    let input = Input {
        kind: "image".into(),
        asset: content,
        text: String::new(),
        subtype: "auto".into(),
        allowed_characters: String::new(),
        revision: String::new(),
    };
    let sdk = Numera::new(settings.clone());
    let gpu = tokio::time::timeout(Duration::from_secs(60), sdk.recognize(input.clone(), None))
        .await
        .unwrap();
    sdk.close().await;
    assert_eq!(gpu.answer.as_deref(), Some("NUMERA 123"));
    assert_eq!(gpu.device.as_deref(), Some("cuda"));
    assert!(!gpu.server_verified);
    assert!(
        gpu.evidence["profile_node_counts"]["CUDAExecutionProvider"]
            .as_u64()
            .unwrap_or(0)
            > 0
    );
    settings.device = "cpu".into();
    let sdk = Numera::new(settings);
    let cpu = tokio::time::timeout(Duration::from_secs(60), sdk.recognize(input, None))
        .await
        .unwrap();
    sdk.close().await;
    assert_eq!(cpu.answer, gpu.answer);
    assert_eq!(cpu.device.as_deref(), Some("cpu"));
    assert_eq!(cpu.evidence["input_shape"], gpu.evidence["input_shape"]);
    assert!(!cpu.server_verified);
}
