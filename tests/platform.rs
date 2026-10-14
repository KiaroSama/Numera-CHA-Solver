use numera_cha_solver::{config::Settings, startup};

#[test]
fn platform_runtime_name_and_default_device_are_explicit() {
    let root = tempfile::tempdir().unwrap();
    let settings = Settings::new(root.path().into());
    assert_eq!(
        settings.ort_library.file_name().unwrap(),
        numera_cha_solver::config::runtime_library_name()
    );
    assert_eq!(
        settings.device,
        if cfg!(target_os = "macos") {
            "cpu"
        } else {
            "cuda"
        }
    );
    assert_eq!(
        numera_cha_solver::config::worker_binary_name(),
        if cfg!(windows) {
            "numera-cha.exe"
        } else {
            "numera-cha"
        }
    );
}
#[test]
fn credential_is_private_and_replacement_remains_valid() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("credential");
    startup::publish_credential(&path, "owned-synthetic-original").unwrap();
    startup::publish_credential(&path, "owned-synthetic-replacement").unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "owned-synthetic-replacement"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
