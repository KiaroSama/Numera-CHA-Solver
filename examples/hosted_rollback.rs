//! Hosted installed CLI/HTTP persistence and restart validation.
use clap::Parser;
use numera_cha_solver::{core::contracts::Input, logging, store::TaskStore, tasks::request_digest};
use reqwest::{Client, Method, redirect};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{
    error::Error,
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    process::{Child, Command},
    time::{Instant, timeout},
};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
const TOKEN: &str = "rollback-owned-test-only";
#[derive(Parser)]
struct Args {
    #[arg(long)]
    executable: PathBuf,
    #[arg(long)]
    work: PathBuf,
}
struct Controller {
    work: PathBuf,
    deadline: Instant,
}
impl Controller {
    fn budget(&self, seconds: u64) -> Result<Duration> {
        let value = self
            .deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_secs(seconds));
        if value.is_zero() {
            return Err("Rollback shared deadline exhausted.".into());
        }
        Ok(value)
    }
    fn command(&self, executable: &Path) -> Command {
        let mut command = Command::new(executable);
        command
            .current_dir(&self.work)
            .env("NUMERA_ROOT", &self.work)
            .env_remove("NUMERA_API_TOKEN")
            .env_remove("NUMERA_MCP_TOKEN")
            .stdin(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        command
    }
    async fn run(&self, executable: &Path, args: &[&OsStr]) -> Result<Vec<u8>> {
        let budget = self.budget(10)?;
        let mut child = self
            .command(executable)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let out = child.stdout.take().ok_or("Missing child stdout.")?;
        let err = child.stderr.take().ok_or("Missing child stderr.")?;
        let read = |stream: Box<dyn tokio::io::AsyncRead + Unpin + Send>| async move {
            let mut data = Vec::new();
            stream.take(1024 * 1024 + 1).read_to_end(&mut data).await?;
            if data.len() > 1024 * 1024 {
                return Err(std::io::Error::other("Rollback child output cap exceeded."));
            }
            Ok::<_, std::io::Error>(data)
        };
        let result = timeout(budget, async {
            let (status, out, err) =
                tokio::try_join!(child.wait(), read(Box::new(out)), read(Box::new(err)))?;
            if !status.success() {
                logging::event("ERROR", "rollback", &String::from_utf8_lossy(&err));
                return Err("Rollback child failed.".into());
            }
            Ok::<_, Box<dyn Error + Send + Sync>>(out)
        })
        .await;
        if !matches!(&result, Ok(Ok(_))) {
            stop(&mut child).await?;
        }
        result.map_err(|_| "Rollback child deadline exhausted.")?
    }
    async fn request(
        &self,
        client: &Client,
        port: u16,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let mut request = client
            .request(
                if body.is_some() {
                    Method::POST
                } else {
                    Method::GET
                },
                format!("http://127.0.0.1:{port}{path}"),
            )
            .bearer_auth(TOKEN)
            .timeout(self.budget(2)?);
        if let Some(body) = body {
            request = request
                .header("Idempotency-Key", "native-rollback-fixture")
                .json(&body);
        }
        let mut response = request.send().await?.error_for_status()?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if bytes.len() + chunk.len() > 1024 * 1024 {
                return Err("Rollback HTTP response cap exceeded.".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(serde_json::from_slice(&bytes)?)
    }
    async fn native(&self, executable: &Path, expected: &Value) -> Result<Value> {
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))?
            .local_addr()?
            .port();
        let mut child = self
            .command(executable)
            .args(["serve", "--port", &port.to_string()])
            .env("NUMERA_API_TOKEN", TOKEN)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let result = self.exercise(&mut child, port, expected).await;
        stop(&mut child).await?;
        result
    }
    async fn exercise(&self, child: &mut Child, port: u16, expected: &Value) -> Result<Value> {
        let client = Client::builder()
            .no_proxy()
            .redirect(redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_millis(500))
            .build()?;
        timeout(self.budget(5)?, async {
            loop {
                if child.try_wait()?.is_some() {
                    return Err("Native server exited before readiness.".into());
                }
                if let Ok(value) = self.request(&client, port, "/health/live", None).await {
                    if value["live"] != true {
                        return Err("Native health response is invalid.".into());
                    }
                    return Ok::<_, Box<dyn Error + Send + Sync>>(());
                }
                // Probe actual readiness under the shared deadline; never assume a startup delay.
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .map_err(|_| "Native readiness deadline exhausted.")??;
        let mut task = self
            .request(
                &client,
                port,
                "/v1/tasks",
                Some(json!({"kind":"text","text":"12 * 3 = ?"})),
            )
            .await?;
        let id = task["id"]
            .as_str()
            .ok_or("Missing native task identity.")?
            .to_owned();
        if id.len() != 36 || !id.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-') {
            return Err("Invalid native task identity.".into());
        }
        timeout(self.budget(5)?, async {
            while task["state"] != "terminal" {
                task = self
                    .request(&client, port, &format!("/v1/tasks/{id}"), None)
                    .await?;
                tokio::task::yield_now().await;
            }
            Ok::<_, Box<dyn Error + Send + Sync>>(())
        })
        .await
        .map_err(|_| "Native durable result deadline exhausted.")??;
        if task["result"] != expected["source"] {
            return Err("Installed HTTP differs from immutable contract fixture.".into());
        }
        let repeated = self
            .request(
                &client,
                port,
                "/v1/tasks",
                Some(json!({"kind":"text","text":"12 * 3 = ?"})),
            )
            .await?;
        if repeated["id"] != id {
            return Err("Native idempotency identity changed.".into());
        }
        Ok(json!({"id":id}))
    }
}
async fn stop(child: &mut Child) -> Result<()> {
    if child.try_wait()?.is_none() {
        child.start_kill()?;
    }
    timeout(Duration::from_secs(5), child.wait())
        .await
        .map_err(|_| "Rollback child cleanup deadline exhausted.")??;
    Ok(())
}
async fn verify(args: Args) -> Result<()> {
    if std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted") {
        return Err("Rollback controller requires hosted CI.".into());
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).canonicalize()?;
    let work = args.work.canonicalize()?;
    if !work.starts_with(root.join(".ci-work"))
        || work == root.join(".ci-work")
        || work.join("data").exists()
    {
        return Err("Rollback state must be fresh and isolated inside hosted work.".into());
    }
    let executable = args.executable.canonicalize()?;
    let controller = Controller {
        work,
        deadline: Instant::now() + Duration::from_secs(80),
    };
    let _log = logging::initialize(
        &controller.work.join("verification-logs"),
        "hosted-rollback",
        false,
    );
    let fixture: Value = serde_json::from_str(include_str!("../tests/fixtures/parity/sdk.json"))?;
    let case = fixture["cases"]
        .as_array()
        .ok_or("SDK fixture cases missing.")?
        .iter()
        .find(|case| case["name"] == "sdk-00")
        .ok_or("Installed arithmetic fixture missing.")?;
    let input = Input::from_value(case["input"].clone())?;
    let digest = request_digest(&input);
    let expected = json!({"source": case["expected"], "digest":digest});
    let database_path = controller.work.join("data/tasks.sqlite3");
    let source_id = {
        let store = TaskStore::new(&database_path, 60)?;
        let task = store.create("persisted-fixture", &digest, 120.0)?;
        let id = task["id"]
            .as_str()
            .ok_or("Missing persisted identity.")?
            .to_owned();
        if !store.finish(&id, expected["source"].clone())? {
            return Err("Persisted fixture could not finish.".into());
        }
        id
    };
    let actual = controller
        .run(
            &executable,
            &[
                OsStr::new("recognize"),
                OsStr::new("--text"),
                OsStr::new("12 * 3 = ?"),
            ],
        )
        .await?;
    if serde_json::from_slice::<Value>(&actual)? != expected["source"] {
        return Err("Installed CLI differs from immutable contract fixture.".into());
    }
    let native = controller.native(&executable, &expected).await?;
    {
        let database = Connection::open(controller.work.join("data/tasks.sqlite3"))?;
        database.busy_timeout(Duration::from_secs(2))?;
        let now = numera_cha_solver::store::now();
        database.execute(
            "INSERT INTO tasks VALUES(?,?,?,?,?,?,?,NULL,NULL,?)",
            params![
                "copied-unfinished",
                "copied-unfinished-key",
                expected["digest"]
                    .as_str()
                    .ok_or("Missing source digest.")?,
                now,
                now + 120.0,
                now + 60.0,
                "recognizing",
                "[{\"state\":\"queued\"},{\"state\":\"recognizing\"}]"
            ],
        )?;
    }
    let restored = TaskStore::new(&database_path, 60)?;
    if restored.get("copied-unfinished")?["result"]["error"]["code"] != "interrupted" {
        return Err("Unfinished task was replayed instead of interrupted.".into());
    }
    for (id, key) in [
        (source_id.as_str(), "persisted-fixture"),
        (
            native["id"].as_str().ok_or("Missing native identity.")?,
            "native-rollback-fixture",
        ),
    ] {
        if restored.get(id)?["result"] != expected["source"]
            || restored.create(key, &digest, 120.0)?["id"] != id
        {
            return Err("Persisted result or idempotency changed after restart.".into());
        }
    }
    logging::event(
        "INFO",
        "rollback",
        "Installed persistence, restart and owned cleanup passed.",
    );
    println!("Installed native persistence and restart verified.");
    Ok(())
}
#[tokio::main]
async fn main() {
    if let Err(error) = verify(Args::parse()).await {
        logging::event("ERROR", "rollback", &error.to_string());
        eprintln!("Rollback validation failed: {error}");
        std::process::exit(1);
    }
}
