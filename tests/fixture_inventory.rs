use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs, path::Path};

#[test]
fn immutable_contract_fixtures_and_native_targets_remain_complete() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures = [
        (
            "arithmetic.json",
            "d7c7c8a198f1e9a99237b18051593876b04bac6d79ccba15c1bfbd58685d171f",
        ),
        (
            "sdk.json",
            "efb1c1d6f6bb00b5423cc7ea95e76d406cbd7cc24da510f464ad8f1e78dbcac8",
        ),
        (
            "ascii-preprocess.f32",
            "2c289136141b9be09a04ab4bc720d7d8246d61ac0d8ac12bd0dd441008b1e8b3",
        ),
        (
            "preprocess.json",
            "462f6bbdc9cacb1e2f0d2d75c6b78c7dfe7d687a8dfe3e69f97fc89f10fb002f",
        ),
        (
            "http.json",
            "1fe4b3e29f052c96c177308b0b741dd00bf3aa720d13e4b20820c614e6622e70",
        ),
        (
            "request-digests.json",
            "e41bae81dd32ddd123ef7d0796b42bb861be28a19de2961a039663f0720bc5c2",
        ),
        (
            "contracts.json",
            "c19364d1569b68987f83a03f775948a48e8287b68f7124b8d0e3cb0baf2a4200",
        ),
        (
            "source-tests.json",
            "81234ca93e587380959d9bb50c09dd35f1582345781452b1d48a23549f431f32",
        ),
    ];
    for (name, expected) in fixtures {
        let bytes = fs::read(root.join("tests/fixtures/parity").join(name)).unwrap();
        // Git may check text out as CRLF on Windows; binary tensor bytes stay exact.
        let bytes = if name.ends_with(".json") {
            String::from_utf8(bytes)
                .unwrap()
                .replace("\r\n", "\n")
                .into_bytes()
        } else {
            bytes
        };
        let actual: String = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(actual, expected, "Immutable fixture changed: {name}");
    }
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/parity/source-tests.json")).unwrap();
    assert_eq!(rows.len(), 45, "Mapped behavior inventory changed.");
    let mut identities = HashSet::new();
    for row in rows {
        assert!(identities.insert((row["source"].to_string(), row["name"].to_string())));
        let status = row["status"].as_str().unwrap();
        assert!(["parity", "native", "retired"].contains(&status));
        if status == "retired" {
            assert!(!row["reason"].as_str().unwrap().is_empty());
        }
        for target in row["target"].as_str().unwrap().split(';') {
            let (path, symbol) = target
                .trim()
                .split_once("::")
                .unwrap_or((target.trim(), ""));
            assert!(!path.starts_with('/') && !path.split('/').any(|p| p == ".."));
            let source = fs::read_to_string(root.join(path)).unwrap();
            if status != "retired" && !symbol.is_empty() {
                assert!(
                    source.contains(&format!("fn {symbol}("))
                        || (path == "scripts/native_browser.py"
                            && symbol == "main"
                            && source.contains("def main(")),
                    "Mapped behavior is missing: {target}"
                );
            }
        }
    }
}
