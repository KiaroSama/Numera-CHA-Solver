# Testing and evidence

Confirmed seams: native SDK/CLI, authenticated HTTP/tasks/review, real model/device, MCP HTTP/stdio bridge, owned dashboard, desktop and installed distribution. Synthetic workers prove ownership behavior only, never OCR accuracy.

## Hosted-only delivery validation

Version0.3.0 is built/tested on GitHub-hosted Windows x64, Ubuntu24.04 x64 and Apple Silicon macOS. Main pushes, pull requests and manual dispatch trigger the native matrix; no local/self-hosted runner is used. The owner's current task explicitly excludes local builds and runtime tests. Source formatting/config syntax inspection does not count as execution evidence.

Each job first exercises duplicate executable discovery and fail-closed dependency notice selection (unknown version, mismatched license/revision and altered bytes). The discovery placeholders are never executed. Packaging verifies all omitted notice texts and includes unchanged preferred source for every MPL-covered dependency; missing or mismatched catalog entries block delivery.

Each job uses two compiler workers, one test thread and one model operation. Native tests have their own async/process deadlines; the captured outer command owner supplies wall/idle/tree cleanup. Stable libtest has no universal per-case kill timeout. Job wall limit is45min; each stage records exit status and duration. Genuine timeouts/failures block package upload, not waived by retries or disabled assertions. Native suite summaries and bounded redacted failure sections are surfaced in CI logs; a missing result capture fails validation. POSIX discovery does not read an already-owned PID/create-time identity's environment again after exec/exit and excludes unknown ended processes, but a live unknown same-user process with inaccessible ownership evidence still blocks the run. No OS security settings are weakened.

## Required tiers

- Source-generated JSON/binary conformance: arithmetic, SDK, HTTP status/body/security headers, geometry, owner assessment, CTC, preprocessing and request digests. Expectations remain generated from the historical Python source, not target translations. Windows additionally regenerates a temporary frozen historical source snapshot and verifies drift/rollback; no historical application is delivered.
- Native lifecycle/negative cases: bounded config/input/files/model integrity, download ownership/crash, redaction, credentials/socket/root collision, task idempotency/TTL/restart/overload, decoder and worker cancellation/reap/successor. The standalone worker fixture is compiled once in CI and supplied through `NUMERA_FIXTURE_WORKER_EXECUTABLE`.
- New public boundaries: MCP initialize/list/call/auth/host/origin/input and bridge failure/protocol/cleanup; manual correction/confirm/cancel/expiry/stale terminal state; native runtime names and POSIX credential modes. Unsupported external input is rejected and human confirmation is not an agent tool.
- Actual device: official CPU ONNX Runtime1.31.0 and immutable owned ASCII image on every hosted OS. Exact answer/input-shape and `server_verified=false` asserted. Hosted CPU evidence is not a skipped CUDA pass; legacy CUDA tests remain a separate explicitly selected hardware tier.
- Browser/distribution: isolated headless dashboard auth/arithmetic/manual review/mobile/reduced-motion on each OS; native Linux installed desktop WebDriver/Xvfb smoke; Windows launcher ownership/source rollback; native package/source/license/hash/private inclusion checks. Windows/macOS native GUI interaction and trusted signing/notarization are not claimed from headless browser/build tests.

## Evidence interpretation

The 45 historical source function dispositions in `tests/fixtures/parity/source-tests.json` map to implemented native coverage; the original generation markers are historical metadata. They are not a pass receipt for later changes. [Validation](../reports/VALIDATION.md) records exact-revision execution evidence. Test counts, timings, compiler results and synthetic recognition do not measure real CAPTCHA accuracy, cross-version performance improvements or full Rust line coverage.

Required real labeled CAPTCHA/audio/provider fixtures are unavailable; no live provider acceptance or new accuracy claim is made. Models/user data/operational logs/shared tools survive cleanup. Disposable hosted runner work/cache is job-contained, and project-owned old self-hosted installation/registration is removed separately after inspection.
