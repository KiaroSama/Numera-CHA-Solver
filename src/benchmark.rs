use crate::{
    assets,
    core::contracts::{Error, Input, Result as Recognition},
    sdk::Numera,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::Instant,
};
fn invalid(message: &str) -> Error {
    Error::new("invalid_benchmark", message)
}
pub fn wilson(correct: usize, count: usize) -> Option<[f64; 2]> {
    if count == 0 {
        return None;
    }
    let z = 1.959963984540054;
    let n = count as f64;
    let p = correct as f64 / n;
    let center = (p + z * z / (2.0 * n)) / (1.0 + z * z / n);
    let margin = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt() / (1.0 + z * z / n);
    Some([(center - margin).max(0.0), (center + margin).min(1.0)])
}
pub fn read_manifest(manifest: &Path) -> Result<(Value, Vec<(Value, PathBuf)>), Error> {
    use std::io::Read;
    let mut file = std::fs::File::open(manifest)
        .map_err(|_| invalid("Benchmark manifest could not be read."))?;
    let mut content = Vec::new();
    file.by_ref()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut content)
        .map_err(|_| invalid("Benchmark manifest could not be read."))?;
    if content.len() > 1024 * 1024 {
        return Err(invalid("Benchmark manifest exceeds its size limit."));
    }
    let document: Value = serde_json::from_slice(&content)
        .map_err(|_| invalid("Benchmark manifest must be valid UTF-8 JSON."))?;
    if !document.is_object() {
        return Err(invalid("Benchmark manifest must be a JSON object."));
    }
    let samples = document["samples"]
        .as_array()
        .filter(|s| !s.is_empty() && s.len() <= 1000)
        .ok_or_else(|| invalid("Use 1-1000 labeled licensed samples."))?;
    if document["license"]
        .as_str()
        .is_none_or(|s| s.trim().is_empty() || s.chars().count() > 256)
        || document.get("kind").is_some_and(|k| !k.is_string())
    {
        return Err(invalid("Use 1-1000 labeled licensed samples."));
    }
    let root = manifest
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .ok_or_else(|| invalid("Sample path is invalid."))?;
    let mut identities = HashSet::new();
    let mut paths = HashSet::new();
    let mut validated = Vec::new();
    for sample in samples {
        if !sample.is_object() {
            return Err(invalid("Each benchmark sample must be an object."));
        }
        let identity = sample["session_id"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.chars().count() <= 128)
            .ok_or_else(|| invalid("Each sample needs a unique identity, label and path."))?;
        let name = sample["path"]
            .as_str()
            .filter(|s| !s.is_empty() && s.chars().count() <= 4096)
            .ok_or_else(|| invalid("Each sample needs a unique identity, label and path."))?;
        if sample["label"]
            .as_str()
            .is_none_or(|s| s.chars().count() > 4096)
            || !identities.insert(identity.to_owned())
        {
            return Err(invalid(
                "Each sample needs a unique identity, label and path.",
            ));
        }
        if sample.get("source_id").is_some_and(|v| {
            v.as_str()
                .is_none_or(|s| s.trim().is_empty() || s.chars().count() > 128)
        }) {
            return Err(invalid("Source identity must be a bounded string."));
        }
        let relative = Path::new(name);
        if relative.is_absolute()
            || relative.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::Prefix(_)
                )
            })
        {
            return Err(invalid(
                "Sample paths must remain inside the manifest directory.",
            ));
        }
        let candidate = root.join(relative);
        let path = candidate.canonicalize().unwrap_or(candidate);
        if !path.starts_with(&root) {
            return Err(invalid(
                "Sample paths must remain inside the manifest directory.",
            ));
        }
        if !paths.insert(path.clone()) {
            return Err(invalid(
                "Duplicate canonical sample paths are not independent evidence.",
            ));
        }
        validated.push((sample.clone(), path));
    }
    Ok((document, validated))
}
pub async fn benchmark(sdk: &Numera, manifest: &Path) -> Result<Value, Error> {
    let manifest = manifest.to_owned();
    let (document, samples) = tokio::task::spawn_blocking(move || read_manifest(&manifest))
        .await
        .map_err(|_| invalid("Benchmark manifest could not be read."))??;
    let mut hashes = HashMap::new();
    let mut seen = HashSet::new();
    for (_, path) in &samples {
        let asset_path = path.clone();
        let settings = sdk.settings.clone();
        if let Ok(Ok(content)) =
            tokio::task::spawn_blocking(move || assets::read_asset(&asset_path, &settings)).await
        {
            let digest = Sha256::digest(&content)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            if !seen.insert(digest.clone()) {
                return Err(invalid(
                    "Duplicate asset content is not independent evidence.",
                ));
            }
            hashes.insert(path.clone(), digest);
        }
    }
    let mut rows = Vec::new();
    let mut independent = HashSet::new();
    for (sample, path) in samples {
        independent.insert(
            sample
                .get("source_id")
                .unwrap_or(&sample["session_id"])
                .clone()
                .to_string(),
        );
        let start = Instant::now();
        let asset_path = path.clone();
        let settings = sdk.settings.clone();
        let result =
            match tokio::task::spawn_blocking(move || assets::read_asset(&asset_path, &settings))
                .await
            {
                Ok(Ok(content))
                    if hashes.get(&path)
                        == Some(
                            &Sha256::digest(&content)
                                .iter()
                                .map(|b| format!("{b:02x}"))
                                .collect::<String>(),
                        ) =>
                {
                    sdk.recognize(
                        Input {
                            kind: "image".into(),
                            asset: content,
                            text: String::new(),
                            subtype: "auto".into(),
                            allowed_characters: String::new(),
                            revision: String::new(),
                        },
                        None,
                    )
                    .await
                }
                _ => Recognition::failure(
                    Error::new(
                        "invalid_asset",
                        "Benchmark sample could not be safely read.",
                    ),
                    "arithmetic",
                ),
            };
        rows.push(json!({"session_id":sample["session_id"],"correct":result.outcome=="recognized"&&result.answer.as_deref()==sample["label"].as_str(),"outcome":result.outcome,"abstained":result.abstained,"duration_ms":(start.elapsed().as_secs_f64()*100000.0).round()/100.0,"device":result.device,"model_revision":result.model_revision}));
    }
    let count = rows.len();
    let correct = rows.iter().filter(|r| r["correct"] == true).count();
    let abstained = rows.iter().filter(|r| r["abstained"] == true).count();
    let mut latencies: Vec<f64> = rows
        .iter()
        .map(|r| r["duration_ms"].as_f64().unwrap_or(0.0))
        .collect();
    latencies.sort_by(f64::total_cmp);
    let median = if count % 2 == 0 {
        (latencies[count / 2 - 1] + latencies[count / 2]) / 2.0
    } else {
        latencies[count / 2]
    };
    Ok(
        json!({"sample_count":count,"correct":correct,"exact_accuracy":correct as f64/count as f64,"abstention_rate":abstained as f64/count as f64,"confidence_interval_95":wilson(correct,count),"p50_ms":median,"p95_ms":latencies[((0.95*count as f64).ceil() as usize-1).min(count-1)],"license":document["license"],"dataset_kind":document.get("kind").cloned().unwrap_or(json!("unverified")),"unique_asset_count":seen.len(),"independent_unit_count":independent.len(),"split_provenance":document.get("split_provenance").cloned().unwrap_or(json!("unverified")),"limited_evidence":true,"qualification":"Exact duplicates excluded; near-duplicates and training/calibration overlap remain unverified. Promotion gates have not been evaluated.","live_acceptance_measured":false,"rows":rows}),
    )
}
