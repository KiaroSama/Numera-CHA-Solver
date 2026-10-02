# Benchmarks

No held-out CAPTCHA benchmark exists yet. User supplied no labeled licensed dataset.
One owned synthetic ASCII smoke was recognized correctly; its confidence is not a probability of end-to-end success. No universal accuracy or latency claim is made.

The benchmark command consumes a local licensed UTF-8 JSON object, bounded to 1 MiB and 1-1000 samples. Every sample requires a unique nonempty string session identity, a string expected label, and a relative path restricted to the manifest directory. The entire manifest is validated before inference; malformed/scalar documents and invalid samples return sanitized errors. Unreadable files count as failed samples rather than disappearing from the report. Failures/abstentions remain in the denominator. It reports exact accuracy, Wilson95%interval and p50/p95 observed latency, model/device and limited-evidence flag. Training/calibration/test split verification requires a real supplied dataset; near-duplicate detection is not implemented.

Promotion thresholds were registered in `benchmarks/gates.yaml` before held-out results. No capability has met those thresholds. Cold/warm performance, peak VRAM/RAM, repeated-attempt accuracy, live acceptance and external costs are unmeasured.
