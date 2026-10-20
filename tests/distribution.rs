#[path = "../examples/distribution/mod.rs"]
mod distribution;

use distribution::{
    Identity, create_archive, verify_package, verify_release, verify_source, write_manifest,
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Cursor, Write},
    path::Path,
};
use tempfile::TempDir;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const VERSION: &str = "0.4.0";

fn temporary_directory() -> TempDir {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/distribution-tests");
    fs::create_dir_all(&root).unwrap();
    tempfile::Builder::new()
        .prefix("owned distribution ")
        .tempdir_in(root)
        .unwrap()
}

fn identity<'a>(platform: &'a str, kind: &'a str) -> Identity<'a> {
    Identity {
        platform,
        kind,
        source: SHA,
        version: VERSION,
        signing: "owner-self-signed",
    }
}
fn put(root: &Path, name: &str, bytes: &[u8]) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
fn zip(entries: &[(&str, &[u8])], comment: &str) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.set_comment(comment).unwrap();
    for (path, data) in entries {
        let mode = if path.contains(".app/Contents/MacOS/") {
            0o755
        } else {
            0o644
        };
        writer
            .start_file(
                *path,
                SimpleFileOptions::default()
                    .system(zip::System::Unix)
                    .unix_permissions(mode),
            )
            .unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
fn source() -> Vec<u8> {
    let mut names = vec![
        "desktop/src/main.rs",
        "Cargo.toml",
        "Cargo.lock",
        "run.ps1",
        "plugin.json",
        ".claude-plugin/plugin.json",
        ".claude-plugin/marketplace.json",
        ".codex-plugin/plugin.json",
        ".cursor-plugin/plugin.json",
        "assets/brand/logo.png",
        "LICENSE",
        "THIRD_PARTY_NOTICES.md",
        "README.md",
    ];
    names.extend([
        "src/lib.rs",
        "src/main.rs",
        "docs/MCP.md",
        "skills/numera-cha-solver/SKILL.md",
        "tests/client_payload.rs",
        "resources/models.json",
        "scripts/setup.ps1",
        "examples/distribution.rs",
        "resources/signing/publisher-cert.pem",
        "resources/signing/publisher-public.pem",
        "resources/signing/publisher.json",
    ]);
    zip(
        &names
            .into_iter()
            .map(|n| (n, b"source".as_slice()))
            .collect::<Vec<_>>(),
        SHA,
    )
}
fn staged(platform: &str, kind: &str) -> TempDir {
    let dir = temporary_directory();
    for name in ["LICENSE", "THIRD_PARTY_NOTICES.md"] {
        put(dir.path(), name, b"independent notices");
    }
    put(dir.path(), "licenses/dependencies.json", br#"[{"name":"fixture","version":"1.0.0","files":["licenses/rust/fixture-1.0.0/LICENSE"]}]"#);
    put(
        dir.path(),
        "licenses/rust/fixture-1.0.0/LICENSE",
        b"fixture license",
    );
    put(
        dir.path(),
        "source/REVISION.txt",
        format!("{SHA}\n").as_bytes(),
    );
    put(
        dir.path(),
        &format!("source/numera-cha-{VERSION}-source-{SHA}.zip"),
        &source(),
    );
    if kind == "cli-agent" {
        for name in [
            "plugin.json",
            ".claude-plugin/plugin.json",
            ".claude-plugin/marketplace.json",
            ".codex-plugin/plugin.json",
            ".cursor-plugin/plugin.json",
        ] {
            put(dir.path(), name, b"{}");
        }
        for name in [
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
            put(dir.path(), name, b"public payload");
        }
        let suffix = if platform == "windows-x64" {
            ".exe"
        } else {
            ""
        };
        for name in ["numera-cha", "numera-cha-solver-mcp"] {
            put(dir.path(), &format!("bin/{name}{suffix}"), b"native");
        }
        let runtime = match platform {
            "windows-x64" => "onnxruntime.dll",
            "linux-x64" => "libonnxruntime.so",
            _ => "libonnxruntime.dylib",
        };
        put(dir.path(), &format!("data/runtime/{runtime}"), b"runtime");
        put(dir.path(), "data/runtime/runtime-manifest.json", b"{}");
        for name in ["LICENSE", "ThirdPartyNotices.txt"] {
            put(
                dir.path(),
                &format!("data/runtime/licenses/{name}"),
                b"runtime notice",
            );
        }
        if platform == "windows-x64" {
            put(dir.path(), "run.ps1", b"launcher");
        }
    } else {
        put(dir.path(), "DESKTOP.md", b"desktop guide");
        let name = match platform {
            "windows-x64" => "Numera-setup.exe",
            "linux-x64" => "numera.deb",
            _ => "Numera CHA-Solver.app.zip",
        };
        if platform == "macos-arm64" {
            let entries = [
                (
                    "Numera CHA-Solver.app/Contents/MacOS/numera-cha",
                    b"native".as_slice(),
                ),
                (
                    "Numera CHA-Solver.app/Contents/Resources/data/runtime/runtime-manifest.json",
                    b"{}".as_slice(),
                ),
                (
                    "Numera CHA-Solver.app/Contents/Resources/data/runtime/licenses/LICENSE",
                    b"license".as_slice(),
                ),
                (
                    "Numera CHA-Solver.app/Contents/Resources/data/runtime/licenses/ThirdPartyNotices.txt",
                    b"notice".as_slice(),
                ),
            ];
            put(dir.path(), name, &zip(&entries, ""));
        } else {
            put(dir.path(), name, b"installer");
        }
    }
    dir
}

#[test]
fn manifest_archive_round_trip_preserves_hashes_modes_and_refuses_overwrite() {
    let dir = staged("linux-x64", "cli-agent");
    let id = identity("linux-x64", "cli-agent");
    write_manifest(dir.path(), &id).unwrap();
    let output = temporary_directory();
    let path = output.path().join("package.zip");
    create_archive(dir.path(), &path).unwrap();
    verify_package(fs::File::open(&path).unwrap(), &id).unwrap();
    let mut archive = ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
    use std::io::Read;
    let mut bytes = Vec::new();
    archive
        .by_name("bin/numera-cha")
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, b"native");
    let mut manifest = Vec::new();
    archive
        .by_name("package-manifest.json")
        .unwrap()
        .read_to_end(&mut manifest)
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
    let license = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["path"] == "LICENSE")
        .unwrap();
    // Independent standard SHA-256 test vector, not recomputed through the utility.
    let fixture = temporary_directory();
    put(fixture.path(), "known.txt", b"abc");
    write_manifest(fixture.path(), &id).unwrap();
    let known: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.path().join("package-manifest.json")).unwrap())
            .unwrap();
    assert_eq!(
        known["files"][0]["sha256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(license["size"], 19);
    assert_eq!(
        archive
            .by_name("bin/numera-cha")
            .unwrap()
            .unix_mode()
            .unwrap()
            & 0o777,
        0o755
    );
    assert!(create_archive(dir.path(), &path).is_err());
    assert!(write_manifest(dir.path(), &id).is_err());
    put(dir.path(), "bin/numera-cha", b"tampered");
    assert!(create_archive(dir.path(), &output.path().join("bad.zip")).is_err());
    assert!(!output.path().join("bad.zip").exists());
    let good = fs::read(&path).unwrap();
    let lost_modes = rewrite(good, |_| {});
    assert!(
        verify_package(Cursor::new(lost_modes), &id)
            .unwrap_err()
            .to_string()
            .contains("mode")
    );
}

// Rewrite through zip-rs to preserve independent manifest bytes while mutating payloads.
fn rewrite(bytes: Vec<u8>, change: impl FnOnce(&mut Vec<(String, Vec<u8>)>)) -> Vec<u8> {
    use std::io::Read;
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let comment = String::from_utf8(zip.comment().to_vec()).unwrap();
    let mut entries = Vec::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).unwrap();
        let name = std::str::from_utf8(entry.name_raw()).unwrap().to_owned();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        entries.push((name, data));
    }
    change(&mut entries);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.set_comment(comment).unwrap();
    for (name, data) in entries {
        writer
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
fn packaged(platform: &str, kind: &str) -> Vec<u8> {
    let dir = staged(platform, kind);
    write_manifest(dir.path(), &identity(platform, kind)).unwrap();
    let output = temporary_directory();
    let path = output.path().join("package.zip");
    create_archive(dir.path(), &path).unwrap();
    fs::read(path).unwrap()
}

#[test]
fn source_is_bound_to_revision_allowlist_and_required_public_payload() {
    verify_source(Cursor::new(source()), SHA).unwrap();
    assert!(
        verify_source(
            Cursor::new(source()),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        )
        .is_err()
    );
    assert!(
        verify_source(Cursor::new(zip(&[("src/main.rs", b"x")], SHA)), SHA)
            .unwrap_err()
            .to_string()
            .contains("allowlist")
    );
    let missing_catalog = rewrite(source(), |entries| {
        entries.retain(|e| e.0 != ".claude-plugin/marketplace.json")
    });
    assert!(
        verify_source(Cursor::new(missing_catalog), SHA)
            .unwrap_err()
            .to_string()
            .contains("required public payload")
    );
    let unsafe_source = rewrite(source(), |entries| {
        entries.push(("nested/secrets.md".into(), b"private".to_vec()))
    });
    let error = verify_source(Cursor::new(unsafe_source), SHA)
        .unwrap_err()
        .to_string();
    assert!(error.contains("path"), "{error}");
}

#[test]
fn package_rejects_tampering_missing_extra_duplicate_manifest_and_wrong_identity() {
    let missing_catalog = staged("windows-x64", "cli-agent");
    fs::remove_file(
        missing_catalog
            .path()
            .join(".claude-plugin/marketplace.json"),
    )
    .unwrap();
    let id = identity("windows-x64", "cli-agent");
    write_manifest(missing_catalog.path(), &id).unwrap();
    let output = temporary_directory();
    let path = output.path().join("missing-catalog.zip");
    assert!(create_archive(missing_catalog.path(), &path).is_err());
    assert!(!path.exists());
    let good = packaged("windows-x64", "cli-agent");
    for mutation in 0..7 {
        let bad = rewrite(good.clone(), |entries| match mutation {
            0 => entries
                .iter_mut()
                .find(|e| e.0 == "LICENSE")
                .unwrap()
                .1
                .push(b'!'),
            1 => entries.retain(|e| e.0 != "LICENSE"),
            2 => entries.push(("unexpected.txt".into(), b"extra".to_vec())),
            _ => {
                let manifest = &mut entries
                    .iter_mut()
                    .find(|e| e.0 == "package-manifest.json")
                    .unwrap()
                    .1;
                let mut value: serde_json::Value = serde_json::from_slice(manifest).unwrap();
                match mutation {
                    3 => {
                        let duplicate = value["files"][0].clone();
                        value["files"].as_array_mut().unwrap().push(duplicate);
                    }
                    4 => {
                        value["source_commit"] =
                            serde_json::json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                    }
                    5 => value["signing"] = serde_json::json!("unsigned"),
                    _ => value["files"][0]["size"] = serde_json::json!(0),
                }
                *manifest = serde_json::to_vec(&value).unwrap();
            }
        });
        assert!(
            verify_package(Cursor::new(bad), &identity("windows-x64", "cli-agent")).is_err(),
            "mutation {mutation}"
        );
    }
    assert!(verify_package(Cursor::new(good), &identity("linux-x64", "cli-agent")).is_err());
}

#[test]
fn archive_rejects_unsafe_entries_symlinks_and_central_directory_duplicates() {
    for path in [
        "../outside",
        "bin\\escape",
        "nested/secrets.md",
        "data/tasks.sqlite3",
        "weights.onnx",
        "C:/escape",
        "a//b",
        "./file",
        "file.",
        "CON.txt",
        "private/signing.key",
    ] {
        let error = verify_package(
            Cursor::new(zip(&[(path, b"x")], "")),
            &identity("linux-x64", "cli-agent"),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("path") || error.contains("User state"),
            "{path}: {error}"
        );
    }
    let mut link = ZipWriter::new(Cursor::new(Vec::new()));
    link.add_symlink("linked", "LICENSE", SimpleFileOptions::default())
        .unwrap();
    let error = verify_package(
        Cursor::new(link.finish().unwrap().into_inner()),
        &identity("linux-x64", "cli-agent"),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("link"), "{error}");
    // zip-rs disallows creating duplicates; patch equal-length names in both headers.
    let mut duplicate = zip(&[("one", b"a"), ("two", b"b")], "");
    for index in 0..duplicate.len() - 2 {
        if &duplicate[index..index + 3] == b"two" {
            duplicate[index..index + 3].copy_from_slice(b"one");
        }
    }
    assert!(
        verify_package(Cursor::new(duplicate), &identity("linux-x64", "cli-agent"))
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
}

#[test]
fn inspection_limits_are_enforced_before_payload_reads() {
    let mut bytes = zip(&[("large", b"a")], "");
    let central = bytes.windows(4).position(|s| s == b"PK\x01\x02").unwrap();
    let local = bytes.windows(4).position(|s| s == b"PK\x03\x04").unwrap();
    let too_large = (512_u32 * 1024 * 1024 + 1).to_le_bytes();
    bytes[central + 24..central + 28].copy_from_slice(&too_large);
    bytes[local + 22..local + 26].copy_from_slice(&too_large);
    let error = verify_package(Cursor::new(bytes), &identity("linux-x64", "cli-agent"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("inspection limits"), "{error}");
}

#[test]
fn private_files_and_hard_links_are_rejected_without_touching_existing_artifacts() {
    for name in [
        "nested/.env.production",
        "data/tasks.sqlite3",
        "weights.onnx",
        "private/key.pfx",
    ] {
        let directory = temporary_directory();
        put(directory.path(), name, b"private");
        assert!(write_manifest(directory.path(), &identity("windows-x64", "cli-agent")).is_err());
        assert!(!directory.path().join("package-manifest.json").exists());
    }
    let directory = temporary_directory();
    put(directory.path(), "original.txt", b"original");
    fs::hard_link(
        directory.path().join("original.txt"),
        directory.path().join("alias.txt"),
    )
    .unwrap();
    assert!(write_manifest(directory.path(), &identity("windows-x64", "cli-agent")).is_err());
    let directory = temporary_directory();
    put(
        directory.path(),
        "data/runtime/runtime-manifest.json",
        b"{}",
    );
    distribution::inspect_directory(directory.path()).unwrap();
    put(directory.path(), "nested/.ai/private.txt", b"private");
    assert!(distribution::inspect_directory(directory.path()).is_err());
}

#[test]
fn release_requires_exact_six_archives_signatures_checksums_and_source() {
    let artifacts = temporary_directory();
    for platform in distribution::PLATFORMS {
        let folder = artifacts
            .path()
            .join(format!("numera-cha-{VERSION}-{platform}"));
        fs::create_dir(&folder).unwrap();
        let mut checksums = String::new();
        for (prefix, kind) in [
            ("numera-cha", "cli-agent"),
            ("numera-cha-desktop", "desktop-installer"),
        ] {
            let name = format!("{prefix}-{VERSION}-{platform}.zip");
            let bytes = packaged(platform, kind);
            let digest: String = Sha256::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            put(&folder, &name, &bytes);
            put(
                &folder,
                &format!("{name}.sig"),
                b"fixture-signature-not-cryptographic-proof",
            );
            checksums.push_str(&format!("{digest}  {name}\n"));
        }
        put(&folder, "SHA256SUMS.txt", checksums.as_bytes());
    }
    let assets = verify_release(artifacts.path(), SHA, VERSION).unwrap();
    assert_eq!(assets.len(), 13);
    assert_eq!(
        fs::read_to_string(artifacts.path().join("SHA256SUMS.txt"))
            .unwrap()
            .lines()
            .count(),
        6
    );
    assert!(verify_release(artifacts.path(), SHA, VERSION).is_err());
    fs::remove_file(artifacts.path().join("SHA256SUMS.txt")).unwrap();
    let folder = artifacts
        .path()
        .join(format!("numera-cha-{VERSION}-windows-x64"));
    let checksum = fs::read(folder.join("SHA256SUMS.txt")).unwrap();
    let signature_name = format!("numera-cha-{VERSION}-windows-x64.zip.sig");
    let signature = fs::read(folder.join(&signature_name)).unwrap();
    fs::remove_file(folder.join(&signature_name)).unwrap();
    assert!(verify_release(artifacts.path(), SHA, VERSION).is_err());
    put(&folder, &signature_name, &signature);
    put(&folder, "unexpected.txt", b"unexpected");
    assert!(verify_release(artifacts.path(), SHA, VERSION).is_err());
    fs::remove_file(folder.join("unexpected.txt")).unwrap();
    assert!(
        verify_release(
            artifacts.path(),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            VERSION
        )
        .is_err()
    );
    assert_eq!(fs::read(folder.join("SHA256SUMS.txt")).unwrap(), checksum);
    put(&folder, "SHA256SUMS.txt", b"wrong\nchecksums\n");
    assert!(verify_release(artifacts.path(), SHA, VERSION).is_err());
    assert!(!artifacts.path().join("SHA256SUMS.txt").exists());
}
