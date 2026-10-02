# Upstream review

Checked2026-10-01UTC. Research is source evidence, not a claim every referenced repository was executed.

| Component | Reviewed identity | Decision / license | Execution |
| --- | --- | --- | --- |
| PaddlePaddle PP-OCRv6_small_rec_onnx | b8f84f0b80c529de40b4fbb3544b84fa7233a513 | Apache2weights, immutableSHA256; generic text-line candidate | Actual ownedASCIIGPU smoke passed |
| PaddleOCR dictionary | dab3fe35379033fdcb2d0e9572fac0b36c9a9ebf | Apache2, pairedUTF8dictionary18710CTCentries | Outputclassdimension checked during actual inference |
| ONNX Runtime |1.30.0 / f2c39fe2f838cf35ce7da92824f5a5e3ee6e88a7 | MIT, explicitCUDAprovider/profile, one namespaceowner | CUDA187nodes observed |
| ddddocr |1.6.1 / c40f56f95412e10bcb9bd0bd24411e92f896d238 | MITsource, namedbundledweightlicenseunresolved/API-docsdrift | Not selected or executed |
| RapidOCR |9936171fa42eb4ca4d900941c827c80f6847535b | Apache2source/modellicense reference; convertedweightsdiffer | No code copied/executed |
| Turnstile official demo |e8dbbd5d3be28a8ab0d9b3e1fc27d190f08bff59 | OwnerSiteverifycontractreference; oldwrangler2/no tests | Not copied; networkintegration unverified |
| GitHub runner |2.337.0, archiveSHA2561150692afa94e71f872017e254ea55b6eece1eece3fe7e3a6d4c93d0a1b85cfc | MIT; dedicatedmanualprivateWindowsrunner | Registeredoffline, no service/startup |

Sources: official ONNXRuntime/Paddle model cards and source, RapidOCRmodel notices and tests, Cloudflare docs and official demo, current GitHubrunner source/release. Full original solver shortlist is not licensed/executed by association; unselected families remain blocked. No GPL/AGPL solver implementation was relicensed as proprietary code.
