# Native operation

Version0.4.0 provides hosted native CLI/MCP and Tauri delivery for Windows x64, Linux x64 and macOS Apple Silicon. Use the inspected [desktop/native artifacts](DESKTOP.md); [MCP setup](MCP.md) is separate. This task builds/tests only on hosted CI. The Windows source-build notes below preserve the earlier development workflow and do not authorize a local build in this delivery.

The replacement application is a Rust executable and library. The existing dashboard remains HTML/CSS/JavaScript. Python is not an application prerequisite. Current CI executes native contracts and immutable generated fixtures; historical Python source restoration and its frozen environment are retired.

## Intentional Python SDK retirement

Version 0.2.0 exposes a Rust crate only. Embedded consumers must install the actual `numera-cha.exe` beside their executable or in the configured root's `bin`, or set `NUMERA_WORKER_EXECUTABLE` to that approved binary. The SDK never assumes an arbitrary embedding executable implements the private worker protocol. The owner explicitly approved removal of Python imports and the synchronous running-event-loop wrapper; CLI/HTTP contracts and the static web dashboard are preserved. Browser and POSIX process-supervision verification are now Rust too; no tracked Python implementation or Python validation prerequisite remains.

## Build or install

Requirements for a source build: PowerShell 7, the selected Rust MSVC toolchain with Cargo, and Visual Studio C++ Build Tools/Windows SDK. Existing tools are reused. These scripts never install global tools automatically.

```powershell
./scripts/setup.ps1
./scripts/setup.ps1 -Yes -Engine gpu
./scripts/update.ps1 -Yes -Engine gpu
```

Setup and update default Enter to `[Y/n]`. `-Yes` enables noninteractive automation. A source checkout runs `cargo build --locked --release --bin numera-cha --jobs 2` through the selected real Cargo/rustc executables, avoiding rustup proxy process ownership ambiguity. Build outputs stay in the checkout. The installed executable is `bin/numera-cha.exe`.

`-Engine cpu` preserves the launcher option but does not create a separate application build or silently select CPU inference. Select CPU explicitly with `--device cpu`, or launcher `-Device cpu`.

Setup stages only the approved existing local ONNX Runtime libraries:

- `onnxruntime.dll`
- `onnxruntime_providers_shared.dll`
- `onnxruntime_providers_cuda.dll`

They live in `data/runtime`, together with ONNX Runtime license and third-party notices. If absent there, setup requires an explicitly supplied approved native runtime source; the prior Python environment fallback is retired. `-RuntimeSourceRoot <path>` selects another explicitly supplied local project runtime source. No native runtime download, model acquisition, Python installation or CUDA installation happens automatically. CUDA dependencies remain at the configured `NUMERA_DLL_DIRECTORY` or saved runtime configuration path. Missing runtime/license files block staging instead of inventing readiness.

For an extracted native package, `setup.ps1 -Yes` checks the included executable/runtime without invoking Cargo or Python. The hosted package includes a matching source ZIP for inspection; it is not an unpacked source checkout and setup does not rebuild it. Replace it with a newly inspected package while preserving data/models/configuration. Do not overlay an active executable.

## Start, stop and diagnose

```powershell
./scripts/start.ps1 -Port 8765
./scripts/doctor.ps1
./scripts/doctor.ps1 -Smoke ./tests/fixtures/ocr/ascii-smoke.png
./scripts/doctor.ps1 -Device cpu -Smoke ./tests/fixtures/ocr/ascii-smoke.png
./scripts/stop.ps1
./bin/numera-cha.exe models list
./bin/numera-cha.exe models fetch ppocr-v6-small
```

Launchers resolve the project from their own location and work from another current directory, including paths containing spaces. Start runs the authenticated loopback server in a captured, owned process with a 24-hour ceiling; stop verifies PID, start time, executable and project root before terminating it. A mismatched live identity is never killed. An obsolete ownership record can be inspected and removed after stop confirms no matching process exists. Stop also understands the old launcher record during rollback.

Diagnostic inventory is not inference proof. Native binding versions are distinct from the loaded ONNX Runtime version; GPU execution is established only by a successful smoke and actual provider profile. Owned ASCII smoke does not measure real CAPTCHA or multilingual accuracy. Neural CPU inference requires explicit selection and emits a warning.

## Logs and error handling

Launchers and the native executable write separate per-run UTF-8 files in `logs`, named `<name>_YYYY-MM-DD_HH-mm-ss_UTC[_unique].log`, with UTC entries `[timestamp UTC] [LEVEL] [COMPONENT] Message`. Console diagnostics go to stderr so command JSON remains on stdout. Native logs rotate at 2 MB with two retained backups; launcher logs have no automatic retention deletion. Keep relevant failure logs and redact personal context before sharing them. Tokens, passwords, cookies, authorization, response values and URLs are suppressed. File logging failure is reported and falls back safely to stderr.

Every invoked build, command or verification stage is captured and has wall/progress ceilings. Child exit statuses are preserved; startup/setup infrastructure errors return nonzero. A timeout or unconfirmed cleanup is a failure, not a successful run. File I/O and OS process termination have platform limits; a configured ceiling is not a claim that every blocking kernel operation can be interrupted immediately.

## Inspect a native package

```powershell
./scripts/native_package.ps1 -Build
./scripts/native_package.ps1 -BinaryPath ./target/release/numera-cha.exe -OutputDirectory ./.ci-work/native-packages
```

The archive includes exactly one native executable, reviewed static resources, project license, dependency license texts, runtime DLLs/notices, operational scripts and `package-manifest.json`. The manifest records each entry's length and SHA-256, and the completed ZIP is reopened and checked against that allowlist. Models, user databases, credentials, runtime configuration, source environments and private tooling are excluded. `-SkipRuntime` explicitly produces a runtime-free package; inference then requires a separately supplied approved runtime. The output directory must be project-contained and existing archives are never silently overwritten.

Rust dependency license files are collected from the locally resolved locked Cargo packages, not downloaded by the packaging script. Missing texts block publication. The presence of a license file is not a legal compatibility audit; retained notices must be reviewed before distribution.

## Native verification and retired migration tooling

Current delivery uses `scripts/hosted_ci.ps1` through manual GitHub-hosted Windows/Linux/macOS jobs. Builds and general suites do not run locally. The old migration CI runner, executed Python oracle and frozen source-environment restoration are retired at the owner's request; Git history retains their source without making it an ongoing test dependency.

Existing generated arithmetic/SDK/HTTP/geometry/CTC/tensor/digest fixtures remain unchanged. Rust tests execute their contracts; `tests/fixture_inventory.rs` checks immutable hashes and every mapped native target. Installed CLI/HTTP persistence, idempotency and interrupted-task recovery run through `examples/hosted_rollback.rs`, without Python or a claim of live cross-language rollback. The Windows installed start/stop/contender/credential-redaction test remains in `tests/installed_launchers.ps1`.

Historical migration evidence remains in prior validation reports. A new tree requires its own exact-source hosted result. Preserve models, private data, operational logs, shared tools and Git history; removing historical test execution is not permission to delete them.
