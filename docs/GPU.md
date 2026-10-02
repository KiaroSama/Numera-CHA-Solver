# GPU execution

Default neural device: CUDA. CPU preprocessing, parsing and browser fixture orchestration are normal and distinct from neural execution. Explicit CPU inference is selected with `--device cpu` or `NUMERA_DEVICE=cpu`; it is logged as a warning.

```bash
uv run numera-cha doctor --smoke tests/fixtures/ocr/ascii-smoke.png
```

The initial Windows machine: NVIDIA RTX 4070 Ti, 12282 MiB, driver 616.92; ONNX Runtime GPU 1.30.0. A real owned ASCII smoke profile recorded 187 CUDA nodes and 2 CPU shape/support nodes, with no expensive Conv/MatMul/Gemm/Attention operator on CPU. Provider presence alone is not used as proof.

CUDA 13/cuDNN 9 runtime DLLs must be discoverable. Set `NUMERA_DLL_DIRECTORY` to a compatible local DLL directory, or configure `data/runtime-config.json` with `dll_directory`. Never hard-code that machine path into application source. Existing compatible PyTorch runtime DLLs were used on the tested machine; importing/installing PyTorch is not a project dependency.

Only one `onnxruntime` distribution owns the namespace. `gpu` and `cpu` extras are mutually exclusive; synchronize one, not both. Strict GPU initialization/fallback/profile failure produces availability errors instead of silently returning CPU results. The arena limit is 1 GiB, not a guarantee of total GPU memory usage. One model worker bounds concurrency; peak VRAM benchmarking and other hardware remain unmeasured.
