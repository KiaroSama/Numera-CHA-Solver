use crate::{
    assets::{is_link, open_regular},
    config::Settings,
    core::contracts::Error,
    logging,
    ownership::Ownership,
};
use reqwest::{Client, Url, redirect::Policy};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::io::AsyncReadExt;

const ORIGINS: &[&str] = &[
    "huggingface.co",
    "raw.githubusercontent.com",
    "cdn-lfs.huggingface.co",
    "cdn-lfs-us-1.hf.co",
    "cas-bridge.xethub.hf.co",
    "us.aws.cdn.hf.co",
];

#[derive(Clone, Debug)]
pub struct ModelRegistry {
    settings: Settings,
    models: Result<Vec<Value>, Error>,
}

impl ModelRegistry {
    pub fn new(settings: Settings) -> Self {
        let models = serde_json::from_str::<Value>(include_str!("../resources/models.json"))
            .map_err(|_| manifest_error())
            .and_then(|document| {
                document
                    .get("models")
                    .and_then(Value::as_array)
                    .cloned()
                    .ok_or_else(manifest_error)
            });
        Self { settings, models }
    }

    pub fn get(&self, id: &str) -> Result<Value, Error> {
        self.models
            .as_ref()
            .map_err(Clone::clone)?
            .iter()
            .find(|model| model["id"].as_str() == Some(id))
            .cloned()
            .ok_or_else(|| Error::new("unknown_model", "Model is not in the reviewed manifest."))
    }

    pub fn directory(&self, id: &str) -> Result<PathBuf, Error> {
        let model = self.get(id)?;
        let revision = text(&model, "revision")?;
        if !segment(id) || !segment(revision) {
            return Err(manifest_error());
        }
        let directory = self.settings.model_dir().join(id).join(revision);
        ensure_directory_path(&directory)?;
        Ok(directory)
    }

    pub fn verify(&self, id: &str) -> Result<Value, Error> {
        self.settings.validate()?;
        let model = self.get(id)?;
        let directory = self.directory(id)?;
        let deadline = Instant::now() + Duration::from_secs_f64(self.settings.deadline_seconds);
        let mut failures = Vec::new();
        for asset in assets(&model)? {
            let path = asset_path(&directory, asset)?;
            if let Some(reason) = inspect_asset(&path, asset, deadline)? {
                failures.push(json!({"file": text(asset, "file")?, "reason": reason}));
            }
        }
        Ok(status(id, &model, failures))
    }

    pub fn list(&self) -> Result<Vec<Value>, Error> {
        self.models
            .as_ref()
            .map_err(Clone::clone)?
            .iter()
            .map(|model| self.verify(text(model, "id")?))
            .collect()
    }

    pub async fn fetch(&self, id: &str) -> Result<Value, Error> {
        let client = Client::builder()
            .redirect(Policy::none())
            .no_proxy()
            .https_only(true)
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| download_error())?;
        self.fetch_with(id, move |asset, path, deadline| {
            let client = client.clone();
            async move { download(&client, &asset, &path, deadline).await }
        })
        .await
    }

    /// Supply an owned transport; every asset still passes immutable post-download integrity checks.
    pub async fn fetch_with<F, Fut>(&self, id: &str, transport: F) -> Result<Value, Error>
    where
        F: Fn(Value, PathBuf, Instant) -> Fut,
        Fut: std::future::Future<Output = Result<(), Error>>,
    {
        self.settings.validate()?;
        let model = self.get(id)?;
        let directory = self.directory(id)?;
        tokio::fs::create_dir_all(&directory)
            .await
            .map_err(|_| download_error())?;
        ensure_directory_path(&directory)?;
        let _lease = Ownership::acquire(&directory.join(".download.lock")).map_err(|error| {
            if error.code == "ownership_busy" {
                Error::new(
                    "model_busy",
                    "Another model download owns this revision; retry after it completes.",
                )
            } else {
                download_error()
            }
        })?;
        let deadline = Instant::now() + Duration::from_secs_f64(self.settings.deadline_seconds);
        for asset in assets(&model)? {
            remaining(deadline)?;
            let path = asset_path(&directory, asset)?;
            if inspect_asset_async(&path, asset, deadline).await?.is_none() {
                continue;
            }
            tokio::time::timeout(
                remaining(deadline)?,
                transport(asset.clone(), path, deadline),
            )
            .await
            .map_err(|_| expired())??;
        }
        let mut failures = Vec::new();
        for asset in assets(&model)? {
            let path = asset_path(&directory, asset)?;
            if let Some(reason) = inspect_asset_async(&path, asset, deadline).await? {
                failures.push(json!({"file":text(asset, "file")?, "reason":reason}));
            }
        }
        logging::event(
            "INFO",
            "model",
            &format!(
                "Model acquisition completed id={id} revision={}",
                text(&model, "revision")?
            ),
        );
        Ok(status(id, &model, failures))
    }
}

