use crate::{
    assets,
    config::Settings,
    core::contracts::{Error, Input},
    models::ModelRegistry,
    sdk::Numera,
};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "numera-cha",
    about = "Local perception and owned verification laboratory.",
    version
)]
pub struct Cli {
    #[arg(long,value_parser=["cuda","cpu"])]
    pub device: Option<String>,
    #[arg(long)]
    pub debug: bool,
    #[command(subcommand)]
    pub command: Command,
}
#[derive(Subcommand)]
pub enum Command {
    Doctor {
        #[arg(long)]
        smoke: Option<PathBuf>,
    },
    Capabilities,
    Models {
        #[arg(value_parser=["list","verify","fetch"])]
        operation: String,
        #[arg(default_value = "ppocr-v6-small")]
        model: String,
    },
    Detect(Observation),
    Recognize(Observation),
    Solve(Observation),
    Serve {
        #[arg(long,default_value="127.0.0.1",value_parser=["127.0.0.1","localhost"])]
        host: String,
        #[arg(long,default_value_t=8765,value_parser=clap::value_parser!(u16).range(1..))]
        port: u16,
    },
    Benchmark {
        manifest: PathBuf,
    },
    Report,
}
#[derive(clap::Args)]
#[group(skip)]
pub struct Observation {
    #[arg(long, required_unless_present = "image", conflicts_with = "image")]
    text: Option<String>,
    #[arg(long, required_unless_present = "text", conflicts_with = "text")]
    image: Option<PathBuf>,
    #[arg(long, default_value = "")]
    characters: String,
}
impl Observation {
    fn input(&self, settings: &Settings) -> Result<Input, Error> {
        let input = Input {
            kind: if self.text.is_some() { "text" } else { "image" }.into(),
            text: self.text.clone().unwrap_or_default(),
            asset: match &self.image {
                Some(path) => assets::read_asset(path, settings)?,
                None => Vec::new(),
            },
            subtype: "auto".into(),
            allowed_characters: self.characters.clone(),
            revision: String::new(),
        };
        input.validate()?;
        Ok(input)
    }
}
pub async fn execute(cli: &Cli, settings: Settings) -> Result<(Value, i32), Error> {
    let sdk = Numera::new(settings.clone());
    let result = async {
        match &cli.command {
            Command::Capabilities | Command::Report => Ok((sdk.capabilities().await?, 0)),
            Command::Models { operation, model } => {
                let registry = ModelRegistry::new(settings.clone());
                let status = match operation.as_str() {
                    "list" => return Ok((json!({"models":registry.list()?}), 0)),
                    "fetch" => Box::pin(registry.fetch(model)).await?,
                    _ => {
                        let model = model.clone();
                        tokio::task::spawn_blocking(move || registry.verify(&model))
                            .await
                            .map_err(|_| {
                                Error::new("internal_error", "Model integrity operation failed.")
                            })??
                    }
                };
                let code = if status["availability"] == "ready" {
                    0
                } else {
                    3
                };
                Ok((status, code))
            }
            Command::Doctor { smoke } => {
                let mut status = sdk.doctor().await?;
                if let Some(path) = smoke {
                    let result = sdk
                        .recognize(
                            Input {
                                kind: "image".into(),
                                asset: assets::read_asset(path, &settings)?,
                                text: String::new(),
                                subtype: "auto".into(),
                                allowed_characters: String::new(),
                                revision: String::new(),
                            },
                            None,
                        )
                        .await;
                    status["gpu_execution_verified"] = json!(
                        result.device.as_deref() == Some("cuda")
                            && result.evidence["profile_node_counts"]["CUDAExecutionProvider"]
                                .as_u64()
                                .unwrap_or(0)
                                > 0
                    );
                    status["smoke"] = serde_json::to_value(&result).map_err(|_| {
                        Error::new("internal_error", "Result serialization failed.")
                    })?;
                    let code = if result.outcome == "recognized" { 0 } else { 3 };
                    return Ok((status, code));
                }
                Ok((status, 0))
            }
            Command::Benchmark { manifest } => {
                Ok((crate::benchmark::benchmark(&sdk, manifest).await?, 0))
            }
            Command::Detect(observation) => Ok((sdk.detect(&observation.input(&settings)?)?, 0)),
            Command::Recognize(observation) | Command::Solve(observation) => {
                let result = sdk.recognize(observation.input(&settings)?, None).await;
                let code = match result.outcome.as_str() {
                    "recognized" => 0,
                    "unsupported" | "needs_user_action" => 3,
                    _ => 4,
                };
                Ok((
                    serde_json::to_value(result).map_err(|_| {
                        Error::new("internal_error", "Result serialization failed.")
                    })?,
                    code,
                ))
            }
            Command::Serve { .. } => Err(Error::new(
                "internal_error",
                "Server is handled by its startup owner.",
            )),
        }
    };
    let result = tokio::select! {
        result=result=>result,
        signal=tokio::signal::ctrl_c()=>{
            match signal {
                Ok(())=>{crate::logging::event("WARNING","cleanup","Cancellation requested.");Ok((Value::Null,130))},
                Err(_)=>Err(Error::new("internal_error","Console cancellation monitoring failed.")),
            }
        }
    };
    sdk.close().await;
    result
}
pub async fn run() -> i32 {
    let cli = Cli::parse();
    let mut settings = match Settings::from_env() {
        Ok(settings) => settings,
        Err(error) => {
            let root = std::env::var_os("NUMERA_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    std::env::current_exe()
                        .ok()
                        .and_then(|p| p.parent().map(PathBuf::from))
                        .unwrap_or_default()
                });
            let _log = crate::logging::initialize(&root.join("logs"), "numera-cha", cli.debug);
            crate::logging::event(
                "ERROR",
                "config",
                "Runtime configuration is invalid; sensitive details suppressed.",
            );
            println!("{}", json!({"outcome":"failed","error":error}));
            crate::logging::event("INFO", "cleanup", "shutdown exit_code=2");
            return 2;
        }
    };
    if let Some(device) = &cli.device {
        settings.device = device.clone();
    }
    let _log = crate::logging::initialize(&settings.log_dir(), "numera-cha", cli.debug);
    if let Err(error) = settings.validate() {
        crate::logging::event(
            "ERROR",
            "config",
            "Runtime configuration is invalid; sensitive details suppressed.",
        );
        println!("{}", json!({"outcome":"failed","error":error}));
        crate::logging::event("INFO", "cleanup", "shutdown exit_code=2");
        return 2;
    }
    if settings.device == "cpu" {
        crate::logging::event("WARNING", "gpu", "Explicit CPU inference selected.");
    }
    let result = match &cli.command {
        Command::Serve { host, port } => crate::startup::serve(settings, host, *port)
            .await
            .map(|_| (Value::Null, 0)),
        _ => execute(&cli, settings).await,
    };
    let code = match result {
        Ok((value, code)) => {
            if !value.is_null() {
                match serde_json::to_string_pretty(&value) {
                    Ok(encoded) => println!("{encoded}"),
                    Err(_) => {
                        crate::logging::event("ERROR", "operation", "Result serialization failed.");
                        println!(
                            "{}",
                            json!({"outcome":"failed","error":{"code":"internal_error","message":"Result serialization failed."}})
                        );
                        crate::logging::event("INFO", "cleanup", "shutdown exit_code=2");
                        return 2;
                    }
                }
            }
            code
        }
        Err(error) => {
            crate::logging::event("ERROR", "operation", &format!("Failed code={}", error.code));
            println!("{}", json!({"outcome":"failed","error":error}));
            2
        }
    };
    crate::logging::event("INFO", "cleanup", &format!("shutdown exit_code={code}"));
    code
}
