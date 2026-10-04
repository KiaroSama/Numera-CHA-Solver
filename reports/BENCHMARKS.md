# Benchmarks

No held-out CAPTCHA benchmark exists yet. User supplied no labeled licensed dataset.
One owned synthetic ASCII smoke was recognized correctly; its confidence is not a probability of end-to-end success. No universal accuracy or latency claim is made.

The benchmark command consumes a local licensed UTF-8 JSON object, bounded to 1 MiB and 1-1000 samples. Every sample requires a unique nonempty string session identity, a string expected label, and a relative path restricted to the manifest directory. The entire manifest is validated before inference; malformed/scalar documents and invalid samples return sanitized errors. Unreadable files count as failed samples rather than disappearing from the report. Failures/abstentions remain in the denominator. It reports exact accuracy, Wilson95%interval and p50/p95 observed latency, model/device and limited-evidence flag. Training/calibration/test split verification requires a real supplied dataset; near-duplicate detection is not implemented.

Canonical path aliases and exact SHA-256 content duplicates are rejected before inference. Reports keep row count, readable unique-asset count and caller-declared source-group count separate; `source_id` optionally groups related crops/frames. Missing source grouping does not prove independence. `split_provenance` is recorded but not trusted as certification. Near-duplicate detection and training/calibration overlap remain unverified, so `limited_evidence` stays true until the registered promotion criteria are independently checked. Confidence intervals are descriptive and are not an independence certificate.

Promotion thresholds were registered in `benchmarks/gates.yaml` before held-out results. No capability has met those thresholds. Cold/warm performance, peak VRAM/RAM, repeated-attempt accuracy, live acceptance and external costs are unmeasured.
