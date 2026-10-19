use super::{
    Identity, MANIFEST, Manifest, Result,
    paths::{self, MAX_ENTRIES, MAX_FILE, MAX_JSON, MAX_TOTAL, Record},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    io::{Read, Seek, Write},
    path::Path,
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

fn inspect<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    source: bool,
    allow_directories: bool,
) -> Result<BTreeMap<String, Record>> {
    if zip.is_empty() || zip.len() > MAX_ENTRIES {
        return Err("Archive entry count exceeds inspection limits.".into());
    }
    // zip-rs indexes by name, so len()/by_index() alone hide exact-name duplicates.
    zip.validate_header_consistency()?;
    let mut records = BTreeMap::new();
    let mut aliases = BTreeSet::new();
    let mut total = 0_u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        let name = std::str::from_utf8(entry.name_raw())?.to_owned();
        if entry.name()?.as_bytes() != name.as_bytes() {
            return Err("Archive has ambiguous filename encoding.".into());
        }
        let directory = entry.is_dir();
        paths::safe_path(
            if directory {
                name.strip_suffix('/')
                    .ok_or("Malformed archive directory.")?
            } else {
                &name
            },
            source,
        )?;
        if !aliases.insert(name.trim_end_matches('/').to_ascii_lowercase()) {
            return Err("Archive has duplicate or case-colliding entries.".into());
        }
        let mode = entry.unix_mode().unwrap_or(0) & 0o170000;
        if entry.encrypted()
            || entry.is_symlink()
            || !matches!(mode, 0 | 0o100000 | 0o040000)
            || (directory && !allow_directories)
            || (directory && entry.size() != 0)
            || (!directory && mode == 0o040000)
            || (directory && mode == 0o100000)
        {
            return Err(
                "Archive has a link, special file, encrypted entry or unexpected directory.".into(),
            );
        }
        if entry.size() > MAX_FILE {
            return Err("Archive entry exceeds inspection limits.".into());
        }
        total = total
            .checked_add(entry.size())
            .ok_or("Archive length overflow.")?;
        if total > MAX_TOTAL {
            return Err("Archive exceeds inspection limits.".into());
        }
        let advertised = entry.size();
        let (size, sha256) = paths::hash(&mut entry, advertised)?;
        if size != advertised {
            return Err("Archive entry length differs from its header.".into());
        }
        if !directory {
            records.insert(
                name.clone(),
                Record {
                    path: name,
                    size,
                    sha256,
                },
            );
        }
    }
    // A file and a descendant cannot both be materialized, even if names differ.
    let file_aliases: BTreeSet<_> = records
        .keys()
        .map(|name| name.to_ascii_lowercase())
        .collect();
    for name in records.keys() {
        for (position, _) in name.match_indices('/') {
            if file_aliases.contains(&name[..position].to_ascii_lowercase()) {
                return Err("Archive file collides with a directory prefix.".into());
            }
        }
    }
    Ok(records)
}

fn read_entry<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str, limit: u64) -> Result<Vec<u8>> {
    let entry = zip.by_name(name)?;
    if entry.size() > limit {
        return Err("Archive metadata exceeds inspection limits.".into());
    }
    let mut bytes = Vec::new();
    entry.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err("Archive metadata exceeds inspection limits.".into());
    }
    Ok(bytes)
}

