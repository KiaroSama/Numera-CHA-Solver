use crate::{config::Settings, core::contracts::Error, models::ModelRegistry};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
    time::Instant,
};

type Verifier = Arc<dyn Fn() -> Result<Value, Error> + Send + Sync>;
struct Snapshot {
    value: Value,
    checked: Option<Instant>,
    checked_at: Option<String>,
}
struct Work {
    task: Option<JoinHandle<()>>,
    done: Option<watch::Receiver<bool>>,
    closed: bool,
}
#[derive(Clone)]
pub struct Readiness {
    state: Arc<Mutex<Snapshot>>,
    work: Arc<Mutex<Work>>,
    verify: Verifier,
}
impl Readiness {
    pub fn new(settings: Settings) -> Self {
        let registry = ModelRegistry::new(settings.clone());
        Self::with_verifier(
            settings,
            Arc::new(move || registry.verify("ppocr-v6-small")),
        )
    }
    /// Supplies a local integrity adapter; the default always checks the reviewed model manifest.
    pub fn with_verifier(_settings: Settings, verify: Verifier) -> Self {
        Self {
            verify,
            state: Arc::new(Mutex::new(Snapshot {
                value: json!({"availability":"checking"}),
                checked: None,
                checked_at: None,
            })),
            work: Arc::new(Mutex::new(Work {
                task: None,
                done: None,
                closed: false,
            })),
        }
    }
    pub async fn get(&self) -> Value {
        let mut work = self.work.lock().await;
        let state = self.state.lock().await;
        let stale = state
            .checked
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(30));
        drop(state);
        if !work.closed && work.task.as_ref().is_none_or(JoinHandle::is_finished) && stale {
            if let Some(completed) = work.task.take() {
                let _ = completed.await;
            }
            let shared = self.state.clone();
            let verify = self.verify.clone();
            let (done, receiver) = watch::channel(false);
            work.done = Some(receiver);
            work.task = Some(tokio::spawn(async move {
                let value = match tokio::task::spawn_blocking(move || verify()).await {
                    Ok(Ok(value)) => value,
                    _ => json!({"availability":"check_failed"}),
                };
                let mut state = shared.lock().await;
                state.value = value;
                state.checked = Some(Instant::now());
                state.checked_at = Some(chrono::Utc::now().to_rfc3339());
                let _ = done.send(true);
            }));
        }
        let state = self.state.lock().await;
        json!({"ready":true,"arithmetic_ready":true,"model_assets_ready":state.value["availability"]=="ready","model":state.value,
            "checked_at":state.checked_at,"refreshing":work.task.as_ref().is_some_and(|t|!t.is_finished()),"gpu_execution_verified":false})
    }
    pub async fn close(&self) {
        let mut work = self.work.lock().await;
        work.closed = true;
        let Some(mut done) = work.done.clone() else {
            return;
        };
        drop(work);
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
                "readiness",
                "Refresh still draining after shutdown budget.",
            );
            return;
        }
        let mut work = self.work.lock().await;
        if let Some(task) = work.task.take() {
            let _ = task.await;
        }
        work.done = None;
    }
}