fn status(id: &str, model: &Value, failures: Vec<Value>) -> Value {
    let availability = if failures.is_empty() {
        "ready"
    } else if failures
        .iter()
        .all(|failure| failure["reason"] == "missing")
    {
        "model_missing"
    } else {
        "model_corrupt"
    };
    json!({"id":id, "revision":model["revision"], "license":model["license"], "availability":availability, "failures":failures})
}
fn assets(model: &Value) -> Result<&Vec<Value>, Error> {
    model["assets"]
        .as_array()
        .filter(|items| !items.is_empty())
        .ok_or_else(manifest_error)
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, Error> {
    value[key].as_str().ok_or_else(manifest_error)
}
fn size(asset: &Value) -> Result<u64, Error> {
    asset["size"]
        .as_u64()
        .filter(|v| *v > 0)
        .ok_or_else(manifest_error)
}
fn segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_'))
        && !matches!(value, "." | "..")
}
fn asset_path(directory: &Path, asset: &Value) -> Result<PathBuf, Error> {
    let filename = text(asset, "file")?;
    if !segment(filename) {
        return Err(manifest_error());
    }
    Ok(directory.join(filename))
}
fn ensure_directory_path(path: &Path) -> Result<(), Error> {
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(path_error());
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if is_link(&metadata) || !metadata.is_dir() => return Err(path_error()),
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(path_error()),
            _ => {}
        }
    }
    Ok(())
}
fn initial_asset(path: &Path, asset: &Value) -> Result<Result<fs::File, &'static str>, Error> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Err("missing")),
        Err(_) => {
            return Err(Error::new(
                "model_corrupt",
                "Model asset could not be safely inspected.",
            ));
        }
    };
    if is_link(&metadata) {
        return Ok(Err(if path.is_file() {
            "size_or_path_mismatch"
        } else {
            "missing"
        }));
    }
    if !metadata.is_file() {
        return Ok(Err("missing"));
    }
    if metadata.len() != size(asset)? {
        return Ok(Err("size_or_path_mismatch"));
    }
    let file = open_regular(path).map_err(|_| path_error())?;
    if file.metadata().map_err(|_| path_error())?.len() != size(asset)? {
        return Ok(Err("size_or_path_mismatch"));
    }
    Ok(Ok(file))
}
fn inspect_asset(
    path: &Path,
    asset: &Value,
    deadline: Instant,
) -> Result<Option<&'static str>, Error> {
    let mut file = match initial_asset(path, asset)? {
        Ok(file) => file,
        Err(reason) => return Ok(Some(reason)),
    };
    let mut digest = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        remaining(deadline)?;
        let read = file.read(&mut buffer).map_err(|_| path_error())?;
        if read == 0 {
            break;
        }
        count += read as u64;
        if count > size(asset)? {
            return Ok(Some("size_or_path_mismatch"));
        }
        digest.update(&buffer[..read]);
    }
    if count != size(asset)? {
        return Ok(Some("size_or_path_mismatch"));
    }
    Ok((digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        != text(asset, "sha256")?)
    .then_some("checksum_mismatch"))
}
async fn inspect_asset_async(
    path: &Path,
    asset: &Value,
    deadline: Instant,
) -> Result<Option<&'static str>, Error> {
    let file = match initial_asset(path, asset)? {
        Ok(file) => file,
        Err(reason) => return Ok(Some(reason)),
    };
    let mut file = tokio::fs::File::from_std(file);
    let mut digest = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = tokio::time::timeout(remaining(deadline)?, file.read(&mut buffer))
            .await
            .map_err(|_| expired())?
            .map_err(|_| path_error())?;
        if read == 0 {
            break;
        }
        count += read as u64;
        if count > size(asset)? {
            return Ok(Some("size_or_path_mismatch"));
        }
        digest.update(&buffer[..read]);
    }
    if count != size(asset)? {
        return Ok(Some("size_or_path_mismatch"));
    }
    Ok((digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        != text(asset, "sha256")?)
    .then_some("checksum_mismatch"))
}

