use crate::core::contracts::Input;
use serde_json::{Value, json};

pub fn detect(input: &Input) -> Value {
    let modality = match input.kind.as_str() {
        "text" => "arithmetic_text",
        "image" => "still_image",
        "audio" => "audio",
        _ => "browser_session",
    };
    json!({"provider":"unknown", "modality":modality, "subtype":input.subtype,
        "requires":["provider/version/subtype and licensed observation"],
        "evidence":"Input metadata only; no live page detection was performed."})
}
