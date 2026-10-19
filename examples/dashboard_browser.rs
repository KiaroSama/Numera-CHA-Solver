//! Hosted real headless Chrome dashboard validation over the existing W3C transport.
#[path = "support/webdriver_client.rs"]
mod webdriver_client;
use base64::{Engine, engine::general_purpose::STANDARD};
use clap::Parser;
use numera_cha_solver::logging;
use serde_json::{Value, json};
use std::{
    error::Error,
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    process::{Child, Command},
    time::{Instant, sleep, timeout},
};
type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
const IO: Duration = Duration::from_secs(5);
#[derive(Parser)]
struct Args {
    #[arg(long)]
    executable: PathBuf,
    #[arg(long)]
    driver: PathBuf,
    #[arg(long)]
    browser: PathBuf,
    #[arg(long)]
    work: PathBuf,
}
struct Driver {
    port: u16,
    session: String,
}
impl Driver {
    async fn command(&self, path: &str, body: Value) -> Result<Value> {
        Ok(webdriver_client::request(
            self.port,
            "POST",
            &format!("/session/{}{path}", self.session),
            Some(&body),
            IO,
        )
        .await?)
    }
    async fn script(&self, script: &str) -> Result<Value> {
        self.command("/execute/sync", json!({"script":script,"args":[]}))
            .await
    }
    async fn wait(&self, expression: &str) -> Result<()> {
        let deadline = Instant::now() + IO;
        loop {
            if self.script(&format!("return !!({expression})")).await? == true {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("Dashboard DOM readiness deadline exhausted.".into());
            }
            sleep(Duration::from_millis(20)).await;
        }
    }
    async fn element(&self, selector: &str) -> Result<String> {
        let value = self
            .command("/element", json!({"using":"css selector","value":selector}))
            .await?;
        Ok(webdriver_client::identifier(
            value["element-6066-11e4-a52e-4f735466cecf"]
                .as_str()
                .ok_or("Missing dashboard element.")?,
        )?)
    }
    async fn click(&self, selector: &str) -> Result<()> {
        self.command(
            &format!("/element/{}/click", self.element(selector).await?),
            json!({}),
        )
        .await?;
        Ok(())
    }
    async fn fill(&self, selector: &str, text: &str) -> Result<()> {
        let id = self.element(selector).await?;
        self.command(&format!("/element/{id}/clear"), json!({}))
            .await?;
        self.command(&format!("/element/{id}/value"), json!({"text":text}))
            .await?;
        Ok(())
    }
    async fn cdp(&self, method: &str, params: Value) -> Result<()> {
        self.command("/goog/cdp/execute", json!({"cmd":method,"params":params}))
            .await?;
        Ok(())
    }
}
fn require(value: bool, message: &str) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err(message.to_owned().into())
    }
}
fn port() -> Result<u16> {
    Ok(TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port())
}
fn child(executable: &Path, args: &[String], work: &Path) -> Result<Child> {
    let mut command = Command::new(executable);
    command
        .args(args)
        .current_dir(work)
        .env("NUMERA_ROOT", work)
        .env("NUMERA_API_TOKEN", "browser-owned-test-only")
        .env_remove("NUMERA_MCP_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    Ok(command.spawn()?)
}
async fn stop(process: &mut Child) -> Result<()> {
    if process.try_wait()?.is_none() {
        process.start_kill()?;
    }
    timeout(Duration::from_secs(5), process.wait())
        .await
        .map_err(|_| "Dashboard child cleanup deadline exhausted.")??;
    Ok(())
}
async fn ready(process: &mut Child, port: u16, path: &str) -> Result<()> {
    let deadline = Instant::now() + IO;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .build()?;
    loop {
        require(
            process.try_wait()?.is_none(),
            "Dashboard child exited before readiness.",
        )?;
        if let Ok(response) = client
            .get(format!("http://127.0.0.1:{port}{path}"))
            .timeout(Duration::from_millis(200))
            .send()
            .await
            && response.status().is_success()
        {
            return Ok(());
        }
        require(
            Instant::now() < deadline,
            "Dashboard child readiness deadline exhausted.",
        )?;
        sleep(Duration::from_millis(20)).await;
    }
}
async fn scenario(driver: &Driver, server_port: u16, work: &Path) -> Result<()> {
    driver
        .command(
            "/timeouts",
            json!({"implicit":0,"script":4000,"pageLoad":5000}),
        )
        .await?;
    driver
        .command("/window/rect", json!({"width":1440,"height":1000}))
        .await?;
    driver
        .command(
            "/url",
            json!({"url":format!("http://127.0.0.1:{server_port}")}),
        )
        .await?;
    driver
        .wait("document.querySelector('h1')?.textContent === 'Evidence, not assumptions.'")
        .await?;
    driver.click("#connect").await?;
    driver.fill("#api-token", "browser-owned-test-only").await?;
    driver.click("#auth-form button[type=submit]").await?;
    driver
        .wait("document.querySelector('#connection-state')?.textContent === 'Connected locally'")
        .await?;
    driver
        .wait("!document.querySelector('#recognize').disabled")
        .await?;
    driver.click("#recognize").await?;
    driver
        .wait("document.querySelector('#result .answer')?.textContent === '36' && review !== null")
        .await?;
    driver
        .wait("document.querySelector('#result')?.textContent.includes('Server verified: no')")
        .await?;
    let first = driver.script("return review.id").await?;
    driver.cdp("Network.enable", json!({})).await?;
    driver
        .cdp(
            "Network.setBlockedURLs",
            json!({"urls":[format!("http://127.0.0.1:{server_port}/v1/tasks")]}),
        )
        .await?;
    driver
        .wait("!document.querySelector('#recognize').disabled")
        .await?;
    driver.click("#recognize").await?;
    driver.wait("review === null && Array.from(document.querySelectorAll('button')).find(x=>x.textContent==='I reviewed this answer')?.disabled === true").await?;
    let first = serde_json::to_string(&first)?;
    let old=driver.command("/execute/async",json!({"script":format!("const done=arguments[arguments.length-1]; api('/v1/reviews/'+{first}).then(x=>done(x.state)).catch(()=>done('failed'));"),"args":[]})).await?;
    require(
        old == "cancelled",
        "Old observation review remained actionable.",
    )?;
    driver
        .cdp("Network.setBlockedURLs", json!({"urls":[]}))
        .await?;
    driver
        .wait("!document.querySelector('#recognize').disabled")
        .await?;
    driver.click("#recognize").await?;
    driver
        .wait("document.querySelector('#review-answer')?.disabled === false")
        .await?;
    driver.fill("#review-answer", "37").await?;
    driver.click("#review-panel button.primary").await?;
    driver.wait("document.querySelector('#result')?.textContent.includes('Review confirmed. No answer was submitted to a website.')").await?;
    driver.wait("document.querySelector('#review-answer')?.value === '37' && document.querySelector('#review-answer').disabled").await?;
    driver
        .cdp(
            "Emulation.setDeviceMetricsOverride",
            json!({"width":375,"height":812,"deviceScaleFactor":1,"mobile":true}),
        )
        .await?;
    require(
        driver
            .script("return document.documentElement.scrollWidth <= window.innerWidth")
            .await?
            == true,
        "Mobile dashboard overflows horizontally.",
    )?;
    driver
        .cdp(
            "Emulation.setEmulatedMedia",
            json!({"features":[{"name":"prefers-reduced-motion","value":"reduce"}]}),
        )
        .await?;
    require(
        driver
            .script(
                "return getComputedStyle(document.querySelector('#recognize')).transitionDuration",
            )
            .await?
            == "0s",
        "Reduced motion was not respected.",
    )?;
    let screenshot = webdriver_client::request(
        driver.port,
        "GET",
        &format!("/session/{}/screenshot", driver.session),
        None,
        IO,
    )
    .await?;
    let bytes = STANDARD.decode(screenshot.as_str().ok_or("Dashboard screenshot missing.")?)?;
    require(
        bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "Dashboard screenshot is not PNG.",
    )?;
    fs::write(work.join("native-dashboard.png"), bytes)?;
    Ok(())
}
async fn verify(args: Args) -> Result<()> {
    require(
        std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted"),
        "Dashboard verification requires hosted CI.",
    )?;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).canonicalize()?;
    fs::create_dir_all(&args.work)?;
    let work = args.work.canonicalize()?;
    require(
        work.starts_with(root.join(".ci-work")) && !work.join("data").exists(),
        "Dashboard requires isolated hosted state.",
    )?;
    let _log = logging::initialize(&work.join("verification-logs"), "dashboard-browser", false);
    let server_port = port()?;
    let driver_port = port()?;
    let executable = args.executable.canonicalize()?;
    let driver = args.driver.canonicalize()?;
    let browser = args.browser.canonicalize()?;
    let mut server = child(
        &executable,
        &["serve".into(), "--port".into(), server_port.to_string()],
        &work,
    )?;
    let mut browser_driver = match child(
        &driver,
        &[
            format!("--port={driver_port}"),
            "--allowed-ips=127.0.0.1".into(),
        ],
        &work,
    ) {
        Ok(process) => process,
        Err(error) => {
            stop(&mut server).await?;
            return Err(error);
        }
    };
    let mut session = None;
    let result=timeout(Duration::from_secs(90),async {
        ready(&mut server,server_port,"/health/live").await?;
        ready(&mut browser_driver,driver_port,"/status").await?;
        let created=webdriver_client::request(driver_port,"POST","/session",Some(&json!({"capabilities":{"alwaysMatch":{"browserName":"chrome","goog:chromeOptions":{"binary":browser,"args":["--headless=new","--disable-background-networking","--no-first-run","--no-default-browser-check",format!("--user-data-dir={}",work.join("browser-profile").display())]}}}})),IO).await?;
        let id=webdriver_client::identifier(created["sessionId"].as_str().ok_or("Missing Chrome session identity.")?)?;
        session=Some(id.clone());
        scenario(&Driver{port:driver_port,session:id},server_port,&work).await
    }).await.map_err(|_| -> Box<dyn Error+Send+Sync> {"Dashboard whole-run deadline exhausted.".into()}).and_then(|x|x);
    if let Some(session) = session {
        let _ = webdriver_client::request(
            driver_port,
            "DELETE",
            &format!("/session/{session}"),
            None,
            IO,
        )
        .await;
    }
    let driver_cleanup = stop(&mut browser_driver).await;
    let server_cleanup = stop(&mut server).await;
    driver_cleanup?;
    server_cleanup?;
    result?;
    logging::event(
        "INFO",
        "browser",
        "Authentication/arithmetic/review/failure/mobile/reduced-motion and owned cleanup verified.",
    );
    println!(
        "Native dashboard authentication/arithmetic/review/failure/mobile/reduced-motion verified."
    );
    Ok(())
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = verify(Args::parse()).await {
        logging::event("ERROR", "browser", &error.to_string());
        std::process::exit(1);
    }
}