pub fn verify_source(reader: impl Read + Seek, source: &str) -> Result<()> {
    if source.len() != 40
        || !source
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err("Invalid matching source revision.".into());
    }
    let mut archive = ZipArchive::new(reader)?;
    if archive.comment() != source.as_bytes() {
        return Err("Matching source archive revision differs.".into());
    }
    let records = inspect(&mut archive, true, true)?;
    if records.len() < 20 {
        return Err("Matching source archive allowlist is incomplete.".into());
    }
    for path in records.keys() {
        let top = path.split('/').next().unwrap_or("");
        let allowed = matches!(
            top,
            "src"
                | "desktop"
                | "resources"
                | "tests"
                | "scripts"
                | "docs"
                | "skills"
                | "plugins"
                | "examples"
                | "reports"
                | ".github"
        ) || path.starts_with("assets/brand/")
            || matches!(
                path.as_str(),
                "Cargo.toml"
                    | "Cargo.lock"
                    | "rust-toolchain.toml"
                    | "LICENSE"
                    | "THIRD_PARTY_NOTICES.md"
                    | "CONTEXT.md"
                    | "CONTEXT-MAP.md"
                    | ".gitignore"
                    | ".gitattributes"
                    | "run.ps1"
                    | "plugin.json"
                    | ".claude-plugin/plugin.json"
                    | ".codex-plugin/plugin.json"
                    | ".cursor-plugin/plugin.json"
            )
            || (!path.contains('/') && path.starts_with("README") && path.ends_with(".md"));
        if !allowed
            || [
                "desktop/generated/",
                "desktop/binaries/",
                "desktop/gen/",
                "desktop/icons/",
                "desktop/permissions/autogenerated/",
            ]
            .iter()
            .any(|prefix| path.starts_with(prefix))
        {
            return Err("Matching source archive contains a non-allowlisted file.".into());
        }
    }
    for name in [
        "desktop/src/main.rs",
        "Cargo.lock",
        "run.ps1",
        "plugin.json",
        ".claude-plugin/plugin.json",
        ".codex-plugin/plugin.json",
        ".cursor-plugin/plugin.json",
        "assets/brand/logo.png",
        "resources/signing/publisher-cert.pem",
        "resources/signing/publisher-public.pem",
        "resources/signing/publisher.json",
    ] {
        if !records.contains_key(name) {
            return Err("Matching source archive omits a required public payload.".into());
        }
    }
    Ok(())
}

pub fn verify_package(reader: impl Read + Seek, id: &Identity<'_>) -> Result<()> {
    id.validate()?;
    let mut archive = ZipArchive::new(reader)?;
    let mut records = inspect(&mut archive, false, false)?;
    let manifest: Manifest =
        serde_json::from_slice(&read_entry(&mut archive, MANIFEST, MAX_JSON)?)?;
    manifest.validate(id)?;
    records
        .remove(MANIFEST)
        .ok_or("Release manifest is missing.")?;
    if manifest.files.len() != records.len() {
        return Err("Release manifest coverage differs.".into());
    }
    let mut seen = BTreeSet::new();
    for item in &manifest.files {
        paths::safe_path(&item.path, false)?;
        if !seen.insert(&item.path) || records.get(&item.path) != Some(item) {
            return Err(
                "Release manifest entry is missing, duplicated or differs in bytes.".into(),
            );
        }
    }
    if id.platform != "windows-x64" {
        for name in records
            .keys()
            .filter(|path| path.starts_with("bin/") || path.contains(".app/Contents/MacOS/"))
        {
            if archive
                .by_name(name)?
                .unix_mode()
                .is_none_or(|mode| mode & 0o777 != 0o755)
            {
                return Err("Native executable archive mode differs from 0755.".into());
            }
        }
    }
    super::payload::verify(&mut archive, &records, id)?;
    let nested = format!("source/numera-cha-{}-source-{}.zip", id.version, id.source);
    verify_source(
        std::io::Cursor::new(read_entry(&mut archive, &nested, MAX_FILE)?),
        id.source,
    )?;
    let revision = read_entry(&mut archive, "source/REVISION.txt", 128)?;
    if revision != format!("{}\n", id.source).as_bytes()
        && revision != format!("{}\r\n", id.source).as_bytes()
    {
        return Err("Matching source revision text differs.".into());
    }
    Ok(())
}

