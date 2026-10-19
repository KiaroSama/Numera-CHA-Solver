use super::{ctc, preprocess};
use crate::{config::Settings, core::contracts::Error, models::ModelRegistry};
use ort::{
    ep,
    session::Session,
    value::{Tensor, TensorElementType, ValueType},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Write},
    path::PathBuf,
};

const MAX_FRAME: usize = 8 * 1024 * 1024;

fn runtime_error() -> Error {
    Error::new(
        "runtime_unavailable",
        "Inference runtime initialization failed; check runtime DLLs and model contract.",
    )
}

pub fn write_frame(stream: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::other("Worker frame exceeds its limit."));
    }
    stream.write_all(&(bytes.len() as u32).to_le_bytes())?;
    stream.write_all(bytes)?;
    stream.flush()
}
fn read_frame(stream: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut header = [0; 4];
    stream.read_exact(&mut header)?;
    let size = u32::from_le_bytes(header) as usize;
    if size > MAX_FRAME {
        return Err(io::Error::other("Worker frame exceeds its limit."));
    }
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes)?;
    Ok(bytes)
}
fn reply(value: Value) -> io::Result<()> {
    let bytes = serde_json::to_vec(&value)?;
    write_frame(&mut io::stdout().lock(), &bytes)
}

#[cfg(windows)]
fn configure_dll_directory(settings: &Settings) -> Result<Vec<*mut std::ffi::c_void>, Error> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::LibraryLoader::{
        AddDllDirectory, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_USER_DIRS,
        SetDefaultDllDirectories,
    };
    let mut handles = Vec::new();
    let mut paths = vec![
        settings
            .ort_library
            .parent()
            .ok_or_else(runtime_error)?
            .to_path_buf(),
    ];
    if !settings.dll_directory.is_empty() {
        paths.push(PathBuf::from(&settings.dll_directory));
    }
    // SAFETY: process-local loader policy, valid NUL-terminated buffers retained through each call.
    unsafe {
        if SetDefaultDllDirectories(
            LOAD_LIBRARY_SEARCH_DEFAULT_DIRS | LOAD_LIBRARY_SEARCH_USER_DIRS,
        ) == 0
        {
            return Err(runtime_error());
        }
        for path in paths {
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let handle = AddDllDirectory(wide.as_ptr());
            if handle.is_null() {
                return Err(runtime_error());
            }
            handles.push(handle);
        }
    }
    Ok(handles)
}

