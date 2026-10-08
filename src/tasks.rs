use crate::{
    core::contracts::{Error, Input},
    sdk::Numera,
    store::{TaskStore, now},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::{Mutex, mpsc, watch},
    task::JoinHandle,
    time::Instant,
};

// Python sorted JSON spacing/ASCII escaping is part of persisted idempotency digests.
fn python_string(text: &str) -> String {
    let mut result = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '\x08' => result.push_str("\\b"),
            '\x0c' => result.push_str("\\f"),
            c if !('\x20'..'\x7f').contains(&c) => {
                let mut utf16 = [0; 2];
                for unit in c.encode_utf16(&mut utf16) {
                    result.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => result.push(c),
        }
    }
    result.push('"');
    result
}
pub fn request_digest(input: &Input) -> String {
    let asset = Sha256::digest(&input.asset)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let fields = [
        ("allowed_characters", input.allowed_characters.as_str()),
        ("asset_sha256", asset.as_str()),
        ("kind", input.kind.as_str()),
        ("revision", input.revision.as_str()),
        ("subtype", input.subtype.as_str()),
        ("text", input.text.as_str()),
    ];
    let encoded = format!(
        "{{{}}}",
        fields
            .iter()
            .map(|(k, v)| format!("{}: {}", python_string(k), python_string(v)))
            .collect::<Vec<_>>()
            .join(", ")
    );
    Sha256::digest(encoded.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
type Job = (String, Input, Instant);
struct State {
    pending: HashSet<String>,
    active: HashMap<String, watch::Sender<bool>>,
    closed: bool,
}
pub struct TaskService {
    pub sdk: Numera,
    pub store: Arc<TaskStore>,
    sender: Mutex<Option<mpsc::Sender<Job>>>,
    receiver: Mutex<Option<mpsc::Receiver<Job>>>,
    state: Mutex<State>,
    consumer: Mutex<Option<JoinHandle<()>>>,
}
impl TaskService {
    pub fn new(sdk: Numera, store: Arc<TaskStore>) -> Arc<Self> {
        let service = Self::new_paused(sdk, store);
        service.start();
        service
    }
    /// Queue admission before startup is used by embedded owners; `new` starts immediately.
    pub fn new_paused(sdk: Numera, store: Arc<TaskStore>) -> Arc<Self> {
        let (sender, receiver) = mpsc::channel(sdk.settings.queue_size);
        Arc::new(Self {
            sdk,
            store,
            sender: Mutex::new(Some(sender)),
            receiver: Mutex::new(Some(receiver)),
            state: Mutex::new(State {
                pending: HashSet::new(),
                active: HashMap::new(),
                closed: false,
            }),
            consumer: Mutex::new(None),
        })
    }
    pub fn start(self: &Arc<Self>) {
        let Ok(mut consumer) = self.consumer.try_lock() else {
            return;
        };
        if consumer.is_some() {
            return;
        }
        let Ok(mut slot) = self.receiver.try_lock() else {
            return;
        };
        let Some(mut receiver) = slot.take() else {
            return;
        };
        let weak = Arc::downgrade(self);
        *consumer = Some(tokio::spawn(async move {
            while let Some((id, input, deadline)) = receiver.recv().await {
                let Some(service) = weak.upgrade() else { break };
                let (cancel, mut cancelled) = watch::channel(false);
                {
                    let mut state = service.state.lock().await;
                    state.pending.remove(&id);
                    if state.closed {
                        if let Err(error) = service.store.cancel(&id) {
                            crate::logging::event(
                                "ERROR",
                                "task",
                                &format!(
                                    "Cancellation persistence failed id={id} code={}",
                                    error.code
                                ),
                            );
                        }
                        continue;
                    }
                    state.active.insert(id.clone(), cancel);
                }
                // One consumer owns the operation future; cancelling it cannot detach a nested task.
                tokio::select! {
                    biased;
                    _=cancelled.changed()=>{if let Err(error)=service.store.cancel(&id){crate::logging::event("ERROR","task",&format!("Cancellation persistence failed id={id} code={}",error.code));}},
                    _=service.execute(&id,input,deadline)=>{},
                }
                service.state.lock().await.active.remove(&id);
            }
        }));
    }
    pub async fn create(&self, key: &str, input: Input) -> Result<Value, Error> {
        input.validate()?;
        let mut state = self.state.lock().await;
        if state.closed {
            return Err(Error::new("cancelled", "Task service is closed."));
        }
        let task = self.store.create(
            key,
            &request_digest(&input),
            self.sdk.settings.deadline_seconds,
        )?;
        if task["state"] != "queued" {
            return Ok(task);
        }
        let id = task["id"]
            .as_str()
            .ok_or_else(|| Error::new("store_failed", "Task identity is unavailable."))?
            .to_owned();
        if state.pending.contains(&id) || state.active.contains_key(&id) {
            return Ok(task);
        }
        let deadline = Instant::now()
            + Duration::from_secs_f64((task["deadline"].as_f64().unwrap_or(0.0) - now()).max(0.0));
        let sender = self.sender.lock().await;
        if sender
            .as_ref()
            .is_some_and(|sender| sender.try_send((id.clone(), input, deadline)).is_ok())
        {
            state.pending.insert(id.clone());
        } else {
            self.store.finish(&id,json!({"outcome":"failed","error":{"code":"queue_full","message":"Task queue is full."}}))?;
        }
        self.store.get(&id)
    }
    async fn execute(&self, id: &str, input: Input, deadline: Instant) {
        let operation = async {
            if !self.store.transition(id, "validating", None)? {
                return Ok::<(), Error>(());
            }
            self.store.transition(id, "detecting", None)?;
            self.store.transition(id, "recognizing", None)?;
            let result = self.sdk.recognize(input, Some(deadline)).await;
            self.store.transition(
                id,
                "acting",
                Some("Local recognition has no browser action."),
            )?;
            self.store.transition(
                id,
                "verifying",
                Some("Recognition does not provide owner-side acceptance."),
            )?;
            self.store.finish(
                id,
                serde_json::to_value(&result)
                    .map_err(|_| Error::new("internal_error", "Result serialization failed."))?,
            )?;
            crate::logging::event(
                "INFO",
                "task",
                &format!("terminal id={id} outcome={}", result.outcome),
            );
            Ok(())
        };
        match tokio::time::timeout_at(deadline, operation).await {
            Ok(Ok(())) => {}
            Err(_) => {
                if let Err(error)=self.store.finish(id,json!({"outcome":"expired","error":{"code":"expired","message":"Task deadline exhausted."}})){crate::logging::event("ERROR","task",&format!("Expiry persistence failed id={id} code={}",error.code));}
            }
            Ok(Err(error)) => {
                crate::logging::event("ERROR","task",&format!("Task operation failed id={id} code={}",error.code));
                if let Err(error)=self.store.finish(id,json!({"outcome":"failed","error":{"code":"internal_error","message":"Task failed; no external action was repeated."}})){crate::logging::event("ERROR","task",&format!("Failure persistence failed id={id} code={}",error.code));}
            }
        }
    }
    pub async fn cancel(&self, id: &str) -> Result<Value, Error> {
        let result = self.store.cancel(id)?;
        if let Some(cancel) = self.state.lock().await.active.get(id) {
            let _ = cancel.send(true);
        }
        Ok(result)
    }
    pub async fn close(&self) -> Result<(), Error> {
        let mut failure = None;
        {
            let mut state = self.state.lock().await;
            state.closed = true;
            for id in state.pending.iter().chain(state.active.keys()) {
                if let Err(error) = self.store.cancel(id) {
                    crate::logging::event(
                        "ERROR",
                        "task",
                        &format!("Shutdown persistence failed id={id} code={}", error.code),
                    );
                    failure = Some(error);
                }
            }
            for cancel in state.active.values() {
                let _ = cancel.send(true);
            }
            self.sender.lock().await.take();
            state.pending.clear();
        }
        if let Some(mut consumer) = self.consumer.lock().await.take()
            && tokio::time::timeout(Duration::from_secs(5), &mut consumer)
                .await
                .is_err()
        {
            consumer.abort();
            let _ = consumer.await;
            crate::logging::event(
                "WARNING",
                "task",
                "Consumer exceeded its shutdown drain budget and was joined after cancellation.",
            );
        }
        self.receiver.lock().await.take();
        self.sdk.close().await;
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}
