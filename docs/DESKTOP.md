# Native desktop delivery

Download your platform from [GitHub Releases](https://github.com/KiaroSama/Numera-CHA-Solver/releases), verify the detached archive signature against the pinned key ([signing](SIGNING.md)) and `SHA256SUMS.txt`, and read the included license and third-party notices before installing. A version is published only after all required native checks pass; an authored or failing version is not a release. Version0.4.1 uses a Tauri2.12.4 Rust shell around the existing Numera workspace; no new JavaScript framework or visual redesign.

## Packages

| Artifact | Architecture | Contents and prerequisites |
|---|---|---|
| `numera-cha-desktop-0.4.1-windows-x64.zip` | Windows x64 | Owner self-signed Authenticode NSIS installer. Microsoft WebView2 is required; the installer may fetch Microsoft's bootstrapper when absent. |
| `numera-cha-desktop-0.4.1-linux-x64.zip` | Linux x64 | Signed-archive Debian package, natively built on Ubuntu24.04. Requires its declared WebKitGTK4.1/GTK system dependencies. Not a universal Linux compatibility claim. |
| `numera-cha-desktop-0.4.1-macos-arm64.zip` | Apple Silicon | Owner self-signed `.app` archive, macOS11 or later. Runtime aliases retain verified native signatures; hardened-runtime distribution is not enabled. No Apple identity, notarization or Gatekeeper approval claim. Intel macOS is not included. |
| `numera-cha-0.4.1-<platform>.zip` | Matching architecture | Native CLI and `numera-cha-solver-mcp`, CPU ONNX Runtime1.31.0, original authored connector guidance/config examples and root plugin manifests, Windows `run.ps1`, matching source and independent notices. |

Packages have owner self-signed identity, **not publicly trusted** publisher signing. OS trust warnings are expected. Inspect provenance and obtain an approved signed distribution where your organization requires one; do not disable system-wide security controls. The owner's release policy promotes the exact inspected artifacts after the complete manually initiated native matrix succeeds and its exact source is integrated into the default branch. A separate manual promotion reuses that successful run's assets without rebuilding. No push, pull request or tag starts CI automatically. Existing versions/assets are never overwritten; a new version is required for a new release. Checksums and matching source are retained with the release, while disposable hosted work and temporary local inspection downloads are not retained. macOS/Linux binaries are linked on GitHub, not sent as chat attachments.

The supplied `package-manifest.json` records the source commit, platform, signing limitation and per-file SHA256/size. Every artifact includes the same revision's source archive. Runtime and Rust dependencies retain their independent licenses; the owner's custom noncommercial network share-alike license is not GNU AGPL or OSI open source.

## Local operation and preserved data

1. Install/extract the package for your actual OS and architecture. On Linux install the `.deb` with your package manager; on macOS extract the nested `.app.zip` with an application-aware archive utility. Command-line archives preserve executable modes in their ZIP entries; if an extractor drops modes, restore execute permission on `bin/numera-cha` and `bin/numera-cha-solver-mcp` only.
2. Open the native desktop. Existing workspace assets are embedded, not fetched from a remote page. The Rust owner binds an allocated `127.0.0.1` port, generates an in-memory credential and returns `{url,token}` only to its embedded `main` webview. External navigation/new windows and frontend shell/filesystem commands are disabled.
3. Use recognition and **review/correct/confirm** the suggestion yourself. Confirmation means a local manual attestation, never site acceptance or a cryptographic proof of human presence. This app submits nothing to third-party sites.
4. Close the application. The owner cancels and joins the API/tasks before exit; a failed bounded drain is logged as an error, not successful cleanup.

By default the desktop uses the platform's application-local data directory for writable task/review state, models and logs, not the read-only installation directory. Set `NUMERA_ROOT` explicitly to reuse an existing application root; the package never imports, replaces or deletes existing user data automatically. The CLI preserves its established root/configuration behavior. Paths with spaces are supported.

The desktop embeds the native `numera-cha` worker sidecar and CPU runtime library. Linux/macOS `.so`/`.dylib` aliases are materialized from the checked official archive without linked paths escaping the runtime. Model **weights are not distributed**: the immutable model manifest is included; use the documented explicit model acquisition workflow for assets you are entitled to use. A missing/corrupt model returns a structured blocker. `NUMERA_ORT_LIBRARY` can select an explicitly configured compatible runtime. The bundled desktop selects CPU with a visible diagnostic warning when no explicit device/runtime is configured, because its checked runtime has no accelerator provider. An explicitly configured CUDA request stays strict through the shared native contract. CLI users select `--device cpu` for this CPU distribution. Packages do not prove GPU availability.

## Hosted verification

All builds, tests and package generation for this change run **only on GitHub-hosted CI**, not on the owner's computer or a self-hosted runner. The workflow's native matrix is Windowsx64, Linuxx64 and macOSARM; job ceiling45minutes, compiler ceiling2jobs, test ceiling1thread.

- All mechanically mapped native integration targets, including `mcp`, `mcp_bridge`, `review` and `platform`, are mechanically discovered and mapped. `cargo test --workspace --all-targets --no-fail-fast -- --test-threads=1` also runs desktop origin/owned-API tests. A missing/unmapped tier or ignored required case fails the gate.
- `NUMERA_TEST_GPU=0` selects a **real CPU** inference tier on every OS, using official ONNX Runtime1.31.0 and the exact model revision/hashes in `resources/models.json`. The maintained owned ASCII image must recognize `NUMERA 123`; this is a synthetic model smoke, not real CAPTCHA accuracy. No hosted GPU pass is claimed.
- The existing headless browser harness runs on every OS against the actual native HTTP server. Linux additionally installs the built `.deb` and drives the actual desktop through `tauri-driver2.1.0`, `WebKitWebDriver` and Xvfb, checking trusted IPC bootstrap, arithmetic, manual correction/confirmation and graceful close.
- Windows retains its native atomic Job/launcher ownership, installed setup/doctor/start/stop and native immutable-fixture/persistence/restart gates. Historical Python execution and frozen environment restoration are retired; current native behavior and installed launcher coverage remain.
- Locked dependency metadata produces independent license texts/notices. Source allowlists and archive round-trip hashes exclude credentials, private configuration, assistant artifacts, runtime user state and model weights. Installed CLI arithmetic, invalid-input exit behavior, connector entry point and per-run logs are checked after extracting to a path with spaces.

**Native GUI limits:** direct upstream `tauri-driver` has no macOS WKWebView driver. macOS uses native compilation/unit/runtime/browser coverage, but native rendered GUI interaction is untested by that route. Windows native WebView automation is not claimed without a matching independently verified Edge driver. A headless Chromium HTTP dashboard test is not native WebView proof. Human review on those platforms and publicly trusted signing/notarization remain separate requirements; report them explicitly rather than calling all GUI coverage complete.

Each command has an outer wall/idle ceiling, captured output, stdin closed and owned-tree cleanup. Windows reuses the established atomic Job owner; POSIX combines launch-gated PID/create-time identities, refreshed parent-chain attribution, inherited per-command tags and Linux subreaping. Ended known children are OS-waited before deciding whether an executing descendant leaked; the root's real exit status is preserved. The owner has success/nonzero/idle/orphan/detached and ended-child regression fixtures. This is approved-tool ownership, not an adversarial sandbox or a guarantee of tracking an unobserved multi-generation detached process with inaccessible ownership evidence on macOS. The integrator verifies final exact-SHA CI; authored configuration alone is not a passing result.

## Diagnostics

The desktop and workers use the shared redacted UTF8 logger: a new `numera-cha-desktop_YYYY-MM-DD_HH-mm-ss_UTC.log` per run under the selected root's `logs/`, with safe collision suffixes. Entries use `[YYYY-MM-DD HH:mm:ss UTC] [LEVEL] [COMPONENT] Message`. INFO covers meaningful startup/shutdown and state changes; WARNING/ERROR identify missing prerequisites and failed operations; DEBUG is off by default. Credentials and authorization values never enter logs; file initialization failure uses safe stderr fallback. Existing size-bounded rotation/retention applies, and handlers flush/close on normal shutdown.

Hosted scripts write separate per-run structured logs in the isolated checkout and compact `results.json` stage timing/status evidence. Only the compact sanitized results and inspected distribution allowlist are uploaded. For support, inspect/redact a relevant log locally before sharing; never attach credential files, arbitrary user state or raw private project logs.

## CI entry points

These are **hosted job commands**, not local build instructions:

```powershell
./scripts/prepare_runtime.ps1 -Platform windows-x64
./scripts/hosted_ci.ps1 -Platform windows-x64 -Target x86_64-pc-windows-msvc
```

The same scripts accept `linux-x64`/`x86_64-unknown-linux-gnu` and `macos-arm64`/`aarch64-apple-darwin`. All scripts enforce the GitHub-hosted environment boundary. The workflow pins Rust1.99.0, Tauri CLI2.12.2 and test-tool versions; daily Dependabot covers the single locked Cargo workspace and GitHub Actions, without duplicate overlapping desktop update roots.

Sources: [Tauri capabilities](https://v2.tauri.app/security/capabilities/), [sidecars](https://v2.tauri.app/develop/sidecar/), [resources](https://v2.tauri.app/develop/resources/), [direct WebDriver limits](https://v2.tauri.app/develop/tests/webdriver/), [ONNX Runtime1.31.0](https://github.com/microsoft/onnxruntime/releases/tag/v1.31.0).
