//! Hosted distribution contracts. No extraction, signing keys, subprocesses or network access.
mod archives;
mod paths;
mod payload;

pub use archives::{create_archive, verify_package, verify_source};
use paths::{MAX_JSON, Record, inventory, regular};
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
pub const PLATFORMS: [&str; 3] = ["windows-x64", "linux-x64", "macos-arm64"];
pub const MANIFEST: &str = "package-manifest.json";

pub struct Identity<'a> {
    pub platform: &'a str,
    pub kind: &'a str,
    pub source: &'a str,
    pub version: &'a str,
    pub signing: &'a str,
}
impl Identity<'_> {
    fn validate(&self) -> Result<()> {
        if !PLATFORMS.contains(&self.platform)
            || !["cli-agent", "desktop-installer", "desktop-resources"].contains(&self.kind)
            || self.source.len() != 40
            || !self
                .source
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            || self.signing != "owner-self-signed"
        {
            return Err("Invalid distribution identity or signing declaration.".into());
        }
        let parts: Vec<_> = self.version.split('.').collect();
        if parts.len() != 3
            || parts.iter().any(|s| {
                s.is_empty()
                    || s.len() > 10
                    || !s.bytes().all(|c| c.is_ascii_digit())
                    || (s.len() > 1 && s.starts_with('0'))
            })
        {
            return Err("A stable three-component version is required.".into());
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    version: String,
    platform: String,
    kind: String,
    source_commit: String,
    signing: String,
    runtime: String,
    files: Vec<Record>,
}
impl Manifest {
    fn identity(&self) -> Identity<'_> {
        Identity {
            platform: &self.platform,
            kind: &self.kind,
            source: &self.source_commit,
            version: &self.version,
            signing: &self.signing,
        }
    }
    fn validate(&self, id: &Identity<'_>) -> Result<()> {
        id.validate()?;
        if self.schema_version != 1
            || self.version != id.version
            || self.platform != id.platform
            || self.kind != id.kind
            || self.source_commit != id.source
            || self.signing != id.signing
            || self.runtime != "ONNX Runtime 1.31.0 CPU"
        {
            return Err("Distribution manifest identity differs from the verified source.".into());
        }
        Ok(())
    }
}

pub fn write_manifest(directory: &Path, id: &Identity<'_>) -> Result<()> {
    id.validate()?;
    let files = inventory(directory)?;
    if files.contains_key(MANIFEST) {
        return Err("Manifest exists; a clean staging directory is required.".into());
    }
    let manifest = Manifest {
        schema_version: 1,
        version: id.version.into(),
        platform: id.platform.into(),
        kind: id.kind.into(),
        source_commit: id.source.into(),
        signing: id.signing.into(),
        runtime: "ONNX Runtime 1.31.0 CPU".into(),
        files: files.into_values().collect(),
    };
    let bytes = serde_json::to_vec_pretty(&manifest)?;
    if bytes.len() as u64 > MAX_JSON {
        return Err("Distribution manifest exceeds inspection limits.".into());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(MANIFEST))?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn read_manifest(directory: &Path) -> Result<Manifest> {
    let path = directory.join(MANIFEST);
    if regular(&path, false)?.len() > MAX_JSON {
        return Err("Distribution manifest exceeds inspection limits.".into());
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_JSON + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_JSON {
        return Err("Distribution manifest exceeds inspection limits.".into());
    }
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn inspect_directory(directory: &Path) -> Result<()> {
    inventory(directory)?;
    Ok(())
}

/// Verify three downloaded platform folders; return six archives, six signatures and combined checksums.
/// Detached signatures/native trust are verified separately by the signing owner.
pub fn verify_release(
    artifacts: &Path,
    source: &str,
    version: &str,
) -> Result<Vec<std::path::PathBuf>> {
    Identity {
        platform: PLATFORMS[0],
        kind: "cli-agent",
        source,
        version,
        signing: "owner-self-signed",
    }
    .validate()?;
    regular(artifacts, true)?;
    let mut assets = Vec::new();
    let mut summary = String::new();
    for platform in PLATFORMS {
        let folder = artifacts.join(format!("numera-cha-{version}-{platform}"));
        regular(&folder, true)?;
        let expected = [
            format!("numera-cha-{version}-{platform}.zip"),
            format!("numera-cha-desktop-{version}-{platform}.zip"),
        ];
        let names = std::fs::read_dir(&folder)?
            .map(|e| Ok(e?.file_name()))
            .collect::<std::io::Result<Vec<_>>>()?;
        let signatures = expected.each_ref().map(|name| format!("{name}.sig"));
        if names.len() != 5
            || !names.iter().all(|n| {
                n == "SHA256SUMS.txt"
                    || expected
                        .iter()
                        .chain(&signatures)
                        .any(|s| n == std::ffi::OsStr::new(s))
            })
        {
            return Err(
                "Platform release inventory is incomplete or contains unexpected files.".into(),
            );
        }
        for signature in &signatures {
            let size = regular(&folder.join(signature), false)?.len();
            if size == 0 || size > 64 * 1024 {
                return Err("Archive signature is missing or exceeds inspection limits.".into());
            }
        }
        let checksums = folder.join("SHA256SUMS.txt");
        if regular(&checksums, false)?.len() > 4096 {
            return Err("Release checksum file exceeds inspection limits.".into());
        }
        let lines = std::fs::read_to_string(checksums)?;
        if lines.lines().count() != 2 {
            return Err("Expected exactly two platform checksums.".into());
        }
        for (name, kind) in expected.iter().zip(["cli-agent", "desktop-installer"]) {
            let id = Identity {
                platform,
                kind,
                source,
                version,
                signing: "owner-self-signed",
            };
            id.validate()?;
            let path = folder.join(name);
            if regular(&path, false)?.len() > paths::MAX_TOTAL {
                return Err("Release archive exceeds inspection limits.".into());
            }
            let (_, digest) = paths::hash(File::open(&path)?, paths::MAX_TOTAL)?;
            let line = format!("{digest}  {name}");
            if lines.lines().filter(|s| *s == line).count() != 1 {
                return Err("Release archive checksum differs.".into());
            }
            verify_package(File::open(&path)?, &id)?;
            summary.push_str(&line);
            summary.push('\n');
            assets.push(path);
            assets.push(folder.join(format!("{name}.sig")));
        }
    }
    let destination = artifacts.join("SHA256SUMS.txt");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)?;
    file.write_all(summary.as_bytes())?;
    file.sync_all()?;
    assets.push(destination);
    Ok(assets)
}
