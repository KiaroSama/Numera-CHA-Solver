use numera_cha_solver::{
    assets, capabilities, config::Settings, logging, models::ModelRegistry, ownership::Ownership,
};
use serde_json::json;
use std::{fs, io::Cursor};

#[test]
fn configuration_bounds_and_secret_safe_debug() {
    let root = tempfile::tempdir().expect("isolated root");
    let settings = Settings::new(root.path().into());
    assert_eq!(
        settings.device,
        if cfg!(target_os = "macos") {
            "cpu"
        } else {
            "cuda"
        }
    );
    assert_eq!(
        settings.ort_library,
        root.path()
            .join("data/runtime")
            .join(numera_cha_solver::config::runtime_library_name())
    );
    assert_eq!(settings.model_dir(), root.path().join("data/models"));
    assert_eq!(settings.log_dir(), root.path().join("logs"));
    settings.validate().expect("defaults valid");
    for value in [f64::NAN, f64::INFINITY, 0.0, 121.0] {
        let mut invalid = settings.clone();
        invalid.deadline_seconds = value;
        assert_eq!(
            invalid.validate().expect_err("deadline rejected").code,
            "invalid_config"
        );
    }
    let mut invalid = settings.clone();
    invalid.confidence_threshold = f64::NAN;
    assert_eq!(
        invalid.validate().expect_err("confidence rejected").code,
        "invalid_config"
    );
    invalid = settings.clone();
    invalid.queue_size = 65;
    assert!(invalid.validate().is_err());
    invalid = settings.clone();
    invalid.result_ttl = 59;
    assert!(invalid.validate().is_err());
    invalid = settings.clone();
    invalid.max_pixels = 16_000_001;
    assert!(invalid.validate().is_err());
    invalid = settings.clone();
    invalid.max_asset_bytes = 8 * 1024 * 1024 + 1;
    assert!(invalid.validate().is_err());
    invalid = settings.clone();
    invalid.api_token = "synthetic-debug-sentinel".into();
    assert!(!format!("{invalid:?}").contains("synthetic-debug-sentinel"));
}

#[test]
fn still_images_white_alpha_and_dimension_bounds() {
    let root = tempfile::tempdir().expect("isolated root");
    let settings = Settings::new(root.path().into());
    for invalid in [b"".as_slice(), b"not an image", b"<html>error</html>"] {
        assert_eq!(
            assets::image(invalid, &settings)
                .expect_err("invalid image")
                .code,
            "invalid_asset"
        );
    }
    let mut encoded = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        30,
        20,
        image::Rgba([0, 0, 0, 0]),
    ))
    .write_to(&mut encoded, image::ImageFormat::Png)
    .expect("encode small fixture");
    let path = root.path().join("local sample فارسی.png");
    fs::write(&path, encoded.get_ref()).expect("write fixture");
    let content = assets::read_asset(&path, &settings).expect("read local image");
    let decoded = assets::image(&content, &settings).expect("decode fixture");
    assert_eq!(decoded.dimensions(), (30, 20));
    assert!(decoded.pixels().all(|p| p.0 == [255, 255, 255]));
    let mut bounded = settings.clone();
    bounded.max_pixels = 100;
    assert_eq!(
        assets::image(&content, &bounded)
            .expect_err("pixel bound")
            .code,
        "invalid_asset"
    );
    bounded.max_asset_bytes = content.len() - 1;
    assert_eq!(
        assets::read_asset(&path, &bounded)
            .expect_err("byte bound")
            .code,
        "asset_too_large"
    );
    assert_eq!(
        assets::read_asset(root.path(), &settings)
            .expect_err("directory rejected")
            .code,
        "invalid_asset"
    );
}

