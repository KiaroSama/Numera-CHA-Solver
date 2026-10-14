use crate::core::contracts::Error;
use serde_json::Value;
use std::{
    env,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct Settings {
    pub root: PathBuf,
    pub device: String,
    pub deadline_seconds: f64,
    pub max_asset_bytes: usize,
    pub max_pixels: u64,
    pub queue_size: usize,
    pub result_ttl: u64,
    pub api_token: String,
    pub dll_directory: String,
    pub confidence_threshold: f64,
    pub ort_library: PathBuf,
    pub desktop_origin: Option<String>,
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Settings")
            .field("root", &self.root)
            .field("device", &self.device)
            .field("deadline_seconds", &self.deadline_seconds)
            .field("max_asset_bytes", &self.max_asset_bytes)
            .field("max_pixels", &self.max_pixels)
            .field("queue_size", &self.queue_size)
            .field("result_ttl", &self.result_ttl)
            .field("api_token", &"[REDACTED]")
            .field("dll_directory", &self.dll_directory)
            .field("confidence_threshold", &self.confidence_threshold)
            .field("ort_library", &self.ort_library)
            .finish()
    }
}

impl Settings {
    pub fn new(root: PathBuf) -> Self {
        let ort_library = root.join("data/runtime").join(runtime_library_name());
        Self {
            root,
            device: default_device().into(),
            deadline_seconds: 120.0,
            max_asset_bytes: 8 * 1024 * 1024,
            max_pixels: 16_000_000,
            queue_size: 32,
            result_ttl: 86_400,
            api_token: String::new(),
            dll_directory: String::new(),
            confidence_threshold: 0.7,
            ort_library,
            desktop_origin: None,
        }
    }

    pub fn from_env() -> Result<Self, Error> {
        let root = if let Some(value) = env::var_os("NUMERA_ROOT").filter(|v| !v.is_empty()) {
            let expanded = expand_home(Path::new(&value))?;
            std::path::absolute(expanded)
                .map_err(|_| config_error("Application root could not be resolved."))?
        } else {
            project_root()?
        };
        let mut settings = Self::new(root);
        settings.device = env_text("NUMERA_DEVICE", default_device())?;
        settings.api_token = env_text("NUMERA_API_TOKEN", "")?;
        settings.dll_directory = env_text("NUMERA_DLL_DIRECTORY", "")?;
        if settings.dll_directory.is_empty() {
            let path = settings.data_dir().join("runtime-config.json");
            if path.is_file() {
                let mut content = Vec::new();
                File::open(path)
                    .and_then(|file| file.take(64 * 1024 + 1).read_to_end(&mut content))
                    .map_err(|_| saved_config_error())?;
                if content.len() > 64 * 1024 {
                    return Err(saved_config_error());
                }
                let document: Value =
                    serde_json::from_slice(&content).map_err(|_| saved_config_error())?;
                let object = document.as_object().ok_or_else(saved_config_error)?;
                settings.dll_directory = match object.get("dll_directory") {
                    None => String::new(),
                    Some(Value::String(value)) => value.clone(),
                    _ => return Err(saved_config_error()),
                };
            }
        }
        if let Some(path) = env::var_os("NUMERA_ORT_LIBRARY").filter(|v| !v.is_empty()) {
            settings.ort_library = expand_home(Path::new(&path))?;
            if settings.ort_library.is_relative() {
                settings.ort_library = settings.root.join(&settings.ort_library);
            }
        }
        settings.validate()?;
        Ok(settings)
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.root.as_os_str().is_empty() {
            return Err(config_error("Application root must be a local path."));
        }
        if self.dll_directory.chars().count() > 4096 || self.dll_directory.contains('\0') {
            return Err(config_error(
                "Runtime library directory must be a bounded path string.",
            ));
        }
        if !self.dll_directory.is_empty() && !Path::new(&self.dll_directory).is_dir() {
            return Err(config_error("Runtime library directory is unavailable."));
        }
        if !matches!(self.device.as_str(), "cuda" | "cpu") {
            return Err(config_error("Device must be explicitly cuda or cpu."));
        }
        if !(1.0..=120.0).contains(&self.deadline_seconds) || !(1..=64).contains(&self.queue_size) {
            return Err(config_error(
                "Deadline or queue limit is outside the supported bounds.",
            ));
        }
        if !(0.0..=1.0).contains(&self.confidence_threshold) {
            return Err(config_error(
                "Confidence threshold must be between 0 and 1.",
            ));
        }
        if !(1..=8 * 1024 * 1024).contains(&self.max_asset_bytes)
            || !(1..=16_000_000).contains(&self.max_pixels)
        {
            return Err(config_error(
                "Asset limits cannot exceed the application safety ceiling.",
            ));
        }
        if !(60..=86_400).contains(&self.result_ttl) {
            return Err(config_error(
                "Result TTL must be between 60 and 86400 seconds.",
            ));
        }
        if self.ort_library.as_os_str().is_empty() || self.ort_library.as_os_str().len() > 4096 {
            return Err(config_error(
                "ONNX Runtime library must be an explicit bounded local path.",
            ));
        }
        Ok(())
    }

    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }
    pub fn model_dir(&self) -> PathBuf {
        self.data_dir().join("models")
    }
    pub fn log_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
}

fn env_text(key: &str, default: &str) -> Result<String, Error> {
    match env::var(key) {
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(default.into()),
        Err(_) => Err(config_error(
            "Runtime environment contains invalid Unicode.",
        )),
    }
}
fn config_error(message: &str) -> Error {
    Error::new("invalid_config", message)
}
fn saved_config_error() -> Error {
    config_error("Saved runtime configuration is invalid or unreadable.")
}
fn home() -> Result<PathBuf, Error> {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| config_error("Home directory is unavailable; configure NUMERA_ROOT."))
}
fn expand_home(path: &Path) -> Result<PathBuf, Error> {
    if let Ok(suffix) = path.strip_prefix("~") {
        return Ok(home()?.join(suffix));
    }
    Ok(path.to_owned())
}
fn project_root() -> Result<PathBuf, Error> {
    let executable =
        env::current_exe().map_err(|_| config_error("Application location is unavailable."))?;
    for parent in executable.ancestors().skip(1) {
        if parent.join("Cargo.toml").is_file() || parent.join("resources/models.json").is_file() {
            return Ok(parent.to_owned());
        }
    }
    Ok(home()?.join(".numera-cha"))
}

pub fn runtime_library_name() -> &'static str {
    if cfg!(windows) {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    }
}
pub fn worker_binary_name() -> &'static str {
    if cfg!(windows) {
        "numera-cha.exe"
    } else {
        "numera-cha"
    }
}
fn default_device() -> &'static str {
    if cfg!(target_os = "macos") {
        "cpu"
    } else {
        "cuda"
    }
}
