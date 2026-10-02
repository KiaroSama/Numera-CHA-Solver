# Third-party notices

Numera's original source is proprietary (see LICENSE). Third-party grants are independent.
No upstream CAPTCHA solver source or unlicensed weights are redistributed.

- PaddlePaddle PP-OCRv6_small_rec_onnx weights: Apache-2.0, PaddlePaddle/Baidu. Downloaded locally only, not included in Git or packages. Model revision b8f84f0b80c529de40b4fbb3544b84fa7233a513; source https://huggingface.co/PaddlePaddle/PP-OCRv6_small_rec_onnx . Dictionary from PaddleOCR dab3fe35379033fdcb2d0e9572fac0b36c9a9ebf under Apache-2.0. Preserve upstream license when redistributing model assets separately.
- ONNX Runtime: MIT, Microsoft; optional runtime installed from the lockfile, not vendored.
- FastAPI, Pydantic, HTTPX: MIT. Uvicorn: BSD-3-Clause. NumPy: BSD-3-Clause. Pillow: MIT-CMU.
- Playwright: Apache-2.0; optional owned-browser tests, no stealth or protection evasion.
- GitHub Actions runner: MIT, GitHub; project-local runtime installation, never included in packages.

Owned synthetic smoke fixtures are original test artwork, not collected provider challenges.
Generic OCR smoke results do not measure real CAPTCHA accuracy.