async fn download(
    client: &Client,
    asset: &Value,
    destination: &Path,
    deadline: Instant,
) -> Result<(), Error> {
    let mut url = Url::parse(text(asset, "url")?).map_err(|_| origin_error())?;
    let mut redirects = 0;
    loop {
        validate_url(&url)?;
        let request = client.get(url.clone()).timeout(remaining(deadline)?);
        let mut response = request.send().await.map_err(|_| network_error(deadline))?;
        if matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
            redirects += 1;
            if redirects > 3 {
                return Err(Error::new(
                    "download_redirects",
                    "Too many model redirects.",
                ));
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(download_error)?;
            url = url.join(location).map_err(|_| origin_error())?;
            continue;
        }
        if !response.status().is_success() {
            return Err(download_error());
        }
        if response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.to_ascii_lowercase().contains("text/html"))
        {
            return Err(Error::new(
                "invalid_model",
                "An HTML page is not a model asset.",
            ));
        }
        let expected_size = size(asset)?;
        if response
            .content_length()
            .is_some_and(|length| length != expected_size)
        {
            return Err(Error::new(
                "invalid_model",
                "Downloaded asset does not match its immutable manifest.",
            ));
        }
        let parent = destination.parent().ok_or_else(path_error)?;
        ensure_directory_path(parent)?;
        let (temporary, mut file) = temporary_file(parent)?;
        let mut digest = Sha256::new();
        let mut received = 0u64;
        while let Some(chunk) = tokio::time::timeout(
            remaining(deadline)?.min(Duration::from_secs(30)),
            response.chunk(),
        )
        .await
        .map_err(|_| network_error(deadline))?
        .map_err(|_| network_error(deadline))?
        {
            received = received
                .checked_add(chunk.len() as u64)
                .ok_or_else(invalid_model)?;
            if received > size(asset)? {
                return Err(Error::new(
                    "invalid_model",
                    "Downloaded asset exceeds manifest size.",
                ));
            }
            digest.update(&chunk);
            remaining(deadline)?;
            file.write_all(&chunk).map_err(|_| download_error())?;
        }
        file.flush().map_err(|_| download_error())?;
        file.sync_all().map_err(|_| download_error())?;
        drop(file);
        if received != size(asset)?
            || digest
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
                != text(asset, "sha256")?
        {
            return Err(invalid_model());
        }
        remaining(deadline)?;
        ensure_directory_path(parent)?;
        if fs::symlink_metadata(destination).is_ok_and(|m| is_link(&m) || !m.is_file()) {
            return Err(path_error());
        }
        atomic_replace(&temporary.0, destination).map_err(|_| download_error())?;
        return Ok(());
    }
}
fn validate_url(url: &Url) -> Result<(), Error> {
    if url.scheme() != "https"
        || !url.host_str().is_some_and(|host| ORIGINS.contains(&host))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 443)
    {
        return Err(origin_error());
    }
    Ok(())
}
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn temporary_file(directory: &Path) -> Result<(Temporary, fs::File), Error> {
    for _ in 0..8 {
        let path = directory.join(format!(".download-{:016x}.tmp", rand::random::<u64>()));
        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
        {
            Ok(file) => return Ok((Temporary(path), file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(download_error()),
        }
    }
    Err(download_error())
}
fn atomic_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let destination: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        // SAFETY: both slices are live NUL-terminated UTF-16 paths; flags replace only this checked asset.
        if unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::rename(source, destination)
    }
}
fn remaining(deadline: Instant) -> Result<Duration, Error> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|v| !v.is_zero())
        .ok_or_else(expired)
}
fn network_error(deadline: Instant) -> Error {
    if Instant::now() >= deadline {
        expired()
    } else {
        download_error()
    }
}
fn manifest_error() -> Error {
    Error::new("invalid_model", "Reviewed model manifest is invalid.")
}
fn path_error() -> Error {
    Error::new(
        "model_corrupt",
        "Model asset path or integrity could not be safely inspected.",
    )
}
fn origin_error() -> Error {
    Error::new(
        "download_origin",
        "Model redirect is outside reviewed asset origins.",
    )
}
fn download_error() -> Error {
    Error::new(
        "download_failed",
        "Model download failed; no incomplete asset was published.",
    )
}
fn invalid_model() -> Error {
    Error::new(
        "invalid_model",
        "Downloaded asset does not match its immutable manifest.",
    )
}
fn expired() -> Error {
    Error::new(
        "expired",
        "Model acquisition or integrity verification deadline exhausted.",
    )
}