#[test]
fn manifest_missing_and_corrupt_outcomes_do_not_acquire_assets() {
    let root = tempfile::tempdir().expect("isolated root");
    let settings = Settings::new(root.path().into());
    let registry = ModelRegistry::new(settings.clone());
    let missing = registry.verify("ppocr-v6-small").expect("reviewed model");
    assert_eq!(missing["availability"], "model_missing");
    assert_eq!(missing["failures"].as_array().expect("failures").len(), 3);
    assert!(!settings.model_dir().exists());
    let directory = registry.directory("ppocr-v6-small").expect("reviewed path");
    fs::create_dir_all(&directory).expect("fixture directory");
    fs::write(directory.join("inference.onnx"), b"not reviewed weights").expect("corrupt fixture");
    let corrupt = registry
        .verify("ppocr-v6-small")
        .expect("inspect corrupt fixture");
    assert_eq!(corrupt["availability"], "model_corrupt");
    assert_eq!(
        corrupt["failures"][0],
        json!({"file":"inference.onnx","reason":"size_or_path_mismatch"})
    );
    assert_eq!(
        registry
            .get("../unknown")
            .expect_err("unreviewed model")
            .code,
        "unknown_model"
    );
    let document = capabilities::capabilities(&settings).expect("capabilities");
    for row in document["capabilities"]
        .as_array()
        .expect("capability rows")
    {
        if row["model_id"] == "ppocr-v6-small" {
            assert_eq!(row["availability"], "model_corrupt");
        }
    }
}

#[test]
fn ownership_is_a_live_handle_not_a_stale_file() {
    let root = tempfile::tempdir().expect("isolated root");
    let path = root.path().join("ownership.lock");
    fs::write(&path, b"stale").expect("stale lock marker");
    let mut first = Ownership::acquire(&path).expect("acquire stale file");
    assert_eq!(
        Ownership::acquire(&path).expect_err("live contender").code,
        "ownership_busy"
    );
    first.close();
    drop(Ownership::acquire(&path).expect("released owner"));
    assert_eq!(fs::read(&path).expect("marker preserved"), b"stale");
}

#[tokio::test]
async fn model_download_live_owner_fails_before_network() {
    let root = tempfile::tempdir().expect("isolated root");
    let registry = ModelRegistry::new(Settings::new(root.path().into()));
    let path = registry
        .directory("ppocr-v6-small")
        .expect("reviewed path")
        .join(".download.lock");
    let lease = Ownership::acquire(&path).expect("own revision");
    assert_eq!(
        registry
            .fetch("ppocr-v6-small")
            .await
            .expect_err("busy before network")
            .code,
        "model_busy"
    );
    drop(lease);
}

#[test]
fn log_runs_are_unique_redacted_utf8_and_closed() {
    let root = tempfile::tempdir().expect("isolated root");
    let sentinel = "owned-synthetic-diagnostic-value";
    let mut paths = Vec::new();
    for _ in 0..2 {
        let guard = logging::initialize(root.path(), "numera-cha", true);
        paths.push(guard.path().expect("file logger").to_owned());
        logging::event(
            "DEBUG",
            "config",
            &format!("token={sentinel} password={sentinel}"),
        );
        logging::event(
            "WARNING",
            "config",
            &format!("password=\"multi line\n{sentinel}\""),
        );
        logging::event(
            "ERROR",
            "runtime",
            &json!({"nested":{"api_key":sentinel}}).to_string(),
        );
        logging::event(
            "INFO",
            "runtime",
            &format!("Bearer {sentinel} https://example.com/private"),
        );
        logging::event("INFO", "cleanup", "cleanup finished فارسی");
        drop(guard);
    }
    assert_ne!(paths[0], paths[1]);
    for path in &paths {
        let text = fs::read_to_string(path).expect("UTF-8 log");
        assert!(!text.contains(sentinel));
        assert!(!text.contains("example.com"));
        for marker in [
            "[DEBUG]",
            "[WARNING]",
            "[ERROR]",
            "UTC]",
            "[REDACTED]",
            "cleanup finished فارسی",
        ] {
            assert!(text.contains(marker), "missing {marker}");
        }
    }
    let mut old = logging::initialize(root.path(), "numera-cha", false);
    let current = logging::initialize(root.path(), "numera-cha", false);
    let current_path = current.path().expect("current logger").to_owned();
    old.close();
    logging::event("INFO", "runtime", "replacement logger remains active");
    drop(current);
    assert!(
        fs::read_to_string(&current_path)
            .expect("current log")
            .contains("replacement logger remains active")
    );
    // On Windows directory cleanup verifies that every owned log handle has closed.
    fs::remove_dir_all(root.path()).expect("closed Windows file handles");
}
