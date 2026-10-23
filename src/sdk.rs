use crate::{
    config::Settings,
    core::{
        arithmetic,
        contracts::{Error, Input, Result},
    },
    engines::ocr::OCREngine,
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{Semaphore, oneshot, watch},
    task::JoinHandle,
    time::{Instant, timeout_at},
};
type Decoder =
    Arc<dyn Fn(&[u8], &Settings) -> std::result::Result<image::RgbImage, Error> + Send + Sync>;
struct DecodeJob {
    handle: JoinHandle<()>,
    done: watch::Receiver<bool>,
}

#[derive(Clone)]
pub struct Numera {
    pub settings: Settings,
    engine: OCREngine,
    admission: Arc<Semaphore>,
    decoder: Decoder,
    decodes: Arc<Mutex<Vec<DecodeJob>>>,
}
impl Numera {
    pub fn new(settings: Settings) -> Self {
        Self::with_engine(settings.clone(), OCREngine::new(settings))
    }
    pub fn with_engine(settings: Settings, engine: OCREngine) -> Self {
        Self::with_decoder(settings, engine, Arc::new(crate::assets::image))
    }
    /// Supplies a bounded local decoding adapter; ordinary constructors use the genuine image decoder.
    pub fn with_decoder(settings: Settings, engine: OCREngine, decoder: Decoder) -> Self {
        Self {
            engine,
            admission: Arc::new(Semaphore::new(settings.queue_size)),
            settings,
            decoder,
            decodes: Arc::new(Mutex::new(Vec::new())),
        }
    }
    pub fn decode_idle(&self) -> bool {
        self.decodes
            .lock()
            .is_ok_and(|jobs| jobs.iter().all(|job| job.handle.is_finished()))
    }
    pub async fn recognize_value(&self, value: Value, deadline: Option<Instant>) -> Result {
        match Input::from_value(value) {
            Ok(input) => self.recognize(input, deadline).await,
            Err(error) => Result::failure(error, "auto"),
        }
    }
    pub async fn recognize(&self, input: Input, deadline: Option<Instant>) -> Result {
        if self.admission.is_closed() {
            return Result::failure(
                Error::new("cancelled", "Recognition SDK is closed."),
                "arithmetic",
            );
        }
        let permit = match self.admission.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                return Result::failure(
                    Error::new("inference_busy", "Recognition admission is full."),
                    "arithmetic",
                );
            }
        };
        let subtype = if input.subtype != "auto" {
            input.subtype.as_str()
        } else if input.kind == "image" {
            "generic_text_line"
        } else {
            "arithmetic"
        };
        if let Err(error) = input.validate() {
            return Result::failure(error, subtype);
        }
        if deadline.is_some_and(|d| d <= Instant::now()) {
            return Result::failure(Error::new("expired", "Task deadline exhausted."), subtype);
        }
        if matches!(input.kind.as_str(), "text" | "image")
            && !matches!(input.subtype.as_str(), "auto")
            && input.subtype
                != if input.kind == "text" {
                    "arithmetic"
                } else {
                    "generic_text_line"
                }
        {
            let mut result = Result::failure(
                Error::new(
                    "unsupported_subtype",
                    "No evaluated local engine for this subtype.",
                ),
                &input.subtype,
            );
            result.outcome = "unsupported".into();
            return result;
        }
        if input.kind == "text" {
            return match arithmetic::evaluate(&input.text) {
                Ok(answer) => Result {
                    answer: Some(answer),
                    device: Some("cpu".into()),
                    evidence: json!({"grammar":"integer-binary-v1"}),
                    ..Result::new("recognized")
                },
                Err(error) => Result::failure(error, subtype),
            };
        }
        if input.kind != "image" {
            let mut result = Result::failure(
                Error::new(
                    "unsupported_modality",
                    "This input requires an available declared perception engine.",
                ),
                "arithmetic",
            );
            result.outcome = "unsupported".into();
            return result;
        }
        let deadline = deadline.unwrap_or_else(|| {
            Instant::now() + Duration::from_secs_f64(self.settings.deadline_seconds)
        });
        let settings = self.settings.clone();
        let asset = input.asset.clone();
        let decoder = self.decoder.clone();
        let (send, receive) = oneshot::channel();
        let (done, finished) = watch::channel(false);
        {
            let Ok(mut jobs) = self.decodes.lock() else {
                return Result::failure(
                    Error::new("internal_error", "Decoder ownership is unavailable."),
                    subtype,
                );
            };
            if self.admission.is_closed() {
                return Result::failure(
                    Error::new("cancelled", "Recognition SDK is closed."),
                    subtype,
                );
            }
            jobs.retain(|job| !job.handle.is_finished());
            let decode = tokio::task::spawn_blocking(move || {
                // The admitted capacity belongs to the actual decoder, not the cancellable caller.
                let result = decoder(&asset, &settings);
                (permit, result)
            });
            let handle = tokio::spawn(async move {
                let result = decode.await;
                let _ = send.send(result);
                let _ = done.send(true);
            });
            jobs.push(DecodeJob {
                handle,
                done: finished,
            });
        }
        let _permit = match timeout_at(deadline, receive).await {
            Ok(Ok(Ok((permit, Ok(_))))) => permit,
            Ok(Ok(Ok((_, Err(error))))) => return Result::failure(error, subtype),
            Ok(_) => {
                return Result::failure(
                    Error::new("invalid_asset", "Image could not be safely decoded."),
                    subtype,
                );
            }
            Err(_) => {
                return Result::failure(Error::new("expired", "Task deadline exhausted."), subtype);
            }
        };
        match self.engine.recognize(&input.asset, deadline).await {
            Err(error) => Result::failure(error, subtype),
            Ok(response) => {
                let text = response["text"].as_str().unwrap_or("");
                let Some(score) = response["confidence"].as_f64().filter(|s| s.is_finite()) else {
                    return Result::failure(
                        Error::new(
                            "model_contract",
                            "OCR output contains invalid probability values.",
                        ),
                        subtype,
                    );
                };
                let abstained = text.is_empty()
                    || score < self.settings.confidence_threshold
                    || (!input.allowed_characters.is_empty()
                        && text.chars().any(|c| !input.allowed_characters.contains(c)));
                Result {
                    answer: if abstained { None } else { Some(text.into()) },
                    confidence: Some(score),
                    abstained,
                    mode: "native_local_ai".into(),
                    subtype: "generic_text_line".into(),
                    model_revision: Some("b8f84f0b80c529de40b4fbb3544b84fa7233a513".into()),
                    device: response["device"].as_str().map(str::to_owned),
                    evidence: response["evidence"].clone(),
                    ..Result::new(if abstained {
                        "needs_user_action"
                    } else {
                        "recognized"
                    })
                }
            }
        }
    }
    pub async fn solve(&self, input: Input, deadline: Option<Instant>) -> Result {
        self.recognize(input, deadline).await
    }
    pub fn detect(&self, input: &Input) -> std::result::Result<Value, Error> {
        input.validate()?;
        Ok(crate::detection::detect(input))
    }
    pub async fn capabilities(&self) -> std::result::Result<Value, Error> {
        let settings = self.settings.clone();
        tokio::task::spawn_blocking(move || crate::capabilities::capabilities(&settings))
            .await
            .map_err(|_| Error::new("internal_error", "Capability inventory failed."))?
    }
    pub async fn models(&self) -> std::result::Result<Vec<Value>, Error> {
        let registry = crate::models::ModelRegistry::new(self.settings.clone());
        tokio::task::spawn_blocking(move || registry.list())
            .await
            .map_err(|_| Error::new("internal_error", "Model inventory failed."))?
    }
    pub async fn doctor(&self) -> std::result::Result<Value, Error> {
        let settings = self.settings.clone();
        tokio::task::spawn_blocking(move || crate::diagnostics::doctor(&settings))
            .await
            .map_err(|_| Error::new("internal_error", "Runtime diagnosis failed."))?
    }
    pub async fn close(&self) -> bool {
        self.admission.close();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let signal = {
                let Ok(mut jobs) = self.decodes.lock() else {
                    break;
                };
                jobs.retain(|job| !job.handle.is_finished());
                jobs.first().map(|job| job.done.clone())
            };
            let Some(mut signal) = signal else { break };
            let drain = async {
                if !*signal.borrow_and_update() {
                    let _ = signal.changed().await;
                }
            };
            if timeout_at(deadline, drain).await.is_err() {
                crate::logging::event(
                    "WARNING",
                    "decoder",
                    "Owned image decoder still draining after shutdown budget.",
                );
                break;
            }
            tokio::task::yield_now().await;
        }
        let engine_closed = self.engine.close().await;
        engine_closed && self.decode_idle()
    }
}
