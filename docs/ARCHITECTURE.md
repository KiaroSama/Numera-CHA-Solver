# Architecture

`SDK / CLI / HTTP -> strict input -> modality hint -> declared local engine -> recognition result`

Owner-side assessment validation is independent of recognition. The initial release does not drive third-party provider challenges or infer verification from tokens.

## Responsibilities

- `core/`: typed observation/result/error contracts, restricted arithmetic, reversible geometry and revision validation. No browser, HTTP or database implementation.
- `sdk.py`: shared async `recognize`/`solve`/`detect`/`capabilities`/`models`/`doctor` routes and synchronous event-loop guard. `solve` does not add browser actions or acceptance. Explicit unsupported subtypes are rejected rather than silently routed to generic OCR.
- `verification.py`: exact provider/hostname/action/session binding against owner-supplied evidence. Test-key success remains fixture completion; no network acceptance is inferred.
- `engines/`: one owned subprocess, one cached ONNX session, generation-fenced cancellation and strict measured CUDA policy. Verification occurs outside the ownership lock; invalidated work cannot publish a late process or dispose a replacement generation. CPU preprocessing is explicit.
- `models.py`: reviewed immutable manifest, size/hash validation and atomic, bounded downloads from allowlisted HTTPS origins.
- `store.py` / `tasks.py`: SQLite WAL, idempotent creation, atomic terminal compare-and-set, bounded queue, cancellation and restart interruption without replay.
- `api.py`: authenticated loopback-only requests, same-origin sessions, strict schemas, byte limits and redacted errors. No URL/path/endpoint input from clients.
- `startup.py`: one exclusive root lease, socket bind, lifespan initialization, then atomic permission-restricted credential publication. Failed competing startup leaves active credentials intact.
- `readiness.py`: timestamped 30-second snapshot, at most one integrity refresh off the event loop. Initial `checking` and failures are explicit; arithmetic remains ready independently.
- `resources/web/`: dependency-free dashboard consuming the same HTTP contracts; no second solver implementation.

## Constraints

One local operator and one inference worker. No Redis, distributed scheduler, stealth browser, remote model code, training stack or paid-service fallback. Tasks have a 120-second ceiling, queue capacity 32 and 24-hour result TTL. Pending inputs are memory-only; persistence contains bounded sanitized result metadata, never images or tokens.

Graceful shutdown terminalizes queued tasks as cancelled, and partial startup failures release acquired store ownership. A forced interruption still uses restart recovery; it never replays pending actions.

SDK admission bounds simultaneous recognition callers to the configured queue-size limit; overload returns `inference_busy`. Image validation is offloaded under the inherited deadline. Session profiling is immutable, but tensor shape is per-request evidence.

Result TTL is a retrieval limit, not a guaranteed physical deletion deadline: expired terminal rows are removed on later task creation. Idle database retention is not advertised as a physical purge guarantee.

The current implementation uses wall-clock timestamps for persisted restart/TTL metadata and a monotonic remaining budget during execution. Recognition does not perform external actions, so restart interruption never replays a submitted provider action.
