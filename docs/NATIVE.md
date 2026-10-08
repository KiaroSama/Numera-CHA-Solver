# Native Windows operation

The replacement application is a Rust executable and library. The existing dashboard remains HTML/CSS/JavaScript. Python is not an application prerequisite. Development/CI verification uses a disposable frozen test-only environment and a temporary Git snapshot of the historic source; neither is a second installed application version.

## Intentional Python SDK retirement

Version 0.2.0 exposes a Rust crate only. Embedded consumers must install the actual `numera-cha.exe` beside their executable or in the configured root's `bin`, or set `NUMERA_WORKER_EXECUTABLE` to that approved binary. The SDK never assumes an arbitrary embedding executable implements the private worker protocol. The owner explicitly approved removal of Python imports and the synchronous running-event-loop wrapper; CLI/HTTP contracts and the static web dashboard are preserved. Temporary Python oracle/browser tools are verification tooling, not a production compatibility layer.

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

For an extracted native package, `setup.ps1 -Yes` checks the included executable/runtime without invoking Cargo or Python. A package does not include source for rebuilding; replace it with a newly inspected package while preserving data/models/configuration. Do not overlay an active executable.

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

## Migration verification and retirement

```powershell
./scripts/native_ci.ps1 -Gpu -Browser
```

This command is for the integrator's final manually dispatched Windows CI, not an extra local suite. The existing `CI` workflow now dispatches native gates and uses only `workflow_dispatch` and the existing dedicated Windows GPU runner. No service/startup registration or automatic push/PR execution is added. The workflow must exist on the default branch before GitHub permits dispatch.

Required tiers are native parity, preprocessing, adapters, lifecycle, GPU and CLI tests; source-generated fixture drift, a headless native-dashboard harness, an inspected package, installed executable and launcher checks also run. Build concurrency is two; test threads are one because global log/environment state and GPU execution are exclusive. Missing tiers or pending source-test mappings fail closed. Independent stage failures are collected before reporting. The source oracle compares immutable arithmetic/SDK/tensor fixtures and source test identities/hashes; it never regenerates expected answers from Rust.

The pre-retirement migration passed all 15 stages on `4a5e60b` in [CI 37777670392](https://github.com/KiaroSama/Numera-CHA-Solver/actions/runs/37777670392). The previous application source and source-only tests/helpers are retired after the copied-state rollback rehearsal. Native fixtures remain maintained. `scripts/source_oracle.ps1` extracts the exact historic source revision into temporary project state, restores its frozen test environment when required, runs generation/rollback and removes the snapshot. Its disposable environment is not deployed and can be recreated; final cleanup removes it. Post-retirement changes require their own affected verification; prior CI is not automatically evidence for a new tree. Never delete model assets, user data, operational logs, shared Rust/Python/CUDA tools or history as part of package cleanup. No runtime, packaging, browser, device or rollback success should be claimed merely because this procedure is documented.