/// Write only an inspected snapshot and reopen/verify it before publishing the output path.
pub fn create_archive(directory: &Path, output: &Path) -> Result<()> {
    let expected = paths::inventory(directory)?;
    let manifest = super::read_manifest(directory)?;
    manifest.validate(&manifest.identity())?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    paths::regular(parent, true)?;
    let staging = directory.canonicalize()?;
    if parent.canonicalize()?.starts_with(&staging) {
        return Err("Archive output must be outside its staging directory.".into());
    }
    // create_new reserves the exact output without following a link or overwriting user data.
    let reservation = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let result = (|| {
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        {
            let mut writer = ZipWriter::new(temporary.as_file_mut());
            for (index, record) in expected.values().enumerate() {
                if index % 100 == 0 || record.size > 16 * 1024 * 1024 {
                    numera_cha_solver::logging::event(
                        "INFO",
                        "archive",
                        &format!(
                            "Compressing entry={} total={} bytes={}",
                            index + 1,
                            expected.len(),
                            record.size
                        ),
                    );
                }
                let path = directory.join(&record.path);
                paths::regular(&path, false)?;
                let executable = manifest.platform != "windows-x64"
                    && (record.path.starts_with("bin/")
                        || record.path.contains(".app/Contents/MacOS/"));
                let options = SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .compression_level(Some(1))
                    .system(zip::System::Unix)
                    .unix_permissions(if executable { 0o755 } else { 0o644 });
                writer.start_file(&record.path, options)?;
                let size = std::io::copy(&mut File::open(path)?.take(MAX_FILE + 1), &mut writer)?;
                if size != record.size {
                    return Err("Staging changed while writing its archive.".into());
                }
            }
            writer.finish()?;
        }
        temporary.as_file().sync_all()?;
        let mut archive = ZipArchive::new(temporary.reopen()?)?;
        if inspect(&mut archive, false, false)? != expected
            || paths::inventory(directory)? != expected
        {
            return Err("Archive bytes differ from the inspected staging allowlist.".into());
        }
        verify_package(temporary.reopen()?, &manifest.identity())?;
        // Retain the create_new reservation; copying cannot replace someone else's path.
        let mut destination = reservation;
        let copied = std::io::copy(&mut temporary.reopen()?, &mut destination)?;
        if copied != temporary.as_file().metadata()?.len() {
            return Err("Archive output copy was incomplete.".into());
        }
        destination.flush()?;
        destination.sync_all()?;
        drop(destination);
        verify_package(File::open(output)?, &manifest.identity())?;
        Ok(())
    })();
    if result.is_err() {
        // The path was reserved by this invocation, never an existing artifact.
        std::fs::remove_file(output)
            .map_err(|e| format!("Archive failed and partial output cleanup failed: {e}"))?;
    }
    result
}

pub(super) fn verify_macos_bundle<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
) -> Result<()> {
    let bytes = read_entry(zip, name, MAX_FILE)?;
    let mut bundle = ZipArchive::new(std::io::Cursor::new(bytes))?;
    let files = inspect(&mut bundle, false, true)?;
    let root = "Numera CHA-Solver.app/Contents/";
    for name in files
        .keys()
        .filter(|path| path.contains(".app/Contents/MacOS/"))
    {
        if bundle
            .by_name(name)?
            .unix_mode()
            .is_none_or(|mode| mode & 0o777 != 0o755)
        {
            return Err("macOS executable archive mode differs from 0755.".into());
        }
    }
    for required in [
        "MacOS/numera-cha",
        "Resources/data/runtime/runtime-manifest.json",
        "Resources/data/runtime/licenses/LICENSE",
        "Resources/data/runtime/licenses/ThirdPartyNotices.txt",
    ] {
        if files
            .get(&format!("{root}{required}"))
            .is_none_or(|item| item.size == 0)
        {
            return Err("macOS application is missing its worker, runtime or notices.".into());
        }
    }
    Ok(())
}

pub(super) fn metadata_entry<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    path: &str,
) -> Result<Vec<u8>> {
    read_entry(zip, path, MAX_JSON)
}
