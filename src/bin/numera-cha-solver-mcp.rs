use clap::{Parser, Subcommand};
use numera_cha_solver::{config::Settings, logging, mcp};

#[derive(Parser)]
#[command(
    name = "numera-cha-solver-mcp",
    version,
    about = "Local licensed recognition and human handoff MCP; no protection bypass."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Serve {
        #[arg(long)]
        port: Option<u16>,
    },
    Bridge {
        /// Explicitly relay to a separately managed authenticated loopback server.
        #[arg(long)]
        remote: bool,
    },
}
#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let result = async {
        let settings = Settings::from_env()?;
        let _log = logging::initialize(&settings.log_dir(), "numera-cha-solver-mcp", false);
        let configured_port = || {
            let environment = std::env::var("NUMERA_MCP_PORT").ok();
            mcp::port(environment.as_deref())
        };
        let result = match cli.command {
            Command::Serve {
                port: override_port,
            } => {
                let chosen = override_port.map(Ok).unwrap_or_else(configured_port)?;
                if chosen == 0 {
                    return Err(numera_cha_solver::core::contracts::Error::new(
                        "invalid_config",
                        "Port0isnotpermitted.",
                    ));
                }
                mcp::transport::serve(settings, chosen).await
            }
            Command::Bridge { remote: false } => mcp::local::run(settings).await,
            Command::Bridge { remote: true } => {
                mcp::bridge::run(settings, configured_port()?).await
            }
        };
        logging::event(
            if result.is_ok() { "INFO" } else { "ERROR" },
            "cleanup",
            if result.is_ok() {
                "MCP stopped successfully."
            } else {
                "MCP stopped with a safe reported error."
            },
        );
        result
    }
    .await;
    if let Err(error) = result {
        eprintln!("{}: {}", error.code, error.message);
        std::process::exit(2);
    }
}
