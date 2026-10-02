# Testing and evidence

The confirmed test seams are SDK/CLI, API/task lifecycle, real model/device, owned browser and distribution contents. Tests exercise public behavior; no mocked result is counted as model inference.

Focused TDD tracers were run while implementing arithmetic, missing models, image availability, durable cancellation/idempotency, API authorization and assessment binding. Continuation regressions cover SDK method parity, provider binding, malformed benchmark manifests, queued shutdown cancellation and recovery after partial startup failure; each repaired seam first reproduced its failure. Additional bounded public cases cover input shape, corrupt assets, TTL and queue pressure. The final integrated suite runs once in manually dispatched Windows CI, not as duplicate local and remote suites.

## Manual local runner

The dedicated repository runner stays offline until a human starts it for a manual workflow. It has no service/startup entry and is located inside the project. The workflow uses `workflow_dispatch` only; no push/PR starts code on the user's machine. Stop its owned PID/tree immediately after the workflow finishes.

```powershell
./scripts/ci.ps1 -Gpu -Browser
```

Use this command only for an explicitly selected local run or a recorded CI-unavailable fallback. Test cases have native timeouts; the script provides outer wall/idle ceilings, captured output, project-owned temp files and one worker. GPU tests use licensed model assets from the owning runtime root; browser tests use isolated headless contexts against local fixture pages. No ordinary browser profile is reused.

## Evidence meanings

- Unit/public-contract: parser, invalid inputs, geometry, verification context, redaction, cancellation/restart and interface consistency.
- Real model smoke: selected immutable weights read owned synthetic ASCII artwork. Not CAPTCHA accuracy, not a held-out evaluation.
- Owned browser: dashboard authorization, arithmetic task/result, responsive width and reduced motion. Not production provider acceptance.
- Official test keys: documented contracts researched; actual positive/negative provider network integration is not executed in the initial deterministic suite and remains blocked/unverified.
- Authorized live E2E: not run, no owner origin or real labeled provider dataset provided.
- Packaging: strict UTF-8, source ceiling, coverage synchronization, wheel/source contents and private-artifact exclusions.

Skipped optional tiers have explicit reasons and are not passed. The GPU/browser tiers are selected for the first manual Windows run; other hardware/platforms and production integrations remain untested.
