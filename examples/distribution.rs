//! Native hosted packaging/inspection adapter. Run under the hosted process owner.
#[path = "distribution/mod.rs"]
mod distribution;

use clap::{Parser, Subcommand};
use distribution::{Identity, Result};
use numera_cha_solver::logging;
use std::{fs::File, path::PathBuf};

#[derive(Parser)]
#[command(
    about = "Create and inspect exact-source native release archives (does not sign artifacts)"
)]
struct Args {
    #[command(subcommand)]
    command: Action,
}
#[derive(Subcommand)]
enum Action {
    Manifest {
        #[arg(long)]
        directory: PathBuf,
        #[arg(long)]
        platform: String,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        source: String,
        #[arg(long)]
        version: String,
        #[arg(long)]
        signing: String,
    },
    Archive {
        #[arg(long)]
        directory: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    Verify {
        #[arg(long)]
        artifacts: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long)]
        version: String,
    },
    Source {
        #[arg(long)]
        archive: PathBuf,
        #[arg(long)]
        source: String,
    },
    Inspect {
        #[arg(long)]
        directory: PathBuf,
    },
}
fn run(args: Args) -> Result<()> {
    match args.command {
        Action::Manifest {
            directory,
            platform,
            kind,
            source,
            version,
            signing,
        } => {
            distribution::write_manifest(
                &directory,
                &Identity {
                    platform: &platform,
                    kind: &kind,
                    source: &source,
                    version: &version,
                    signing: &signing,
                },
            )?;
        }
        Action::Archive { directory, output } => distribution::create_archive(&directory, &output)?,
        Action::Verify {
            artifacts,
            source,
            version,
        } => {
            for path in distribution::verify_release(&artifacts, &source, &version)? {
                println!("{}", path.display());
            }
        }
        Action::Source { archive, source } => {
            distribution::verify_source(File::open(archive)?, &source)?
        }
        Action::Inspect { directory } => distribution::inspect_directory(&directory)?,
    }
    Ok(())
}
fn main() -> std::process::ExitCode {
    let root = std::env::var_os("GITHUB_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    let _log = logging::initialize(&root.join("logs"), "distribution", false);
    logging::event(
        "INFO",
        "distribution",
        "Hosted distribution operation started.",
    );
    match run(Args::parse()) {
        Ok(()) => {
            logging::event(
                "INFO",
                "distribution",
                "Distribution verification completed exit_code=0; signing trust is verified separately.",
            );
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            logging::event(
                "ERROR",
                "distribution",
                &format!("Distribution operation failed exit_code=1: {error}"),
            );
            std::process::ExitCode::FAILURE
        }
    }
}
