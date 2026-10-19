use super::{
    Identity, Result,
    archives::metadata_entry,
    paths::{Record, safe_path},
};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    io::{Read, Seek},
};
use zip::ZipArchive;

#[derive(Deserialize)]
struct Dependency {
    name: String,
    version: String,
    files: Vec<String>,
    #[serde(default)]
    covered_source: Vec<String>,
}

pub(super) fn verify<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    files: &BTreeMap<String, Record>,
    id: &Identity<'_>,
) -> Result<()> {
    let require = |path: &str| -> Result<()> {
        if files.get(path).is_none_or(|record| record.size == 0) {
            return Err("Release matching source, native payload or independent notices are missing or empty.".into());
        }
        Ok(())
    };
    for name in ["licenses/dependencies.json", "source/REVISION.txt"] {
        require(name)?;
    }
    require(&format!(
        "source/numera-cha-{}-source-{}.zip",
        id.version, id.source
    ))?;
    if id.kind != "desktop-resources" {
        for name in ["LICENSE", "THIRD_PARTY_NOTICES.md"] {
            require(name)?;
        }
    }
    let dependencies: Vec<Dependency> =
        serde_json::from_slice(&metadata_entry(archive, "licenses/dependencies.json")?)?;
    if dependencies.is_empty() {
        return Err("Dependency notice inventory is empty.".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for dependency in dependencies {
        if dependency.name.is_empty()
            || dependency.version.is_empty()
            || dependency.files.is_empty()
            || !seen.insert((dependency.name.clone(), dependency.version.clone()))
        {
            return Err("Dependency notices are incomplete or duplicated.".into());
        }
        let prefix = format!("licenses/rust/{}-{}/", dependency.name, dependency.version);
        safe_path(&prefix[..prefix.len() - 1], false)?;
        for path in &dependency.files {
            safe_path(path, false)?;
            if !path.starts_with(&prefix) {
                return Err("Dependency notice escapes its package.".into());
            }
            require(path)?;
        }
        for path in &dependency.covered_source {
            safe_path(path, false)?;
            if !path.starts_with(&format!(
                "source/third-party/{}-{}/",
                dependency.name, dependency.version
            )) {
                return Err("Covered dependency source escapes its package.".into());
            }
            require(path)?;
        }
    }
    if id.kind == "cli-agent" {
        for name in [
            "plugin.json",
            ".claude-plugin/plugin.json",
            ".codex-plugin/plugin.json",
            ".cursor-plugin/plugin.json",
            "skills/numera-cha-solver/SKILL.md",
            "assets/brand/logo.png",
            "docs/QUICKSTART.fa.md",
            "docs/TESTING.md",
            "reports/VALIDATION.md",
            "CONTEXT.md",
            "tests/fixtures/ocr/ascii-smoke.png",
            "resources/signing/publisher-cert.pem",
            "resources/signing/publisher-public.pem",
            "resources/signing/publisher.json",
        ] {
            require(name)?;
        }
        let suffix = if id.platform == "windows-x64" {
            ".exe"
        } else {
            ""
        };
        for name in ["numera-cha", "numera-cha-solver-mcp"] {
            require(&format!("bin/{name}{suffix}"))?;
        }
        if id.platform == "windows-x64" {
            require("run.ps1")?;
        }
    }
    if matches!(id.kind, "cli-agent" | "desktop-resources") {
        let runtime = match id.platform {
            "windows-x64" => "onnxruntime.dll",
            "linux-x64" => "libonnxruntime.so",
            _ => "libonnxruntime.dylib",
        };
        require(&format!("data/runtime/{runtime}"))?;
        require("data/runtime/runtime-manifest.json")?;
        for name in ["LICENSE", "ThirdPartyNotices.txt"] {
            require(&format!("data/runtime/licenses/{name}"))?;
        }
    }
    if id.kind == "desktop-installer" {
        require("DESKTOP.md")?;
        let installers: Vec<_> = files
            .keys()
            .filter(|name| {
                !name.contains('/')
                    && match id.platform {
                        "windows-x64" => name.ends_with(".exe"),
                        "linux-x64" => name.ends_with(".deb"),
                        _ => name.as_str() == "Numera CHA-Solver.app.zip",
                    }
            })
            .collect();
        if installers.len() != 1 {
            return Err("Expected exactly one native desktop installer.".into());
        }
        require(installers[0])?;
        if id.platform == "macos-arm64" {
            super::archives::verify_macos_bundle(archive, installers[0])?;
        }
    }
    Ok(())
}
