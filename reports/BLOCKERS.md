# Initial release blockers

| Area | Cause and evidence | Alternatives examined | Next action |
| --- | --- | --- | --- |
| C01-C23 provider native recognition/E2E | No licensed subtype fixtures, evaluated native model or owner-origin lifecycle. All rows retained in coverage. | Directive shortlist reviewed selectively; browser stealth, replicas, hosted defaults and registry-only claims rejected. | Supply an owned licensed labeled subtype fixture and documented owner verification lifecycle; no live execution until approved. |
| Distorted/Amazon-style text | Generic PP-OCR ASCII smoke is not distorted CAPTCHA evidence. | ddddocr source/weights provenance and API mismatch reviewed; unqualified weights not selected. | Obtain licensed real samples and evaluate exact-string/character error independently. |
| Audio | No licensed speech/digit/sound dataset or selected evaluated weights. | faster-whisper remains a research candidate, not installed or claimed working. | Provide owned labeled audio and locale/answer semantics. |
| Spatial/temporal/VLM | Geometry is implemented, but no evaluated perception checkpoint/temporal fixture. | Heavy multimodal/training stacks deferred; no external hidden fallback. | Supply licensed subtype data and choose a traceable hardware-compatible local model. |
| Multilingual model | PaddleOCR issue18364 reports possible non-ASCII corruption; not reproduced. | No blind re-encoding workaround. | Evaluate paired dictionary/output and real non-ASCII labels before promotion. |
| Official network/live integration | Contract fixtures do not prove provider network acceptance; live scope deferred. | Google/Turnstile official docs/test keys researched. | Run bounded owner-controlled official positive/negative integration separately when selected. |
| Native migration verification — resolved | Rust 0.2.0 passed final exact revision 43c272e / CI37802044609: 43 native tests and all 16 stages; source retirement, installed launchers, GPU/CPU and copied-state rollback verified. | Historic source oracle recreates Python 3.14.8 test-only dependencies; application runtime does not use Python. | Old application environments and disposable project caches removed after green; see [Validation](VALIDATION.md). |
| Platform breadth | Windows-only product by owner decision; other platforms are out of scope. | Portable Rust core retained without a cross-platform support promise. | Do not infer Linux/macOS or cross-platform GPU reliability from Windows tests. |

## Next perception slice: explicit intake

The owner confirmed on 2026-10-04 that no licensed labeled image set is available. Offline perception does not require live credentials, but it still requires licensed inputs and traceable weights before accuracy can be measured. The named MIT-code reference `DannyLuna17/VisionAIRecaptchaSolver` was inspected at `133aaa47ba28f027e0c3ffec4a8708ca3daf7bae`; its code license alone does not establish dataset/weight redistribution or evaluated accuracy here. No provider engine was copied or claimed operational.

Next bounded experiment: obtain an owner-controlled labeled image subtype set with dataset provenance/license; verify the selected checkpoint's own license and immutable hash; evaluate only local perception with exact-string/tile metrics before considering owner-browser integration. Domain replication, hosts/certificate changes and stealth are excluded. Other provider subtypes remain unevaluated, not disproved or inherited from the generic OCR smoke.

This is a partial first release, not full completion of the original multi-provider mission.
