# Models

One selected model: first-party `PaddlePaddle/PP-OCRv6_small_rec_onnx`, Apache-2.0.
Revision: `b8f84f0b80c529de40b4fbb3544b84fa7233a513`.
Model SHA-256: `5435fd747c9e0efe15a96d0b378d5bd157e9492ed8fd80edf08f30d02fa24634`.
The packaged manifest records model, dictionary and authoritative sidecar URLs, sizes and checksums. Assets remain local; no large weights are in Git or distributions.

```bash
./bin/numera-cha.exe models fetch ppocr-v6-small
```

```bash
./bin/numera-cha.exe models verify ppocr-v6-small
```

The implementation uses BGR, height 48, bounded dynamic width, FP32 NCHW, right padding and CTC decoding. Pillow bilinear resizing is a versioned Numera preprocessing baseline; exact OpenCV resize parity is not asserted. Blank is 0; paired dictionary plus trailing space has 18710 entries, confirmed against actual output.

The owned ASCII smoke image was recognized as `NUMERA 123`. This is not distorted-CAPTCHA or multilingual accuracy. Upstream PaddleOCR issue 18364 reports possible non-ASCII corruption for the selected weights; multilingual promotion remains blocked pending real labeled validation.

Missing/corrupt assets never download implicitly during recognition. Fetch is explicit, checksum-bound, atomic and rejects HTML/error content and unreviewed redirects. Concurrent acquisition returns `model_busy`. The download lease is an OS-held file lock: a stale marker is not an active owner, a live slow download is never evicted just because of a timeout, and process exit releases ownership. No blind lock-file deletion is used.

Rejected initial candidate: ddddocr source is MIT but named bundled weights lacked the directive's separate explicit provenance/license evidence, and reviewed APIs disagreed with README examples. No GPL/AGPL solver code was copied.
