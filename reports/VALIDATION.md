# Validation

## Rust migration — in progress (2026-10-08)

Rust 0.2.0 is implemented in the working tree but is not yet certified as the replacement. No migration commit/push or final exact-SHA native CI has occurred; old source/runtime remain until all gates and rollback pass.

Observed native focused evidence: 24 source arithmetic cases; initial 10 SDK cases; owned ASCII preprocessing 184320 bytes exactly equal to pinned Pillow output; actual CLI answer `36`; actual CUDA OCR `NUMERA 123` with 187 CUDA/2 CPU support nodes and `[1,3,48,320]`; 13 source-generated HTTP status/body/header cases including >2 MiB JSON input; restricted atomic credential replacement/failure preservation. These are focused development checks, not full-suite evidence. Current source oracle runs Python 3.14.8, while historical release evidence below ran 3.14.7.

Required native lifecycle, installed package/launchers, full CUDA/CPU parity, headless dashboard, source-runtime copied-state rollback, replay/resource comparisons and final cleanup remain unverified. The controlled worker binary-protocol isolation and handshake/inference cancellation/reap/successor tracers now pass (0.66s and 4.16s); initial fixture failures remain recorded, and a single focused pass is not repeat-stability evidence. Verification-time cancellation, public deadline, active shutdown and unacknowledged-result race regressions still await final CI. Fixture mappings await final CI, not inferred green from implementation.

Status: partial local application. Initial runtime revision `f9e71d87cb1205f6dd659234fc458339b2251452` passed manual Windows CI [36944907182](https://github.com/KiaroSama/Numera-CHA-Solver/actions/runs/36944907182): 24 passed, 0 failed, 0 skipped, 1 upstream TestClient deprecation warning; measured code coverage 67%. This evidence does not certify later revisions.

## Commit-bound evidence index

| Revision | Run | Verified scope |
| --- | --- | --- |
| `6ae5a2d24b6b9dd3be58b39751d604468edd2762` | [37068455085](https://github.com/KiaroSama/Numera-CHA-Solver/actions/runs/37068455085) | 55 passed, 0 failed/skipped, 1 warning, 74% statement coverage; Windows GPU/browser/package gates |
| `f681183a9c4763295a570da3e57361b09b07a8cc` | [37076991398](https://github.com/KiaroSama/Numera-CHA-Solver/actions/runs/37076991398) | Same 55-test baseline on repository-hygiene revision; not provider accuracy |
| `92afcbf231cbde290212302e1e9b19e9fa607874` | [37229059450](https://github.com/KiaroSama/Numera-CHA-Solver/actions/runs/37229059450) | 70 passed, 0 failed/skipped, 1 warning in 34.49s; 79% statement coverage; Windows GPU/CPU smoke, browser, package and isolated installed-wheel gates |

Each run's `validation` artifact contains JUnit results. Historical measurements remain attached to their own revision.

## Reliability repair milestone

Eight supplied failure scenarios were reproduced on the locked Windows environment and repaired at their owning boundaries. Persistent regressions cover cancellation during verification/handshake with a real synthetic child, competing startup credential stability, stale/crashed download ownership, exact duplicate evidence, deadline outcome, malformed saved config, quoted/nested/exception redaction, and event-loop responsiveness during health polling. Synthetic worker/evaluator cases prove lifecycle semantics only.

The successful repair run above includes JUnit and coverage JSON in its `validation` artifact. The first repair run failed on a missing-model exception classification and crash-test cleanup; both were corrected before the successful replacement run. The suite includes real GPU and explicit CPU smoke, archive inspection and a fresh installed-wheel smoke. Statement coverage without subprocess instrumentation is not a measurement of the spawned worker's executed lines; actual provider profiling remains separate evidence.

## Observed focused execution

- Arithmetic public SDK returned `36` for `12 * 3 = ?`, without server verification.
- Missing-model registry and OCR availability return explicit `model_missing`; no fabricated result.
- Durable cancellation/idempotency tracer passed; terminal cancelled task cannot be overwritten.
- HTTP authorization/strict-schema/shared-SDK tracer passed; endpoint override rejected.
- Owner-context verification tracer passed; test-key-style fixture success remains `challenge_completed`, not production `server_verified`.
- Static type check passed on the implementation tree before final review.

## Actual model/device evidence

Selected model revision `b8f84f0b80c529de40b4fbb3544b84fa7233a513`, raw model SHA256 `5435fd747c9e0efe15a96d0b378d5bd157e9492ed8fd80edf08f30d02fa24634`.
Owned synthetic input: `NUMERA 123`; exact output: `NUMERA 123`; heuristic confidence `0.9980584383`.
Device: RTX4070Ti, ONNX RuntimeGPU1.30.0, Windows, CPython3.14.7.
Profile:187CUDAExecutionProvider nodes,2CPUExecutionProvider shape/support nodes; expensive Conv/MatMul/Gemm/Attention not observed on CPU. Preprocessing CPU, output class map18710, input `[1,3,48,320]`.

This single sample is a wiring smoke, not measured CAPTCHA accuracy. The repair run verified CPU/GPU answer and tensor-shape parity for this same single synthetic sample only. Broad CPU/GPU parity, peak VRAM, non-ASCII correctness and other hardware are not verified.

## Initial failures and repairs

Model acquisition first rejected a new official CDN hostname; inspected public302 redirect to `us.aws.cdn.hf.co`, verified official Xet storage docs, added only that hostname, retained checksum/size/HTTPS bounds. Same bounded scenario then passed.
CPU/GPU distributions briefly overlapped during optional-group setup; final environment removed CPU and reinstalled GPU namespace. Only one ORT distribution is present.
Python3.14.8 was verified latest but uv download metadata did not offer it; nearest available3.14.7 recorded. uv0.12.21 update was blocked by unrelated busyuvx; no unrelated process terminated.

Final exact-SHA CI outcome is recorded after publication, not invented here.
