//! rustc --edition=2024 desktop/ci/owned_command.rs -o .ci-work/owned-command
//! Pre-fetch GitHub-hosted POSIX command owner; deliberately std-only.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "owner/contract.rs"]
mod contract;
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "owner/log.rs"]
mod log;
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "owner/metadata.rs"]
mod metadata;
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "owner/platform.rs"]
mod platform;
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "owner/policy.rs"]
mod policy;
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "owner/runner.rs"]
mod runner;

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() {
    let result = (|| -> Result<i32, String> {
        if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        {
            return Err("This process owner is restricted to GitHub-hosted POSIX CI.".into());
        }
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        if args.first().is_some_and(|value| value == "--launch") {
            return runner::launch(&args[1..]);
        }
        if args.first().is_some_and(|value| value == "--fixture") {
            return contract::fixture(&args[1..]);
        }
        if args.first().is_some_and(|value| value == "--contract") {
            if args.len() != 3 || args[1] != "--work" {
                return Err("Expected --contract --work NEW_DIRECTORY.".into());
            }
            contract::verify(std::path::Path::new(&args[2]))?;
            return Ok(0);
        }
        runner::run(runner::Options::parse(&args)?)
    })();
    let code = match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{}", log::redact(&error));
            1
        }
    };
    std::process::exit(code);
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {
    eprintln!("The hosted process owner supports Linux and macOS only.");
    std::process::exit(1);
}
