use super::Result;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, Metadata},
    io::Read,
    path::{Component, Path},
};

pub const MAX_FILE: u64 = 512 * 1024 * 1024;
pub const MAX_TOTAL: u64 = 1024 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 20_000;
pub const MAX_JSON: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

pub fn safe_path(path: &str, source: bool) -> Result<()> {
    if path.is_empty()
        || path.len() > 4096
        || path.contains(['\\', ':', '\0'])
        || path.starts_with('/')
    {
        return Err("Unsafe distribution path.".into());
    }
    if path.to_ascii_lowercase().ends_with(".pem")
        && ![
            "resources/signing/publisher-cert.pem",
            "resources/signing/publisher-public.pem",
        ]
        .iter()
        .any(|public| path == *public || path.ends_with(&format!("/{public}")))
    {
        return Err("Only the pinned public publisher PEM paths may be distributed.".into());
    }
    let parts: Vec<_> = path.split('/').collect();
    for (index, part) in parts.iter().enumerate() {
        let lower = part.to_ascii_lowercase();
        let stem = lower.split('.').next().unwrap_or("");
        if part.is_empty()
            || matches!(*part, "." | "..")
            || part.ends_with(['.', ' '])
            || part
                .chars()
                .any(|c| c.is_control() || matches!(c, '<' | '>' | '"' | '|' | '?' | '*'))
            || matches!(
                stem,
                "con"
                    | "prn"
                    | "aux"
                    | "nul"
                    | "com1"
                    | "com2"
                    | "com3"
                    | "com4"
                    | "com5"
                    | "com6"
                    | "com7"
                    | "com8"
                    | "com9"
                    | "lpt1"
                    | "lpt2"
                    | "lpt3"
                    | "lpt4"
                    | "lpt5"
                    | "lpt6"
                    | "lpt7"
                    | "lpt8"
                    | "lpt9"
            )
            || matches!(
                lower.as_str(),
                ".ai"
                    | ".specify"
                    | "specs"
                    | ".claude"
                    | ".codex"
                    | ".cursor"
                    | ".kiro"
                    | ".cline"
                    | ".agents"
                    | "graphify-out"
                    | ".codebase-memory"
                    | "plans"
                    | ".git"
                    | ".ignoreme"
                    | "secrets.md"
                    | "explain-ai.md"
                    | "reference.md"
                    | "claude.md"
                    | "agents.md"
                    | "api-token.txt"
                    | "mcp-token.txt"
                    | "server.json"
            )
            || lower.starts_with(".env")
            || lower.starts_with(".ci")
            || lower.ends_with(".sqlite3")
            || lower.ends_with(".onnx")
            || lower.ends_with(".pfx")
            || lower.ends_with(".p12")
            || lower.ends_with(".key")
            || matches!(lower.as_str(), "logs" | "target" | "node_modules")
            || (lower == "data"
                && (source
                    || parts
                        .get(index + 1)
                        .is_some_and(|next| !next.eq_ignore_ascii_case("runtime"))))
        {
            return Err("Protected or nonportable distribution path.".into());
        }
    }
    if !source
        && (path.starts_with("logs/")
            || path.starts_with("target/")
            || path.starts_with("node_modules/")
            || (path.starts_with("data/")
                && path != "data/runtime"
                && !path.starts_with("data/runtime/")))
    {
        return Err("User state is not a distribution payload.".into());
    }
    Ok(())
}

pub fn regular(path: &Path, directory: bool) -> Result<Metadata> {
    let mut current = std::path::PathBuf::new();
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            return Err("Parent directory traversal is not allowed.".into());
        }
        current.push(component);
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() {
            return Err("Distribution links are not allowed.".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("Distribution reparse points are not allowed.".into());
            }
        }
    }
    let metadata = fs::symlink_metadata(path)?;
    if (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return Err("Expected an ordinary distribution file or directory.".into());
    }
    if !directory {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.nlink() != 1 {
                return Err("Distribution hard links are not allowed.".into());
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::{
                BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
            };
            let file = File::open(path)?;
            let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
            if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            if info.nNumberOfLinks != 1 {
                return Err("Distribution hard links are not allowed.".into());
            }
        }
    }
    Ok(metadata)
}

pub fn hash(mut reader: impl Read, limit: u64) -> Result<(u64, String)> {
    let mut hash = Sha256::new();
    let mut bytes = 0_u64;
    let mut next_progress = 16 * 1024 * 1024;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes = bytes
            .checked_add(n as u64)
            .ok_or("Distribution length overflow.")?;
        if bytes > limit {
            return Err("Distribution entry exceeds inspection limits.".into());
        }
        hash.update(&buffer[..n]);
        if bytes >= next_progress {
            numera_cha_solver::logging::event(
                "INFO",
                "archive-integrity",
                &format!("Hashed bytes={bytes} limit={limit}"),
            );
            next_progress = bytes.saturating_add(16 * 1024 * 1024);
        }
    }
    let digest = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok((bytes, digest))
}

pub fn inventory(root: &Path) -> Result<BTreeMap<String, Record>> {
    regular(root, true)?;
    let mut records = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    let mut aliases = std::collections::BTreeSet::new();
    let mut total = 0_u64;
    let mut count = 0;
    while let Some(directory) = stack.pop() {
        regular(&directory, true)?;
        for item in fs::read_dir(directory)? {
            let path = item?.path();
            count += 1;
            if count > MAX_ENTRIES {
                return Err("Distribution has too many entries.".into());
            }
            let name = path
                .strip_prefix(root)?
                .to_str()
                .ok_or("Distribution path is not UTF-8.")?
                .replace(std::path::MAIN_SEPARATOR, "/");
            safe_path(&name, false)?;
            if !aliases.insert(name.to_ascii_lowercase()) {
                return Err("Distribution has case-colliding entries.".into());
            }
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.is_dir() {
                regular(&path, true)?;
                stack.push(path);
                continue;
            }
            let metadata = regular(&path, false)?;
            if metadata.len() > MAX_FILE {
                return Err("Distribution file exceeds inspection limits.".into());
            }
            let (size, sha256) = hash(File::open(&path)?, MAX_FILE)?;
            if metadata.len() != size {
                return Err("Distribution changed during inspection.".into());
            }
            total = total
                .checked_add(size)
                .ok_or("Distribution length overflow.")?;
            if total > MAX_TOTAL {
                return Err("Distribution exceeds inspection limits.".into());
            }
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
    Ok(records)
}
