use crate::{config::Settings, core::contracts::Error, models::ModelRegistry};
use serde_json::{Value, json};
use std::{
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub fn doctor(settings: &Settings) -> Result<Value, Error> {
    settings.validate()?;
    Ok(json!({
        "platform": if cfg!(windows) { "Windows" } else { std::env::consts::OS },
        "application_version": env!("CARGO_PKG_VERSION"),
        "rust_minimum_version": env!("CARGO_PKG_RUST_VERSION"),
        "runtime_versions": {
            "onnxruntime": null,
            "ort_binding": dependency_version("ort"),
            "image": dependency_version("image"),
            "axum": dependency_version("axum"),
        },
        "native_runtime_library_present": settings.ort_library.is_file(),
        "configured_device": settings.device,
        "gpu_inventory": gpu_inventory(),
        "models": ModelRegistry::new(settings.clone()).list()?,
        "gpu_execution_verified": false,
        "note": "Inventory is not inference evidence. Native ONNX Runtime version is not inferred from its filename or binding version. Run doctor --smoke with a licensed local image."
    }))
}

fn dependency_version(name: &str) -> Option<&'static str> {
    include_str!("../Cargo.toml").lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        if key.trim() != name {
            return None;
        }
        let quoted = value.split('"').nth(1)?;
        Some(quoted.trim_start_matches('='))
    })
}

fn gpu_inventory() -> Vec<Vec<String>> {
    let mut command = Command::new("nvidia-smi");
    command
        .args([
            "--query-gpu=name,memory.total,memory.free,driver_version",
            "--format=csv,noheader,nounits",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    let Ok(mut child) = command.spawn() else {
        return Vec::new();
    };
    let Some(output) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Vec::new();
    };
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        output
            .take(64 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let success = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Err(_) => break false,
            Ok(None) if Instant::now() >= deadline => break false,
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    if !success {
        let _ = child.kill();
    }
    let _ = child.wait();
    let Ok(Ok(bytes)) = reader.join() else {
        return Vec::new();
    };
    if !success || bytes.len() > 64 * 1024 {
        return Vec::new();
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return Vec::new();
    };
    text.trim()
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| line.split(", ").map(str::to_owned).collect())
        .collect()
}
