use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Error {
    pub code: String,
    pub message: String,
}

impl Error {
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Result {
    pub outcome: String,
    pub answer: Option<String>,
    pub confidence: Option<f64>,
    pub abstained: bool,
    pub server_verified: bool,
    pub mode: String,
    pub provider: String,
    pub subtype: String,
    pub model_revision: Option<String>,
    pub device: Option<String>,
    pub error: Option<Error>,
    pub evidence: Value,
}

impl Result {
    pub fn new(outcome: &str) -> Self {
        Self {
            outcome: outcome.into(),
            answer: None,
            confidence: None,
            abstained: false,
            server_verified: false,
            mode: "local_classical".into(),
            provider: "generic".into(),
            subtype: "arithmetic".into(),
            model_revision: None,
            device: None,
            error: None,
            evidence: json!({}),
        }
    }
    pub fn failure(error: Error, subtype: &str) -> Self {
        let outcome = match error.code.as_str() {
            "expired" => "expired",
            "cancelled" => "cancelled",
            "model_missing" | "model_corrupt" | "gpu_unavailable" | "runtime_unavailable" => {
                "unsupported"
            }
            _ => "failed",
        };
        Self {
            subtype: subtype.into(),
            error: Some(error),
            ..Self::new(outcome)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub kind: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub asset: Vec<u8>,
    #[serde(default = "auto")]
    pub subtype: String,
    #[serde(default)]
    pub allowed_characters: String,
    #[serde(default)]
    pub revision: String,
}
fn auto() -> String {
    "auto".into()
}

impl Input {
    pub fn from_value(value: Value) -> std::result::Result<Self, Error> {
        if !value.is_object() {
            return Err(Error::new(
                "invalid_input",
                "Input must be an observation object.",
            ));
        }
        let fields = [
            "kind",
            "text",
            "asset",
            "subtype",
            "allowed_characters",
            "revision",
        ];
        let object = value.as_object().expect("Object checked above.");
        if object.keys().any(|key| !fields.contains(&key.as_str())) {
            return Err(Error::new(
                "invalid_input",
                "Unknown input fields are not permitted.",
            ));
        }
        if object
            .get("kind")
            .and_then(Value::as_str)
            .is_none_or(|kind| !matches!(kind, "text" | "image" | "audio" | "browser"))
        {
            return Err(Error::new(
                "invalid_input",
                "Input kind must be text, image, audio or browser.",
            ));
        }
        for field in ["text", "subtype", "allowed_characters", "revision"] {
            if object.get(field).is_some_and(|value| !value.is_string()) {
                return Err(Error::new(
                    "invalid_input",
                    &format!("{field} must be a string."),
                ));
            }
        }
        let input: Self = serde_json::from_value(value).map_err(|_| {
            Error::new(
                "invalid_input",
                "Asset must contain local bytes, not a URL.",
            )
        })?;
        input.validate()?;
        Ok(input)
    }
    pub fn validate(&self) -> std::result::Result<(), Error> {
        if !matches!(self.kind.as_str(), "text" | "image" | "audio" | "browser") {
            return Err(Error::new("invalid_input", "Unsupported input kind."));
        }
        if self.text.chars().count() > 4096 || self.allowed_characters.chars().count() > 256 {
            return Err(Error::new(
                "invalid_input",
                "Text or character set exceeds its limit.",
            ));
        }
        if self.subtype.chars().count() > 64
            || self.revision.chars().count() > 128
            || self.asset.len() > 8 * 1024 * 1024
        {
            return Err(Error::new(
                "invalid_input",
                "Observation exceeds its size limit.",
            ));
        }
        Ok(())
    }
}
