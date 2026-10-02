# Validation

Status: partial local application. Initial runtime revision `f9e71d87cb1205f6dd659234fc458339b2251452` passed manual Windows CI [36944907182](https://github.com/KiaroSama/Numera-CHA-Solver/actions/runs/36944907182): 24 passed, 0 failed, 0 skipped, 1 upstream TestClient deprecation warning; measured code coverage 67%. This evidence does not certify later revisions.

Continuation: shared SDK routes, explicit subtype rejection, provider-bound assessment, bounded benchmark validation and lifecycle cleanup repairs have focused regression evidence. The integrated CI result for this continuation is pending until the final commit is published; no newer SHA is marked green here.

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

This single sample is a wiring smoke, not measured CAPTCHA accuracy. CPU/GPU parity, peak VRAM, non-ASCII correctness and other hardware are not verified.

## Initial failures and repairs

Model acquisition first rejected a new official CDN hostname; inspected public302 redirect to `us.aws.cdn.hf.co`, verified official Xet storage docs, added only that hostname, retained checksum/size/HTTPS bounds. Same bounded scenario then passed.
CPU/GPU distributions briefly overlapped during optional-group setup; final environment removed CPU and reinstalled GPU namespace. Only one ORT distribution is present.
Python3.14.8 was verified latest but uv download metadata did not offer it; nearest available3.14.7 recorded. uv0.12.21 update was blocked by unrelated busyuvx; no unrelated process terminated.

Final exact-SHA CI outcome is recorded after publication, not invented here.
