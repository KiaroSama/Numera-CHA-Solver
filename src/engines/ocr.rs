use crate::{config::Settings, core::contracts::Error, models::ModelRegistry};
use serde_json::Value;
use std::{
    ffi::OsString,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::{Mutex, oneshot, watch},
    task::JoinHandle,
    time::{Instant, timeout_at},
};

type Verifier = Arc<dyn Fn() -> Result<Value, Error> + Send + Sync>;
fn worker_executable(settings: &Settings) -> Result<PathBuf, Error> {
    if let Some(path) = std::env::var_os("NUMERA_WORKER_EXECUTABLE") {
        let path = PathBuf::from(path);
        let path = if path.is_absolute() {
            path
        } else {
            settings.root.join(path)
        };
        return path.is_file().then_some(path).ok_or_else(|| {
            Error::new(
                "runtime_unavailable",
                "Configured native OCR worker executable is unavailable.",
            )
        });
    }
    if let Ok(path) = std::env::current_exe()
        && path
            .file_name()
            .is_some_and(|name| name == crate::config::worker_binary_name())
    {
        return Ok(path);
    }
    if let Ok(consumer) = std::env::current_exe()
        && let Some(directory) = consumer.parent()
    {
        for path in [
            directory.join(crate::config::worker_binary_name()),
            directory
                .join("..")
                .join(crate::config::worker_binary_name()),
        ] {
            if path.is_file() {
                return Ok(path);
            }
        }
    }
    for relative in ["bin", "target/release", "target/debug"] {
        let path = settings
            .root
            .join(relative)
            .join(crate::config::worker_binary_name());
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(Error::new(
        "runtime_unavailable",
        "Native OCR worker executable is unavailable; install numera-cha or configure NUMERA_WORKER_EXECUTABLE.",
    ))
}
struct Worker {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
}
impl Worker {
    async fn receive(&mut self) -> Result<Value, Error> {
        let size = self
            .output
            .read_u32_le()
            .await
            .map_err(|_| worker_error())? as usize;
        if size > 8 * 1024 * 1024 {
            return Err(worker_error());
        }
        let mut bytes = vec![0; size];
        self.output
            .read_exact(&mut bytes)
            .await
            .map_err(|_| worker_error())?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| worker_error())?;
        if let Some(code) = value["error"].as_str() {
            return Err(Error::new(
                code,
                value["message"].as_str().unwrap_or("Owned worker failed."),
            ));
        }
        Ok(value)
    }
    async fn stop(&mut self) -> Result<(), Error> {
        self.child.start_kill().map_err(|_| worker_error())?;
        tokio::time::timeout(Duration::from_secs(5), self.child.wait())
            .await
            .map_err(|_| {
                Error::new(
                    "worker_failed",
                    "Owned worker cleanup exceeded its deadline.",
                )
            })?
            .map_err(|_| worker_error())?;
        Ok(())
    }
}
fn worker_error() -> Error {
    Error::new(
        "worker_failed",
        "Owned inference worker exited unexpectedly.",
    )
}
fn cancelled() -> Error {
    Error::new("cancelled", "Inference operation was invalidated.")
}
fn expired() -> Error {
    Error::new("expired", "Inference deadline exhausted.")
}
#[derive(Clone)]
struct Launch {
    executable: PathBuf,
    arguments: Vec<OsString>,
    environment: Vec<(OsString, Option<OsString>)>,
}
struct Work {
    job: Option<JoinHandle<()>>,
    cancel: Option<watch::Sender<bool>>,
    done: Option<watch::Receiver<bool>>,
}
struct Cancellation {
    signal: watch::Sender<bool>,
    armed: bool,
}
impl Drop for Cancellation {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.signal.send(true);
        }
    }
}
#[derive(Clone)]
pub struct OCREngine {
    settings: Settings,
    worker: Arc<Mutex<Option<Worker>>>,
    work: Arc<Mutex<Work>>,
    closed: Arc<AtomicBool>,
    verify: Verifier,
    launch: Option<Launch>,
}
impl OCREngine {
    pub fn new(settings: Settings) -> Self {
        let registry = ModelRegistry::new(settings.clone());
        Self::construct(
            settings,
            None,
            Arc::new(move || registry.verify("ppocr-v6-small")),
        )
    }
    /// Adapts an owner-provided local worker and integrity verifier; `new` remains manifest-bound.
    pub fn with_runtime(
        settings: Settings,
        command: std::process::Command,
        verify: Verifier,
    ) -> Self {
        let launch = Launch {
            executable: command.get_program().into(),
            arguments: command.get_args().map(OsString::from).collect(),
            environment: command
                .get_envs()
                .map(|(k, v)| (k.into(), v.map(OsString::from)))
                .collect(),
        };
        Self::construct(settings, Some(launch), verify)
    }
    fn construct(settings: Settings, launch: Option<Launch>, verify: Verifier) -> Self {
        Self {
            settings,
            launch,
            verify,
            worker: Arc::new(Mutex::new(None)),
            work: Arc::new(Mutex::new(Work {
                job: None,
                cancel: None,
                done: None,
            })),
            closed: Arc::new(AtomicBool::new(false)),
        }
    }
    pub async fn recognize(&self, content: &[u8], deadline: Instant) -> Result<Value, Error> {
        self.settings.validate()?;
        if content.len() > self.settings.max_asset_bytes {
            return Err(Error::new("invalid_asset", "Image exceeds the byte limit."));
        }
        if self.closed.load(Ordering::Acquire) {
            return Err(cancelled());
        }
        if Instant::now() >= deadline {
            return Err(expired());
        }
        let mut work = self.work.lock().await;
        if self.closed.load(Ordering::Acquire) {
            return Err(cancelled());
        }
        if work.job.as_ref().is_some_and(|job| !job.is_finished()) {
            return Err(Error::new(
                "inference_busy",
                "Previous inference cleanup is still draining.",
            ));
        }
        if let Some(job) = work.job.take() {
            let _ = job.await;
        }
        let (cancel, receiver) = watch::channel(false);
        let (done, finished) = watch::channel(false);
        let (send, result) = oneshot::channel();
        let (accept, accepted) = oneshot::channel();
        let mut cancellation_receiver = receiver.clone();
        let engine = self.clone();
        let content = content.to_vec();
        work.cancel = Some(cancel.clone());
        work.done = Some(finished);
        work.job = Some(tokio::spawn(async move {
            let response = engine.run(&content, deadline, receiver).await;
            let cached = response.is_ok();
            let delivered = send.send(response).is_ok();
            // Cache ownership commits only when the caller actually receives the result.
            let accepted = if delivered && cached {
                tokio::select! {biased;_=cancellation_receiver.changed()=>false,result=accepted=>result.is_ok()}
            } else {
                false
            };
            if cached && !accepted {
                let mut slot = engine.worker.lock().await;
                if let Some(mut worker) = slot.take()
                    && let Err(error) = worker.stop().await
                {
                    crate::logging::event("ERROR", "worker", &error.message);
                }
            }
            let _ = done.send(true);
        }));
        drop(work);
        let mut cancellation = Cancellation {
            signal: cancel,
            armed: true,
        };
        let response = tokio::select! {
            response=result=>response.map_err(|_|worker_error())?,
            _=tokio::time::sleep_until(deadline)=>Err(expired()),
        };
        if response
            .as_ref()
            .is_err_and(|error| error.code == "expired")
        {
            let _ = cancellation.signal.send(true);
        }
        if response.is_ok() {
            let _ = accept.send(());
        } else {
            drop(accept);
        }
        cancellation.armed = false;
        response
    }
    async fn run(
        &self,
        content: &[u8],
        deadline: Instant,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<Value, Error> {
        let mut slot = self.worker.lock().await;
        let mut owned = slot.take();
        if let Some(worker) = owned.as_mut() {
            match worker.child.try_wait() {
                Ok(Some(_)) => owned = None,
                Ok(None) => {}
                Err(_) => {
                    if let Some(mut worker) = owned.take() {
                        let _ = worker.stop().await;
                    }
                    return Err(worker_error());
                }
            }
        }
        if *cancel.borrow() || self.closed.load(Ordering::Acquire) {
            if let Some(mut worker) = owned.take() {
                worker.stop().await?;
            }
            return Err(cancelled());
        }
        if owned.is_none() {
            let verify = self.verify.clone();
            let mut verification = tokio::task::spawn_blocking(move || verify());
            let status = tokio::select! {
                biased;
                _=cancel.changed()=>{let _=verification.await;return Err(cancelled());},
                result=timeout_at(deadline,&mut verification)=>match result{
                    Ok(Ok(result))=>result?,Ok(Err(_))=>return Err(worker_error()),
                    Err(_)=>{let _=verification.await;return Err(expired());},
                }
            };
            if *cancel.borrow() || self.closed.load(Ordering::Acquire) {
                return Err(cancelled());
            }
            if Instant::now() >= deadline {
                return Err(expired());
            }
            if status["availability"] != "ready" {
                return Err(Error::new(
                    status["availability"].as_str().unwrap_or("model_missing"),
                    "Reviewed model assets are missing or corrupt.",
                ));
            }
        }
        let response = async {
            if owned.is_none() {
                let mut command = if let Some(launch) = &self.launch {
                    let mut command = Command::new(&launch.executable);
                    command.args(&launch.arguments);
                    for (key, value) in &launch.environment {
                        if let Some(value) = value {
                            command.env(key, value);
                        } else {
                            command.env_remove(key);
                        }
                    }
                    command
                } else {
                    let executable = worker_executable(&self.settings)?;
                    let mut command = Command::new(executable);
                    command.arg("__ocr-worker");
                    command
                };
                command
                    .env("NUMERA_ROOT", &self.settings.root)
                    .env("NUMERA_DEVICE", &self.settings.device)
                    .env("NUMERA_ORT_LIBRARY", &self.settings.ort_library)
                    .env("NUMERA_DLL_DIRECTORY", &self.settings.dll_directory)
                    .env_remove("NUMERA_API_TOKEN")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .kill_on_drop(true);
                #[cfg(windows)]
                command.creation_flags(0x08000000);
                let mut child = command.spawn().map_err(|_| worker_error())?;
                let input = child.stdin.take().ok_or_else(worker_error)?;
                let output = child.stdout.take().ok_or_else(worker_error)?;
                owned = Some(Worker {
                    child,
                    input,
                    output,
                });
                let ready = owned.as_mut().ok_or_else(worker_error)?.receive().await?;
                if ready["ready"] != true {
                    return Err(worker_error());
                }
            }
            let worker = owned.as_mut().ok_or_else(worker_error)?;
            worker
                .input
                .write_u32_le(content.len() as u32)
                .await
                .map_err(|_| worker_error())?;
            worker
                .input
                .write_all(content)
                .await
                .map_err(|_| worker_error())?;
            worker.input.flush().await.map_err(|_| worker_error())?;
            worker.receive().await
        };
        let result = tokio::select! {biased;_=cancel.changed()=>Err(cancelled()),result=timeout_at(deadline,response)=>result.unwrap_or_else(|_|Err(expired()))};
        if result.is_ok() && !*cancel.borrow() && !self.closed.load(Ordering::Acquire) {
            *slot = owned;
        } else {
            if let Some(mut worker) = owned {
                worker.stop().await?;
            }
            if *cancel.borrow() || self.closed.load(Ordering::Acquire) {
                return Err(cancelled());
            }
        }
        result
    }
    /// The cached child's identity, for local ownership diagnostics; not inference evidence.
    pub async fn worker_pid(&self) -> Option<u32> {
        self.worker
            .lock()
            .await
            .as_ref()
            .and_then(|worker| worker.child.id())
    }
    /// True only after the retained operation has finished, including child cleanup.
    pub async fn idle(&self) -> bool {
        self.work
            .lock()
            .await
            .job
            .as_ref()
            .is_none_or(JoinHandle::is_finished)
    }
    pub async fn close(&self) {
        self.closed.store(true, Ordering::Release);
        let work = self.work.lock().await;
        if let Some(cancel) = &work.cancel {
            let _ = cancel.send(true);
        }
        let done = work.done.clone();
        drop(work);
        if let Some(mut done) = done {
            let drain = async {
                if !*done.borrow_and_update() {
                    let _ = done.changed().await;
                }
            };
            if tokio::time::timeout(Duration::from_secs(5), drain)
                .await
                .is_err()
            {
                crate::logging::event(
                    "WARNING",
                    "worker",
                    "Inference cleanup still draining after shutdown budget.",
                );
                return;
            }
        }
        let mut work = self.work.lock().await;
        if let Some(job) = work.job.take() {
            let _ = job.await;
        }
        work.done = None;
        work.cancel = None;
        drop(work);
        let mut slot = self.worker.lock().await;
        if let Some(mut worker) = slot.take()
            && let Err(error) = worker.stop().await
        {
            crate::logging::event("ERROR", "worker", &error.message);
        }
    }
}
