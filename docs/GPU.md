# GPU execution

CLI/SDK default neural device: CUDA on Windows/Linux, CPU on macOS. Packaged desktop defaults to its bundled CPU runtime unless the owner supplies an explicit device/runtime. CPU preprocessing, parsing and browser fixture orchestration are normal and distinct from neural execution. CPU selection uses `--device cpu` or `NUMERA_DEVICE=cpu` and is explicitly logged; choosing CUDA remains strict, with no silent inference fallback.

GitHub-hosted 0.3.0 jobs acquire reviewed ONNX Runtime1.31.0 CPU archives for each native platform and run the immutable ASCII smoke. Those jobs cannot establish CUDA execution. Their results remain pending until exact-revision CI; the Windows hardware evidence below is historical1.30.0, not a claim about the new runtime or other hardware.

```bash
./bin/numera-cha.exe doctor --smoke tests/fixtures/ocr/ascii-smoke.png
```

The initial Windows machine: NVIDIA RTX 4070 Ti, 12282 MiB, driver 616.92; ONNX Runtime GPU 1.30.0. A real owned ASCII smoke profile recorded 187 CUDA nodes and 2 CPU shape/support nodes, with no expensive Conv/MatMul/Gemm/Attention operator on CPU. Provider presence alone is not used as proof.

CUDA 13/cuDNN 9 runtime DLLs must be discoverable. Set `NUMERA_DLL_DIRECTORY` to a compatible local DLL directory, or configure `data/runtime-config.json` with `dll_directory`. Never hard-code that machine path into application source. Existing compatible PyTorch runtime DLLs were used on the tested machine; importing/installing PyTorch is not a project dependency.

The native application dynamically loads an explicitly selected ONNX Runtime DLL through exact `ort = 2.0.0-rc.13`. Automatic runtime downloads are disabled. GPU and CPU use the same native build; CPU selection is explicit, not a second Python distribution. Strict GPU initialization/fallback/profile failure produces availability errors instead of silently returning CPU results. The arena limit is 1 GiB, not a guarantee of total GPU memory usage. One model worker bounds concurrency; peak VRAM benchmarking and other hardware remain unmeasured.
