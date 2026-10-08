# Numera CHA-Solver

<div align="center">

<img src="assets/brand/logo.png" width="320" alt="Numera blue and cyan circular logo">

[![License: Proprietary](https://img.shields.io/badge/License-Proprietary-2ea44f?style=flat-square)](LICENSE) [![Version: 0.2.0](https://img.shields.io/badge/Version-0.2.0-0d6efd?style=flat-square)](Cargo.toml) [![Language: Rust](https://img.shields.io/badge/Language-Rust-ce422b?style=flat-square)](Cargo.toml) [![Rust: 1.99](https://img.shields.io/badge/Rust-1.99-ce422b?style=flat-square)](Cargo.toml) [![OS: Windows 11](https://img.shields.io/badge/OS-Windows%2011-2563eb?style=flat-square)](docs/QUICKSTART.fa.md) [![Inference: ONNX Runtime](https://img.shields.io/badge/Inference-ONNX%20Runtime-2563eb?style=flat-square)](docs/MODELS.md) [![Device: CUDA or explicit CPU](https://img.shields.io/badge/Device-CUDA%20or%20explicit%20CPU-2563eb?style=flat-square)](docs/GPU.md) [![API: Axum](https://img.shields.io/badge/API-Axum-2563eb?style=flat-square)](docs/ARCHITECTURE.md) [![Storage: SQLite](https://img.shields.io/badge/Storage-SQLite-2563eb?style=flat-square)](docs/ARCHITECTURE.md) [![Dependencies: Cargo locked](https://img.shields.io/badge/Dependencies-Cargo%20locked-008080?style=flat-square)](Cargo.lock) [![Dashboard: local HTML](https://img.shields.io/badge/Dashboard-local%20HTML-2563eb?style=flat-square)](docs/QUICKSTART.fa.md) [![Documentation: English and Persian](https://img.shields.io/badge/Docs-English%20%7C%20Persian-0369a1?style=flat-square)](docs/QUICKSTART.fa.md) [![Support donations](https://img.shields.io/badge/Support-donations-d04a9a)](#donate)

</div>

**Private local perception and owner-controlled verification laboratory—not a universal CAPTCHA solver.**

Rust 0.2.0 passed the pre-retirement native verification on `4a5e60b`: 43 native tests plus all 15 build/device/browser/package/launcher/rollback stages. The previous application source is now retired; final source-retirement verification and disposable cache cleanup are in progress. Exact commit-bound evidence is in [Validation](reports/VALIDATION.md).

Available: restricted arithmetic, actual local text-line OCR, typed async SDK, JSON CLI, authenticated loopback API, durable cancellable tasks, model inventory/acquisition/verification and local evidence dashboard. No paid-service hidden fallback, browser stealth, target impersonation or third-party protection bypass.

## Start

PowerShell7:

```powershell
./scripts/setup.ps1
```

```powershell
./bin/numera-cha.exe models fetch ppocr-v6-small
```

```powershell
./bin/numera-cha.exe recognize --text "12 * 3 = ?"
```

```powershell
./run.ps1
```

Dashboard: `http://127.0.0.1:8765`. Connect with the locally generated credential in `data/api-token.txt`; never publish or attach its value. Stop with `./scripts/stop.ps1`.

Neural inference defaults to CUDA. Set `NUMERA_DLL_DIRECTORY` to compatible CUDA/cuDNN DLLs if needed. CPU inference is explicit: `./bin/numera-cha.exe --device cpu recognize --image <licensed-image>`.

## SDK

```rust
use numera_cha_solver::{config::Settings, sdk::Numera};
use serde_json::json;

#[tokio::main]
async fn main() {
    let sdk = Numera::new(Settings::from_env().expect("valid local configuration"));
    let result = sdk.recognize_value(json!({"kind": "text", "text": "12 * 3"}), None).await;
    println!("{}", serde_json::to_string(&result).expect("result serialization"));
    sdk.close().await;
}
```

Native interfaces: async `recognize(Input, deadline)`, `recognize_value(JSON, deadline)`, `solve`, `capabilities`, `models`, `doctor` and `close`; synchronous metadata-only `detect(&Input)`. CLI and HTTP share this SDK. `solve` is recognition only, never a browser action or owner acceptance. Explicit subtypes must match the implemented engine (`arithmetic` or `generic_text_line`).

Python import compatibility and its synchronous wrapper are intentionally retired by the owner-approved migration. No thin Python production wrapper is provided. `recognized` never implies `server_verified`.

## CLI and API

Commands: `doctor`, `capabilities`, `models list|verify|fetch`, `detect`, `recognize`, `solve`, `serve`, `benchmark`, `report`. JSON goes to stdout; logs go to stderr and per-run files. Exit 0: success; 2: input/configuration/infrastructure failure; 3: unavailable/abstention; 4: failed recognition. Exit 130 denotes an interrupted non-server command; actual console-signal verification remains separate from ordinary JSON/exit tests.

HTTP: `/health/live`, `/health/ready`, `/v1/capabilities`, `/v1/models`, `/v1/detect`, `/v1/recognize`, `/v1/tasks`, `/v1/tasks/{id}`, `/v1/tasks/{id}/cancel`. Bearer or local same-origin session required for v1; task creation requires `Idempotency-Key`. Strict JSON input accepts local data, not arbitrary paths/URLs/endpoints. Default queue32, taskbudget120seconds, retrieval TTL24hours (not a physical-purge guarantee). `/health/ready` returns a timestamped cached integrity snapshot; `checking` is not model readiness. Recognition overload returns `inference_busy`.

## Evidence and coverage

Real first-party PP-OCRv6small weights read the owned synthetic `NUMERA 123` sample on RTX4070Ti. Runtime profiling recorded187CUDA and2CPUshape/support nodes. This proves local model wiring/device execution only—not real CAPTCHA accuracy, multilingual correctness or live acceptance.

<!-- COVERAGE:START -->
| ID | Family | Declared scope | Evidence / blockers |
| --- | --- | --- | --- |
| C01 | Google reCAPTCHA | 5 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C02 | Google reCAPTCHA | 2 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C03 | Cloudflare Turnstile | 3 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C04 | Cloudflare challenges | 3 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C05 | hCaptcha | 6 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C06 | Arkose FunCaptcha | 6 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C07 | GeeTest | 4 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C08 | AWS WAF | 2 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C09 | Tencent TCaptcha | 4 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C10 | Alibaba CAPTCHA | 3 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C11 | NetEase Yidun | 3 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C12 | Yandex SmartCaptcha | 2 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C13 | MTCaptcha | 2 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C14 | Capy | 2 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C15 | Lemin Cropped | 1 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C16 | KeyCAPTCHA | 2 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C17 | Friendly Captcha | 2 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C18 | ALTCHA | 1 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C19 | Prosopo Procaptcha | 2 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C20 | Cutcaptcha | 1 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C21 | VK CAPTCHA | 1 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C22 | DataDome | 2 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C23 | HUMAN PerimeterX | 2 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C24 | Generic text | 5 subtype decisions | Limited local building blocks; provider accuracy unverified |
| C25 | Generic audio | 3 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
| C26 | Generic spatial | 9 subtype decisions | Blocked: model/data/owned lifecycle unavailable |
<!-- COVERAGE:END -->

[Validation](reports/VALIDATION.md) · [Blockers](reports/BLOCKERS.md) · [Benchmark limits](reports/BENCHMARKS.md) · [Upstream decisions](reports/UPSTREAM_REVIEW.md)

## Documentation

[Persian quickstart](docs/QUICKSTART.fa.md) · [Architecture](docs/ARCHITECTURE.md) · [Providers](docs/PROVIDERS.md) · [Models](docs/MODELS.md) · [GPU](docs/GPU.md) · [Testing](docs/TESTING.md) · [Security](docs/SECURITY.md)

## Logs

UTF8 diagnostic files live under `logs`, resolved from project root. One per execution: `<script>_YYYY-MM-DD_HH-mm-ss_UTC.log`; UTC entries `[timestamp UTC] [LEVEL] [COMPONENT] Message`. INFO/WARNING/ERROR/DEBUG supported; `--debug` enables verbose operational metadata. Logging initialization falls back safely to stderr. Native logs sanitize quoted/nested fields before writing; runtime errors expose stable messages, not raw sensitive exception payloads. Logs rotate at 2 MB with two backups per run; real operational runs are not deleted automatically. Credentials, authorization, cookies, response tokens, signed URLs and raw inputs are not logged. Attach only sanitized relevant logs, never credentials/raw private assets.

## Development and manual CI

Native dependencies are pinned by `Cargo.lock`; exact `ort` prerelease is an explicit reviewed exception. ONNX/CUDA native DLLs are external libraries, not a Python application dependency. The dedicated Windows repositoryrunner starts manually, has no service/startupregistration, and runs only a manualworkflow. Deterministic/model/browser/packagechecks and their limitations are documented in [Testing](docs/TESTING.md). No staticgreenpassingbadge substitutes for exactSHAevidence.

Originalcode is proprietary, matching the user's requested license. Third-party weights/dependencies retain their grants; see [LICENSE](LICENSE) and [third-party notices](THIRD_PARTY_NOTICES.md).

## Donate

If this project helps you, donations are appreciated.

| Currency | Network | Address |
| --- | --- | --- |
| Bitcoin (BTC) | Bitcoin | `bc1qmth5m03pu5hujw5xw5jmywam3jj3sqwqupesdt` |
| USDT, BNB, USDC, etc. | BEP20 | `0x0Bd0BA443a8B9cf15922bf7f0Bb0a4b495fD06Ef` |
| USDT, TRX, USDC, etc. | TRC20 | `TWBA3xFTqgZAeAYMxqo85xWnzvty3DcAhw` |
| Ethereum (ETH) | ERC20 | `0x0Bd0BA443a8B9cf15922bf7f0Bb0a4b495fD06Ef` |
| TON | TON | `UQCN8Umo_OfOWqImZetQsrNStPcmLkMAKajFyiCOhso23NDb` |
| Litecoin (LTC) | LTC | `ltc1qntqnnrunadurnw4cshv3qgspywrueyyeyngwuy` |
| Solana (SOL) | Solana | `7B2wkczUjmkDhETwQuknBL8sUsbuV7nErxc317TmQuwR` |
| Polygon (POL) | Polygon | `0x0Bd0BA443a8B9cf15922bf7f0Bb0a4b495fD06Ef` |