struct Worker {
    session: Session,
    characters: Vec<String>,
    evidence: Option<Value>,
    profile_directory: PathBuf,
}
impl Worker {
    fn new(settings: &Settings) -> Result<Self, Error> {
        let registry = ModelRegistry::new(settings.clone());
        let status = registry.verify("ppocr-v6-small")?;
        if status["availability"] != "ready" {
            return Err(Error::new(
                status["availability"].as_str().unwrap_or("model_missing"),
                "Reviewed model assets are missing or corrupt.",
            ));
        }
        if !settings.ort_library.is_file() {
            return Err(runtime_error());
        }
        let environment = ort::init_from(&settings.ort_library)
            .map_err(|_| runtime_error())?
            .with_telemetry(false)
            .build()
            .map_err(|_| runtime_error())?;
        let directory = registry.directory("ppocr-v6-small")?;
        let mut characters = vec![String::new()];
        let dictionary =
            fs::read_to_string(directory.join("characters.txt")).map_err(|_| runtime_error())?;
        characters.extend(
            dictionary
                .split_terminator(['\n', '\r'])
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
        );
        characters.push(" ".into());
        let profile_directory = settings.data_dir().join("runtime");
        fs::create_dir_all(&profile_directory).map_err(|_| runtime_error())?;
        let mut builder = Session::builder(&environment)
            .map_err(|_| runtime_error())?
            .with_auto_device(ort::session::builder::AutoDevicePolicy::PreferCPU)
            .map_err(|_| runtime_error())?
            .with_intra_threads(1)
            .map_err(|_| runtime_error())?
            .with_inter_threads(1)
            .map_err(|_| runtime_error())?
            .with_profiling(profile_directory.join(format!("ocr-{}", std::process::id())))
            .map_err(|_| runtime_error())?;
        if settings.device == "cuda" {
            builder = builder
                .with_execution_providers([ep::CUDA::default()
                    .with_device_id(0)
                    .with_memory_limit(1073741824)
                    .with_conv_algorithm_search(ep::cuda::ConvAlgorithmSearch::Heuristic)
                    .build()
                    .error_on_failure()])
                .map_err(|_| {
                    Error::new(
                        "gpu_unavailable",
                        "CUDA initialization failed; CPU mode must be selected explicitly.",
                    )
                })?;
        }
        let session = builder
            .commit_from_file(directory.join("inference.onnx"))
            .map_err(|_| runtime_error())?;
        if session.inputs().len() != 1
            || session.outputs().len() != 1
            || !matches!(session.inputs()[0].dtype(),ValueType::Tensor{ty:TensorElementType::Float32,shape,..} if shape.len()>=3 && shape[1]==3 && shape[2]==48)
        {
            return Err(Error::new(
                "model_contract",
                "ONNX input/output metadata does not match the reviewed text-line model.",
            ));
        }
        Ok(Self {
            session,
            characters,
            evidence: None,
            profile_directory,
        })
    }
    fn recognize(&mut self, content: &[u8], settings: &Settings) -> Result<Value, Error> {
        let (batch, shape) = preprocess::preprocess(content, settings)?;
        let tensor = Tensor::from_array((shape, batch)).map_err(|_| runtime_error())?;
        let outputs = self.session.run(ort::inputs![tensor]).map_err(|_| {
            Error::new(
                "inference_failed",
                "Local model inference failed; request content was not logged.",
            )
        })?;
        let (output_shape, data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|_| runtime_error())?;
        let (text, confidence) = ctc::decode(data, output_shape, &self.characters)?;
        drop(outputs);
        if self.evidence.is_none() {
            let profile = PathBuf::from(self.session.end_profiling().map_err(|_| runtime_error())?);
            if !profile.starts_with(&self.profile_directory) {
                return Err(runtime_error());
            }
            let events: Value =
                serde_json::from_slice(&fs::read(&profile).map_err(|_| runtime_error())?)
                    .map_err(|_| runtime_error())?;
            let _ = fs::remove_file(profile);
            let mut counts = BTreeMap::<String, usize>::new();
            let mut expensive_cpu = false;
            for event in events.as_array().ok_or_else(runtime_error)? {
                if event["cat"] != "Node" {
                    continue;
                }
                if let Some(provider) = event["args"]["provider"].as_str() {
                    *counts.entry(provider.into()).or_default() += 1;
                    if provider == "CPUExecutionProvider"
                        && matches!(
                            event["args"]["op_name"].as_str(),
                            Some("Conv" | "MatMul" | "Gemm" | "Attention")
                        )
                    {
                        expensive_cpu = true;
                    }
                }
            }
            if settings.device == "cuda"
                && (counts.get("CUDAExecutionProvider").copied().unwrap_or(0) == 0 || expensive_cpu)
            {
                return Err(Error::new(
                    "gpu_unavailable",
                    "GPU profiling did not prove CUDA execution of the expensive model operators.",
                ));
            }
            let active: Vec<&str> = ["CUDAExecutionProvider", "CPUExecutionProvider"]
                .into_iter()
                .filter(|provider| counts.contains_key(*provider))
                .collect();
            self.evidence = Some(
                json!({"active_providers":active,"provider_evidence":"executed_profile_nodes",
                "profile_node_counts":counts,"cpu_preprocessing":true,"preprocessing":"bgr48-pillow-bilinear-ctc-v1","character_count":self.characters.len()}),
            );
        }
        let mut evidence = self.evidence.clone().unwrap_or(json!({}));
        evidence["input_shape"] = json!(shape);
        Ok(
            json!({"text":text,"confidence":confidence,"device":settings.device,"evidence":evidence}),
        )
    }
}

pub fn run(settings: Settings) -> i32 {
    let _log = crate::logging::initialize(&settings.log_dir(), "ocr-worker", false);
    #[cfg(windows)]
    let handles = match configure_dll_directory(&settings) {
        Ok(handles) => handles,
        Err(error) => {
            let _ = reply(json!({"error":error.code,"message":error.message}));
            return 2;
        }
    };
    let result = (|| -> Result<(), Error> {
        let mut worker = Worker::new(&settings)?;
        reply(json!({"ready":true})).map_err(|_| runtime_error())?;
        loop {
            let content = match read_frame(&mut io::stdin().lock()) {
                Ok(content) => content,
                Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(_) => return Err(runtime_error()),
            };
            if content.is_empty() {
                break;
            }
            let response = match worker.recognize(&content, &settings) {
                Ok(value) => value,
                Err(error) => json!({"error":error.code,"message":error.message}),
            };
            reply(response).map_err(|_| runtime_error())?;
        }
        Ok(())
    })();
    #[cfg(windows)]
    for handle in handles {
        // SAFETY: each handle was returned by this process's AddDllDirectory.
        unsafe {
            windows_sys::Win32::System::LibraryLoader::RemoveDllDirectory(handle);
        }
    }
    match result {
        Ok(()) => 0,
        Err(error) => {
            let _ = reply(json!({"error":error.code,"message":error.message}));
            2
        }
    }
}
