//! Hosted Linux native desktop verification; no renderer mock or environment-skipped GUI test.
#[cfg(target_os = "linux")]
#[path = "support/webdriver_client.rs"]
mod webdriver_client;
#[cfg(target_os = "linux")]
#[path = "support/webdriver_owner.rs"]
mod webdriver_owner;

#[cfg(target_os = "linux")]
mod linux {
    use super::{webdriver_client, webdriver_owner::DriverOwner};
    use clap::Parser;
    use numera_cha_solver::logging;
    use serde_json::{Value, json};
    use std::{
        fs::{self, File, OpenOptions},
        io::{Read, Seek, SeekFrom},
        net::{Ipv4Addr, TcpListener},
        path::{Path, PathBuf},
        process::{Command, Stdio},
        time::Duration,
    };
    use tokio::{
        net::TcpStream,
        time::{Instant, sleep, timeout},
    };

    type Check<T> = Result<T, String>;
    const IO: Duration = Duration::from_secs(10);
    const ELEMENT: &str = "element-6066-11e4-a52e-4f735466cecf";

    #[derive(Parser)]
    struct Arguments {
        #[arg(long)]
        executable: PathBuf,
        #[arg(long)]
        driver: PathBuf,
        #[arg(long)]
        native_close: PathBuf,
        #[arg(long)]
        work: PathBuf,
    }
    struct Driver {
        port: u16,
        session: Option<String>,
    }
    impl Driver {
        async fn request(
            &self,
            method: &str,
            path: &str,
            body: Option<&Value>,
            budget: Duration,
        ) -> Check<Value> {
            webdriver_client::request(self.port, method, path, body, budget)
                .await
                .map_err(|error| error.to_string())
        }
        async fn command(&self, path: &str, body: Value, budget: Duration) -> Check<Value> {
            let session = self
                .session
                .as_deref()
                .ok_or("Native WebDriver session is unavailable.")?;
            self.request(
                "POST",
                &format!("/session/{session}{path}"),
                Some(&body),
                budget,
            )
            .await
        }
        async fn script(&self, script: &str, budget: Duration) -> Check<Value> {
            self.command("/execute/sync", json!({"script":script,"args":[]}), budget)
                .await
        }
        async fn element(&self, selector: &str) -> Check<String> {
            let value = self
                .command(
                    "/element",
                    json!({"using":"css selector","value":selector}),
                    IO,
                )
                .await?;
            element_id(&value)
        }
        async fn click(&self, selector: &str) -> Check<()> {
            self.click_id(&self.element(selector).await?).await
        }
        async fn click_id(&self, element: &str) -> Check<()> {
            self.command(&format!("/element/{element}/click"), json!({}), IO)
                .await?;
            Ok(())
        }
        async fn wait_script(
            &self,
            owner: &mut DriverOwner,
            script: &str,
            description: &str,
        ) -> Check<()> {
            let deadline = Instant::now() + IO;
            loop {
                require(
                    owner.running()?,
                    "Owned native driver exited before DOM readiness.",
                )?;
                let remaining = deadline.saturating_duration_since(Instant::now());
                require(
                    !remaining.is_zero(),
                    &format!("Native desktop readiness failed: {description}."),
                )?;
                if self.script(script, remaining).await? == Value::Bool(true) {
                    return Ok(());
                }
                sleep(Duration::from_millis(30).min(remaining)).await; // Bounded DOM state polling, not startup delay.
            }
        }
    }
    fn require(condition: bool, message: &str) -> Check<()> {
        condition.then_some(()).ok_or_else(|| message.to_owned())
    }
    fn element_id(value: &Value) -> Check<String> {
        let id = value
            .get(ELEMENT)
            .and_then(Value::as_str)
            .ok_or("Native WebDriver element identity is invalid.")?;
        webdriver_client::identifier(id).map_err(|error| error.to_string())
    }
    fn executable(path: &Path) -> Check<PathBuf> {
        let path = path
            .canonicalize()
            .map_err(|_| "Required native executable is unavailable.")?;
        require(path.is_file(), "Required native executable is not a file.")?;
        Ok(path)
    }
    fn log_paths(work: &Path) -> Check<Vec<PathBuf>> {
        let entries = match fs::read_dir(work.join("logs")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => return Err("Native application logs could not be inspected.".into()),
        };
        let mut paths = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|_| "Native application log discovery failed.")?;
            if entry
                .file_type()
                .map_err(|_| "Native log type is unavailable.")?
                .is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "log")
            {
                paths.push(entry.path());
                require(
                    paths.len() <= 128,
                    "Native application log discovery exceeded its bound.",
                )?;
            }
        }
        paths.sort();
        Ok(paths)
    }
    fn tail(path: &Path) -> Check<String> {
        let mut file =
            File::open(path).map_err(|_| "Native diagnostic log could not be opened.")?;
        let length = file
            .metadata()
            .map_err(|_| "Native diagnostic log size is unavailable.")?
            .len();
        file.seek(SeekFrom::Start(length.saturating_sub(8192)))
            .map_err(|_| "Native diagnostic log could not be positioned.")?;
        let mut bytes = Vec::new();
        file.take(8192)
            .read_to_end(&mut bytes)
            .map_err(|_| "Native diagnostic log could not be read.")?;
        let prefix = if length > 8192 {
            bytes
                .iter()
                .take_while(|byte| (**byte & 0xc0) == 0x80)
                .count()
        } else {
            0
        };
        require(
            prefix <= 3,
            "Native diagnostic tail contains invalid UTF-8; content withheld.",
        )?;
        String::from_utf8(bytes[prefix..].to_vec())
            .map_err(|_| "Native diagnostic tail was not complete UTF-8; content withheld.".into())
    }
    fn diagnostics(work: &Path) {
        let mut paths = vec![work.join("native-driver.log")];
        match log_paths(work) {
            Ok(logs) => paths.extend(logs),
            Err(error) => logging::event("WARNING", "native-gui", &error),
        }
        for path in paths {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("native-log");
            match tail(&path) {
                Ok(text) => {
                    logging::event("ERROR", "native-gui", &format!("Diagnostic {name}: {text}"))
                }
                Err(error) => logging::event("WARNING", "native-gui", &error),
            }
        }
    }
    async fn wait_ready(driver: &Driver, owner: &mut DriverOwner, native_port: u16) -> Check<()> {
        let deadline = Instant::now() + IO;
        loop {
            require(
                owner.running()?,
                "Owned native driver exited before readiness.",
            )?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            require(
                !remaining.is_zero(),
                "Native desktop readiness failed: driver ports.",
            )?;
            let slice = Duration::from_millis(200).min(remaining);
            let connected = matches!(
                timeout(
                    slice,
                    TcpStream::connect((Ipv4Addr::LOCALHOST, native_port))
                )
                .await,
                Ok(Ok(_))
            );
            if connected {
                match webdriver_client::request(driver.port, "GET", "/status", None, slice).await {
                    Ok(value) if !value.is_null() => return Ok(()),
                    Ok(_)
                    | Err(
                        webdriver_client::Failure::Transport | webdriver_client::Failure::Deadline,
                    ) => {}
                    Err(error) => return Err(error.to_string()),
                }
            }
            sleep(Duration::from_millis(30).min(remaining)).await; // Bounded port/status observation.
        }
    }
    async fn native_close(
        arguments: &Arguments,
        application: &super::webdriver_owner::Identity,
        capture: &File,
    ) -> Check<()> {
        application.verify_executable(&arguments.executable)?;
        let mut helper = tokio::process::Command::new(&arguments.native_close)
            .arg(application.pid.to_string())
            .arg(&arguments.executable)
            .arg(application.birth.to_string())
            .stdin(Stdio::null())
            .stdout(
                capture
                    .try_clone()
                    .map_err(|_| "Native close capture clone failed.")?,
            )
            .stderr(
                capture
                    .try_clone()
                    .map_err(|_| "Native close capture clone failed.")?,
            )
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| "Native close helper could not start.")?;
        match timeout(Duration::from_secs(5), helper.wait()).await {
            Ok(Ok(status)) if status.success() => Ok(()),
            Ok(_) => Err("Native window close failed.".into()),
            Err(_) => {
                let _ = helper.start_kill();
                timeout(Duration::from_secs(2), helper.wait())
                    .await
                    .map_err(|_| "Native close helper survived cleanup.")?
                    .map_err(|_| "Native close helper could not be reaped.")?;
                Err("Native close helper exceeded its deadline.".into())
            }
        }
    }
    async fn scenario(
        arguments: &Arguments,
        driver: &mut Driver,
        owner: &mut DriverOwner,
        capture: &File,
        native_port: u16,
    ) -> Check<()> {
        wait_ready(driver, owner, native_port).await?;
        logging::event(
            "INFO",
            "native-gui",
            "Native driver ports are ready; creating actual desktop session.",
        );
        let capabilities = json!({"capabilities":{"alwaysMatch":{
            "browserName":"wry","tauri:options":{"application":arguments.executable}}}});
        let session = driver
            .request("POST", "/session", Some(&capabilities), IO)
            .await?;
        driver.session = Some(
            webdriver_client::identifier(
                session
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .ok_or("Native WebDriver session identity is invalid.")?,
            )
            .map_err(|error| error.to_string())?,
        );
        require(
            driver.session.is_some(),
            "Native WebDriver session identity is invalid.",
        )?;
        driver
            .command(
                "/timeouts",
                json!({"implicit":0,"script":5000,"pageLoad":10000}),
                IO,
            )
            .await?;
        driver.wait_script(owner, "return document.querySelector('#connection-state')?.textContent === 'Connected locally'", "trusted IPC bootstrap").await?;
        require(
            driver
                .script("return document.querySelector('h1').textContent", IO)
                .await?
                == "Evidence, not assumptions.",
            "Packaged desktop heading changed.",
        )?;
        logging::event(
            "INFO",
            "native-gui",
            "Trusted native IPC is ready; recognizing arithmetic.",
        );
        driver.click("#recognize").await?;
        driver
            .wait_script(
                owner,
                "return document.querySelector('#result .answer')?.textContent === '36'",
                "arithmetic answer",
            )
            .await?;
        driver
            .wait_script(
                owner,
                "return !!document.querySelector('#review-answer')",
                "manual review",
            )
            .await?;
        let field = driver.element("#review-answer").await?;
        driver
            .command(&format!("/element/{field}/clear"), json!({}), IO)
            .await?;
        driver
            .command(
                &format!("/element/{field}/value"),
                json!({"text":"37","value":["3","7"]}),
                IO,
            )
            .await?;
        let button = driver.script("return Array.from(document.querySelectorAll('button')).find(x => x.textContent === 'I reviewed this answer')", IO).await?;
        driver.click_id(&element_id(&button)?).await?;
        driver.wait_script(owner, "return document.querySelector('#result').textContent.includes('Review confirmed. No answer was submitted to a website.')", "explicit confirmation").await?;
        require(driver.script("return document.querySelector('#result').textContent.includes('Server verified: no')", IO).await? == true,
            "Human review incorrectly implied external verification.")?;
        require(driver.script("return document.querySelector('#review-answer').value === '37' && document.querySelector('#review-answer').disabled", IO).await? == true,
            "Human correction was not retained and locked after confirmation.")?;
        logging::event(
            "INFO",
            "native-gui",
            "Actual correction and confirmation passed; requesting owned GTK window close.",
        );
        let application = owner.application(&arguments.executable)?;
        native_close(arguments, &application, capture).await?;
        let deadline = Instant::now() + IO;
        loop {
            let mut drained = false;
            for path in log_paths(&arguments.work)? {
                if tail(&path)?.contains("Desktop API and owned tasks drained.") {
                    drained = true;
                    break;
                }
            }
            if drained {
                break;
            }
            require(
                Instant::now() < deadline,
                "Native application did not report owned task drain.",
            )?;
            sleep(Duration::from_millis(30)).await; // Bounded actual application drain observation.
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while application.executing()? {
            require(
                Instant::now() < deadline,
                "Owned desktop remained executing after native close.",
            )?;
            sleep(Duration::from_millis(20)).await; // Bounded kernel exit observation.
        }
        Ok(())
    }
    pub async fn run() -> Check<()> {
        let mut arguments = Arguments::parse();
        require(
            std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted")
                && std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true"),
            "This native GUI tier requires isolated GitHub-hosted Linux/Xvfb.",
        )?;
        let display =
            std::env::var("DISPLAY").map_err(|_| "An isolated local Xvfb display is required.")?;
        require(
            display.strip_prefix(':').is_some_and(|value| {
                !value.is_empty() && value.chars().all(|c| c.is_ascii_digit() || c == '.')
            }),
            "An isolated local Xvfb display is required.",
        )?;
        arguments.executable = executable(&arguments.executable)?;
        arguments.driver = executable(&arguments.driver)?;
        arguments.native_close = executable(&arguments.native_close)?;
        if let Some(parent) = arguments.work.parent() {
            fs::create_dir_all(parent)
                .map_err(|_| "Native GUI work parent could not be created.")?;
        }
        fs::create_dir(&arguments.work)
            .map_err(|_| "Native GUI work directory must be new and writable.")?;
        let _log = logging::initialize(
            &arguments.work.join("verification-logs"),
            "native-webdriver",
            false,
        );
        let capture = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(arguments.work.join("native-driver.log"))
            .map_err(|_| "Native driver capture could not be created.")?;
        let reservation = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .map_err(|_| "Native driver port allocation failed.")?;
        let native_reservation = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .map_err(|_| "Native port allocation failed.")?;
        let port = reservation
            .local_addr()
            .map_err(|_| "Native driver port is unavailable.")?
            .port();
        let native_port = native_reservation
            .local_addr()
            .map_err(|_| "Native port is unavailable.")?
            .port();
        drop((reservation, native_reservation));
        let mut command = Command::new(&arguments.driver);
        command
            .args([
                "--port",
                &port.to_string(),
                "--native-port",
                &native_port.to_string(),
            ])
            .env("NUMERA_ROOT", &arguments.work)
            .env("GDK_BACKEND", "x11");
        for key in [
            "NUMERA_DEVICE",
            "WAYLAND_DISPLAY",
            "NUMERA_API_TOKEN",
            "NUMERA_ORT_LIBRARY",
            "NUMERA_WORKER_EXECUTABLE",
        ] {
            command.env_remove(key);
        }
        logging::event(
            "INFO",
            "native-gui",
            "Starting actual packaged Linux desktop verification.",
        );
        let mut owner = DriverOwner::spawn(&mut command, &capture).await?;
        let mut driver = Driver {
            port,
            session: None,
        };
        let result = match timeout(
            Duration::from_secs(90),
            scenario(&arguments, &mut driver, &mut owner, &capture, native_port),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err("Native desktop scenario exceeded its whole-run deadline.".into()),
        };
        if let Some(session) = driver.session.as_deref()
            && let Err(error) = driver
                .request(
                    "DELETE",
                    &format!("/session/{session}"),
                    None,
                    Duration::from_secs(2),
                )
                .await
        {
            logging::event(
                "WARNING",
                "native-gui",
                &format!("Session deletion after native close: {error}"),
            );
        }
        let cleanup = owner.stop().await;
        if result.is_err() || cleanup.is_err() {
            if let Err(error) = &result {
                logging::event("ERROR", "native-gui", error);
            }
            if let Err(error) = &cleanup {
                logging::event("ERROR", "cleanup", error);
            }
            diagnostics(&arguments.work);
        }
        match (result, cleanup) {
            (Ok(()), Ok(())) => {
                logging::event(
                    "INFO",
                    "native-gui",
                    "Packaged Linux native IPC/arithmetic/human-review/close and owned cleanup passed.",
                );
                println!("Packaged Linux native IPC/arithmetic/human-review/close passed.");
                Ok(())
            }
            (Err(error), Err(cleanup)) => Err(format!("{error} Cleanup: {cleanup}")),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        }
    }
}

#[cfg(target_os = "linux")]
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = linux::run().await {
        numera_cha_solver::logging::event("ERROR", "native-gui", &error);
        std::process::exit(1);
    }
}
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("This native GUI executable requires isolated GitHub-hosted Linux/Xvfb.");
    std::process::exit(1);
}
