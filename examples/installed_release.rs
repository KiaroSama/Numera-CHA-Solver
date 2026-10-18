//! Hosted-only installed-package consumer, replacing the Python validation path.
use clap::Parser;
use numera_cha_solver::logging;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    process::Command,
    time::{Instant, timeout},
};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
#[derive(Parser)]
struct Args {
    #[arg(long)]
    root: PathBuf,
    #[arg(long)]
    runtime_root: PathBuf,
    #[arg(long)]
    image: PathBuf,
    #[arg(long)]
    work: PathBuf,
}
fn regular(path: &Path) -> Result<()> {
    for parent in path.ancestors() {
        if fs::symlink_metadata(parent)?.file_type().is_symlink() {
            return Err("Input traverses a symbolic link.".into());
        }
    }
    if !path.is_file() {
        return Err("Input is not a regular file.".into());
    }
    Ok(())
}
fn json(path: &Path) -> Result<Value> {
    regular(path)?;
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn contains(root: &Path, suffix: &str) -> Result<bool> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            return Err("Package contains a linked entry.".into());
        }
        if entry.file_type()?.is_dir() {
            if entry.file_name() == suffix || contains(&entry.path(), suffix)? {
                return Ok(true);
            }
        } else if entry.path().extension().is_some_and(|x| x == suffix) {
            return Ok(true);
        }
    }
    Ok(false)
}
struct Runner {
    root: PathBuf,
    library: PathBuf,
    deadline: Instant,
}
impl Runner {
    async fn run(
        &self,
        binary: &Path,
        args: &[&str],
        expected: i32,
        seconds: u64,
    ) -> Result<String> {
        let remaining = self
            .deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_secs(seconds));
        if remaining.is_zero() {
            return Err("Installed shared deadline exhausted.".into());
        }
        let mut command = Command::new(binary);
        command
            .args(args)
            .current_dir(&self.root)
            .env("NUMERA_ROOT", &self.root)
            .env("NUMERA_DEVICE", "cpu")
            .env("NUMERA_ORT_LIBRARY", &self.library)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for key in [
            "NUMERA_WORKER_EXECUTABLE",
            "NUMERA_DLL_DIRECTORY",
            "NUMERA_API_TOKEN",
            "NUMERA_MCP_TOKEN",
            "NUMERA_FIXTURE_WORKER_EXECUTABLE",
        ] {
            command.env_remove(key);
        }
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let mut child = command.spawn()?;
        let stdout = child.stdout.take().ok_or("Missing stdout.")?;
        let stderr = child.stderr.take().ok_or("Missing stderr.")?;
        let read = |stream: Box<dyn tokio::io::AsyncRead + Unpin + Send>| async move {
            let mut data = Vec::new();
            stream
                .take(2 * 1024 * 1024 + 1)
                .read_to_end(&mut data)
                .await?;
            if data.len() > 2 * 1024 * 1024 {
                return Err(std::io::Error::other("Child output exceeded its cap."));
            }
            Ok::<_, std::io::Error>(data)
        };
        let operation = async {
            let (status, out, err) =
                tokio::try_join!(child.wait(), read(Box::new(stdout)), read(Box::new(stderr)))?;
            if status.code() != Some(expected) {
                logging::event("ERROR", "installed", &String::from_utf8_lossy(&err));
                return Err(format!(
                    "Installed command exited {:?}, expected {expected}.",
                    status.code()
                )
                .into());
            }
            Ok::<_, Box<dyn Error + Send + Sync>>(String::from_utf8(out)?)
        };
        let result = timeout(remaining, operation).await;
        if !matches!(&result, Ok(Ok(_))) {
            let _ = child.start_kill();
            timeout(Duration::from_secs(5), child.wait()).await??;
        }
        result.map_err(|_| "Installed command exceeded its deadline.")?
    }
}
async fn verify(args: &Args) -> Result<()> {
    if std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted") {
        return Err("Installed validation requires hosted CI.".into());
    }
    let root = args.root.canonicalize()?;
    if args.work.exists() || args.work.starts_with(&root) || args.work == root {
        return Err("Private model state must be fresh and separate from the package.".into());
    }
    fs::create_dir_all(&args.work)?;
    let work = args.work.canonicalize()?;
    if work.starts_with(&root) {
        return Err("Private state resolves inside the package.".into());
    }
    let log = logging::initialize(&work.join("verification-logs"), "installed-release", false);
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let binary = root.join(format!("bin/numera-cha{suffix}"));
    let mcp = root.join(format!("bin/numera-cha-solver-mcp{suffix}"));
    regular(&binary)?;
    regular(&mcp)?;
    regular(&root.join("source/REVISION.txt"))?;
    for relative in [
        "plugin.json",
        ".claude-plugin/plugin.json",
        ".codex-plugin/plugin.json",
        ".cursor-plugin/plugin.json",
    ] {
        let metadata = json(&root.join(relative))?;
        if metadata["name"] != "numera-cha-solver"
            || metadata["version"] != env!("CARGO_PKG_VERSION")
        {
            return Err("Installed plugin identity differs.".into());
        }
    }
    for relative in [
        "skills/numera-cha-solver/SKILL.md",
        "assets/brand/logo.png",
        "docs/QUICKSTART.fa.md",
        "docs/TESTING.md",
        "reports/VALIDATION.md",
        "CONTEXT.md",
        "tests/fixtures/ocr/ascii-smoke.png",
    ] {
        regular(&root.join(relative))?;
    }
    if cfg!(windows) {
        regular(&root.join("run.ps1"))?;
    }
    if contains(&root, "onnx")? || contains(&root, ".ci-oracle-env")? {
        return Err("Weights or historical environment unexpectedly shipped.".into());
    }
    let runner = Runner {
        root: work.clone(),
        library: root
            .join("data/runtime")
            .join(numera_cha_solver::config::runtime_library_name()),
        deadline: Instant::now() + Duration::from_secs(55),
    };
    let arithmetic: Value = serde_json::from_str(
        &runner
            .run(
                &binary,
                &["--device", "cpu", "recognize", "--text", "7+5"],
                0,
                10,
            )
            .await?,
    )?;
    if arithmetic["answer"] != "12" || arithmetic["server_verified"] != false {
        return Err("Installed arithmetic contract differs.".into());
    }
    runner
        .run(
            &binary,
            &["--device", "invalid", "recognize", "--text", "7+5"],
            2,
            10,
        )
        .await?;
    runner.run(&mcp, &["--help"], 0, 10).await?;
    let manifest = json(&root.join("resources/models.json"))?;
    let model = manifest["models"]
        .as_array()
        .ok_or("Missing model catalog.")?
        .iter()
        .find(|m| m["id"] == "ppocr-v6-small")
        .ok_or("Missing pinned model.")?;
    let revision = model["revision"].as_str().ok_or("Missing revision.")?;
    for asset in model["assets"].as_array().ok_or("Missing model assets.")? {
        let name = asset["file"].as_str().ok_or("Missing model file.")?;
        if Path::new(name).file_name().and_then(|x| x.to_str()) != Some(name) {
            return Err("Unsafe model filename.".into());
        }
        let relative = PathBuf::from("data/models/ppocr-v6-small")
            .join(revision)
            .join(name);
        let source = args.runtime_root.join(&relative);
        regular(&source)?;
        let bytes = fs::read(&source)?;
        let digest = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if asset["size"].as_u64() != Some(bytes.len() as u64) || asset["sha256"] != digest {
            return Err("Pinned model size/digest differs.".into());
        }
        let target = work.join(relative);
        fs::create_dir_all(target.parent().ok_or("Missing model directory.")?)?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        std::io::Write::write_all(&mut output, &bytes)?;
        drop(output);
        if fs::read(&target)? != bytes {
            return Err("Model copy differs.".into());
        }
    }
    let image = args.image.canonicalize()?;
    let result: Value = serde_json::from_str(
        &runner
            .run(
                &binary,
                &[
                    "--device",
                    "cpu",
                    "recognize",
                    "--image",
                    image.to_str().ok_or("Image path is not Unicode.")?,
                ],
                0,
                45,
            )
            .await?,
    )?;
    if result["answer"] != "NUMERA 123"
        || result["device"] != "cpu"
        || result["server_verified"] != false
        || result["model_revision"] != model["revision"]
        || result["evidence"]["input_shape"] != serde_json::json!([1, 3, 48, 320])
        || result["evidence"]["profile_node_counts"]["CPUExecutionProvider"]
            .as_u64()
            .unwrap_or(0)
            == 0
        || result["evidence"]["profile_node_counts"]["CUDAExecutionProvider"]
            .as_u64()
            .unwrap_or(0)
            != 0
    {
        return Err("Installed CPU inference/profile contract differs.".into());
    }
    let logs: Vec<_> = fs::read_dir(work.join("logs"))?.collect::<std::io::Result<_>>()?;
    if logs.is_empty() {
        return Err("No diagnostic logs.".into());
    }
    for entry in logs {
        if entry.path().extension().is_some_and(|x| x == "log")
            && !fs::read_to_string(entry.path())?.contains(" UTC] [")
        {
            return Err("Diagnostic log format differs.".into());
        }
    }
    if contains(&root, "onnx")? {
        return Err("Package mutated with weights.".into());
    }
    logging::event(
        "INFO",
        "verification",
        "Installed CLI/connector/CPU OCR/profile/source/log contracts passed.",
    );
    drop(log);
    fs::remove_dir_all(work)?;
    println!("Installed release Rust consumer passed all mapped contracts.");
    Ok(())
}
#[tokio::main]
async fn main() {
    let args = Args::parse();
    if let Err(error) = verify(&args).await {
        eprintln!("Installed validation failed: {error}");
        std::process::exit(1);
    }
}
