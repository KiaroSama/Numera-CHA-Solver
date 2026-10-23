# Architecture

`Rust SDK / CLI / Axum HTTP -> strict observation -> declared local engine -> recognition result`

Owner-side assessment is separate from perception. The native application never infers server acceptance from a token or drives a third-party protection bypass. The existing HTML/CSS/JavaScript dashboard is preserved; Tauri embeds it with an owned loopback API, restricted IPC connection and explicit human review.

## Responsibilities

- `src/core/`: typed observation/result/error contracts, bounded arithmetic and reversible geometry/revision checks. Independent of HTTP, persistence and native runtime libraries.
- `src/sdk.rs`: shared recognition, solve, detection, inventory and diagnosis. Explicit subtype validation, bounded admission and inherited monotonic deadlines. The Python import SDK is intentionally replaced, not wrapped.
- `src/engines/`: Pillow-compatible BGR preprocessing, CTC decoding, one cached ONNX session in an owned native child, framed bounded IPC, strict measured CUDA execution. Retained operation ownership separates caller cancellation from verification/child cleanup; an unacknowledged result cannot publish a reusable late worker.
- `src/models.rs` / `assets.rs`: immutable reviewed assets, safe local decoding, exact sizes/hashes, allowlisted HTTPS downloads and crash-released revision lease. Explicit transport adapters still undergo immutable post-download verification.
- `src/store.rs` / `tasks.rs`: existing SQLite WAL schema, sorted-JSON-compatible idempotency digest, bounded queue and one owned consumer. Atomic terminal results cannot be overwritten; restart interrupts unfinished work without replay.
- `src/api.rs` / `startup.rs` / `readiness.rs`: loopback host/origin boundary, strict bounded JSON, bearer/same-origin sessions, root lease before exclusive socket binding, store initialization before restricted atomic credential publication, singleflight off-loop integrity snapshots.
- `src/verification.rs`: exact provider/hostname/action/session binding. Test-key success remains fixture completion, not production acceptance.
- `resources/web/`: retained dependency-free dashboard; no second recognition implementation. `scripts/` contains captured bounded Windows operational/build/package adapters.

## Contracts and limits

One local operator and one inference worker. No Redis, distributed scheduler, stealth browser, remote model execution, training stack or paid-service fallback. Operation ceiling 120 seconds, queue default 32/safety maximum 64, retrieval TTL default 24 hours. Inputs remain in memory; persisted results contain bounded metadata, not images or credentials.

Shutdown cancels queued/active tasks and joins owned work. Arbitrary OS disk I/O cannot be force-interrupted safely inside a thread; draining work remains owned and blocks replacement rather than being falsely declared finished. Outer command/CI ceilings bound the process tree. Forced termination uses restart recovery, never replay.

Persistence uses wall-clock restart/TTL timestamps; execution and sessions use monotonic time. TTL is a retrieval bound, not scheduled physical purge: later task creation removes expired terminal rows.

## Agent and desktop boundaries

`src/mcp/` uses the official Rust MCP SDK. Default `bridge` owns the local SDK over stdio without an HTTP/token prerequisite; explicit `bridge --remote` relays to the owner's selected authenticated loopback endpoint. Each direct process owns its inference/admission lifecycle; persisted reviews share the selected root. EOF closes admission and drains owned work. Protocol stdout is separate from diagnostics. Tools reuse the SDK; agent confirmation and external submission are not exposed. `src/review.rs` persists exact revision-bound pending/terminal transitions with TTL and compare-and-set. Human attestation does not set owner/server acceptance.

`desktop/` hosts the static workspace with Tauri. The main embedded origin alone can request the in-memory connection token, external navigation is blocked, and the owned API/worker drains on close. Windows/Linux/macOS packages use native platform runtime and worker names. Packaged desktop defaults to its CPU runtime unless explicitly configured; strict CUDA never silently falls back.

## Migration boundary

Rust 0.2.0 intentionally retires Python SDK imports and preserves command/HTTP semantics. Runtime-version diagnostics truthfully change to native fields; no fabricated Python values. Source-generated fixtures retain exact meaningful answers/errors and normalize named nondeterministic request IDs only. Current migration verification status is in [Validation](../reports/VALIDATION.md); design/source inspection is not final runtime evidence.
