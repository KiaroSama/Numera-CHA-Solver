# Numera CHA-Solver

<div align="center">

<img src="assets/brand/logo.png" width="320" alt="Numera blue and cyan circular logo">

![Version](https://img.shields.io/badge/version-0.1.0-2563EB?style=for-the-badge&labelColor=172033)
![License](https://img.shields.io/badge/license-Proprietary-7C3AED?style=for-the-badge&labelColor=172033)
![Language](https://img.shields.io/badge/language-Python-007ACC?style=for-the-badge&labelColor=172033)
![Runtime](https://img.shields.io/badge/runtime-Python_3.12--3.14-4338CA?style=for-the-badge&labelColor=172033)

![Platform](https://img.shields.io/badge/platform-Windows_11-0067C0?style=for-the-badge&labelColor=172033)
![Inference](https://img.shields.io/badge/inference-ONNX_Runtime-9333EA?style=for-the-badge&labelColor=172033)
![GPU](https://img.shields.io/badge/device-CUDA_or_explicit_CPU-3F7D00?style=for-the-badge&labelColor=172033)
![API](https://img.shields.io/badge/API-FastAPI-00826D?style=for-the-badge&labelColor=172033)
![Storage](https://img.shields.io/badge/storage-SQLite-0369A1?style=for-the-badge&labelColor=172033)

![Dependencies](https://img.shields.io/badge/dependencies-uv_locked-BB1BCD?style=for-the-badge&labelColor=172033)
![Dashboard](https://img.shields.io/badge/dashboard-local_HTML-C2410C?style=for-the-badge&labelColor=172033)
[![Docs](https://img.shields.io/badge/docs-English_%2B_Persian-BE185D?style=for-the-badge&labelColor=172033)](docs/QUICKSTART.fa.md)
[![Support donations](https://img.shields.io/badge/Support-donations-d04a9a?style=for-the-badge&labelColor=172033)](#donate)

</div>

**Private local perception and owner-controlled verification laboratory. Initial release with explicit limited coverage—not a universal CAPTCHA solver.**

Available: restricted arithmetic, actual local text-line OCR, typed async SDK, JSON CLI, authenticated loopback API, durable cancellable tasks, model inventory/acquisition/verification and local evidence dashboard. No paid-service hidden fallback, browser stealth, target impersonation or third-party protection bypass.

## Start

PowerShell7:

```powershell
./scripts/setup.ps1
```

```powershell
uv run numera-cha models fetch ppocr-v6-small
```

```powershell
uv run numera-cha recognize --text "12 * 3 = ?"
```

```powershell
./run.ps1
```

Dashboard: `http://127.0.0.1:8765`. Connect with the locally generated credential in `data/api-token.txt`; never publish or attach its value. Stop with `./scripts/stop.ps1`.

Neural inference defaults to CUDA. Set `NUMERA_DLL_DIRECTORY` to compatible CUDA/cuDNN DLLs if needed. CPU inference is explicit: `uv run numera-cha --device cpu recognize --image <licensed-image>`.

## SDK

```python
import asyncio
from numera_cha_solver.sdk import Numera

async def example():
    sdk = Numera()
    try:
        result = await sdk.recognize({"kind": "text", "text": "12 * 3"})
        print(result.to_dict())
    finally:
        sdk.close()

asyncio.run(example())
```

`recognized` never implies `server_verified`. The synchronous wrapper rejects an already-running event loop rather than deadlocking.

## CLI and API

Commands: `doctor`, `capabilities`, `models list|verify|fetch`, `detect`, `recognize`, `solve`, `serve`, `benchmark`, `report`. JSON goes to stdout; logs go to stderr and per-run files. Exit0success,2invalidinput/config,3unavailable/abstention,4failedrecognition,130cancelled.

HTTP: `/health/live`, `/health/ready`, `/v1/capabilities`, `/v1/models`, `/v1/detect`, `/v1/recognize`, `/v1/tasks`, `/v1/tasks/{id}`, `/v1/tasks/{id}/cancel`. Bearer or local same-origin session required for v1; task creation requires `Idempotency-Key`. Strict JSON input accepts local data, not arbitrary paths/URLs/endpoints. Default queue32, taskbudget120seconds, resultTTL24hours.

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

UTF8 diagnostic files live under `logs`, resolved from project root. One per execution: `<script>_YYYY-MM-DD_HH-mm-ss_UTC.log`; UTC entries `[timestamp UTC] [LEVEL] [COMPONENT] Message`. INFO/WARNING/ERROR/DEBUG supported; `--debug` enables verbose operational metadata. Logging initialization falls back safely to stderr. Pythonlogsrotateat2MBwith2backupsperrun; no automatic deletion of realoperationalruns. Credentials, authorization, cookies, response tokens, signed URLs and raw inputs are not logged. Attach only sanitized relevant logs, never credentials/raw private assets.

## Development and manual CI

Locked environment via `uv.lock`. `gpu` and `cpu` extras conflict intentionally; only one owns ONNXnamespace. The dedicated Windows repositoryrunner starts manually, has no service/startupregistration, and runs only a manualworkflow. Deterministic/model/browser/packagechecks and their limitations are documented in [Testing](docs/TESTING.md). No staticgreenpassingbadge substitutes for exactSHAevidence.

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
