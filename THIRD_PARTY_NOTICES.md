# Third-party notices

Numera's original source uses the custom Noncommercial Network Share-Alike License1.0 (see LICENSE). It is source-available, not GNU AGPL or OSI-approved open source. Third-party grants are independent.
No upstream CAPTCHA solver source or unlicensed weights are redistributed.

- PaddlePaddle PP-OCRv6_small_rec_onnx weights: Apache-2.0, PaddlePaddle/Baidu. Downloaded locally only, not included in Git or packages. Model revision b8f84f0b80c529de40b4fbb3544b84fa7233a513; source https://huggingface.co/PaddlePaddle/PP-OCRv6_small_rec_onnx . Dictionary from PaddleOCR dab3fe35379033fdcb2d0e9572fac0b36c9a9ebf under Apache-2.0. Preserve upstream license when redistributing model assets separately.
- ONNX Runtime: MIT, Microsoft; approved native DLLs may be staged and packaged with their original LICENSE and ThirdPartyNotices.txt. Binding `ort` is MIT OR Apache-2.0, exact approved 2.0.0-rc.13; automatic binary downloads are disabled.
- Rust dependencies retain the grants recorded in Cargo metadata. Native distribution collects actual license/notice files for the resolved native platform dependency graph and blocks missing texts; `licenses/dependencies.json` names each included grant. No third-party license is replaced with Numera's custom license.
- Pillow bilinear compatibility is independently implemented against its documented/released resampling algorithm. Pillow is MIT-CMU; source-oracle tooling uses Pillow/NumPy, not the native application runtime. No wholesale Pillow source was copied.
- Historical source-oracle tooling: FastAPI, Pydantic and HTTPX MIT; Uvicorn BSD-3-Clause; NumPy BSD-3-Clause; Pillow MIT-CMU. These are not native application dependencies.
- Playwright: Apache-2.0; optional owned-browser tests, no stealth or protection evasion.
- Official Rust MCP SDK rmcp: Apache-2.0 declaration with legacy MIT grants retained in the complete upstream license; Tauri: MIT OR Apache-2.0. Actual resolved dependency license texts are included in native distributions.
- Missing published crate texts are supplied only from the exact version/revision/hash-reviewed catalog in scripts/license_fallbacks.json. Original bytes and copyright notices are preserved in resources/licenses/upstream; dependency updates without a reviewed matching entry fail packaging.
- MPL-2.0 dependencies cssparser, cssparser-macros, dtoa-short, option-ext and selectors retain their own terms. Native packages include their unchanged, checksum-pinned preferred source archives and instructions in source/third-party/<crate>-<version>/SOURCE-NOTICE.txt. Numera's noncommercial restriction does not limit recipients' MPL rights to that covered source.
- macOS objc2-family notices preserve the upstream license-by-reference declarations, selected full MIT/Apache texts and inherited copyright. Their upstream Apple SDK/Xcode qualification is included unchanged; this packaging record is not a legal-clearance opinion.
- GitHub-hosted build tooling is not an application dependency. No self-hosted runner is shipped.

Owned synthetic smoke fixtures are original test artwork, not collected provider challenges.
Generic OCR smoke results do not measure real CAPTCHA accuracy.
