use serde_json::{Value, json};
use std::{
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn run(root: &std::path::Path, args: &[&str]) -> (i32, Value) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_numera-cha"));
    command
        .args(args)
        .env("NUMERA_ROOT", root)
        .env_remove("NUMERA_API_TOKEN")
        .env_remove("NUMERA_DLL_DIRECTORY")
        .env_remove("NUMERA_DEVICE")
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().unwrap();
    let start = Instant::now();
    loop {
        match child.try_wait().unwrap() {
            Some(status) => {
                let mut output = String::new();
                child
                    .stdout
                    .take()
                    .unwrap()
                    .read_to_string(&mut output)
                    .unwrap();
                return (
                    status.code().unwrap_or(-1),
                    serde_json::from_str(&output).unwrap(),
                );
            }
            None if start.elapsed() < Duration::from_secs(5) => {
                std::thread::sleep(Duration::from_millis(5))
            }
            None => {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("CLI exceeded its five-second local tracer budget.");
            }
        }
    }
}
#[test]
fn cli_json_exit_codes_and_saved_configuration_failures() {
    let root = tempfile::tempdir().unwrap();
    let (code, result) = run(root.path(), &["recognize", "--text", "12 * 3 = ?"]);
    assert_eq!(code, 0);
    assert_eq!(result["answer"], "36");
    assert_eq!(result["server_verified"], false);
    let (code, result) = run(root.path(), &["recognize", "--text", "9/0"]);
    assert_eq!(code, 4);
    assert_eq!(result["error"]["code"], "invalid_expression");
    std::fs::create_dir_all(root.path().join("data")).unwrap();
    for content in ["{", "null", "{\"dll_directory\":7}"] {
        std::fs::write(
            root.path().join("data/runtime-config.json"),
            content.as_bytes(),
        )
        .unwrap();
        let (code, result) = run(root.path(), &["capabilities"]);
        assert_eq!(code, 2);
        assert_eq!(result["error"]["code"], "invalid_config");
        assert!(
            std::fs::read_dir(root.path().join("logs"))
                .unwrap()
                .any(|p| p.unwrap().path().extension().is_some_and(|e| e == "log"))
        );
    }
}
#[test]
fn image_character_filter_is_not_a_second_exclusive_input() {
    use clap::Parser;
    use numera_cha_solver::cli::Cli;
    assert!(
        Cli::try_parse_from([
            "numera-cha",
            "recognize",
            "--image",
            "sample.png",
            "--characters",
            "ABC"
        ])
        .is_ok()
    );
    assert!(
        Cli::try_parse_from([
            "numera-cha",
            "recognize",
            "--text",
            "1+2",
            "--image",
            "sample.png"
        ])
        .is_err()
    );
    assert!(Cli::try_parse_from(["numera-cha", "recognize", "--characters", "ABC"]).is_err());
}

#[tokio::test]
async fn sdk_unknown_and_expired_requests_remain_nonrecognized() {
    use numera_cha_solver::{config::Settings, core::contracts::Input, sdk::Numera};
    let root = tempfile::tempdir().unwrap();
    let sdk = Numera::new(Settings::new(root.path().into()));
    let input = Input::from_value(json!({"kind":"text","text":"1+2"})).unwrap();
    assert_eq!(
        sdk.recognize(input, Some(tokio::time::Instant::now()))
            .await
            .outcome,
        "expired"
    );
    let unsupported = sdk.recognize_value(json!({"kind":"audio"}), None).await;
    assert_eq!(unsupported.outcome, "unsupported");
    assert!(!unsupported.server_verified);
    sdk.close().await;
}
