"""Generate the honest requested coverage inventory, never support by name."""

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FAMILIES = [
    (
        "C01",
        "P0",
        "Google reCAPTCHA",
        "v2/Enterprise-checkbox",
        "checkbox,invisible-v2,static-3x3,dynamic-3x3,region-4x4",
    ),
    (
        "C02",
        "P0",
        "Google reCAPTCHA",
        "v3/Enterprise-score",
        "risk-assessment,expected-action-hostname",
    ),
    ("C03", "P0", "Cloudflare Turnstile", "current", "managed,non-interactive,invisible"),
    ("C04", "P0", "Cloudflare challenges", "current", "interstitial,javascript,hard-block"),
    (
        "C05",
        "P0",
        "hCaptcha",
        "current/Enterprise",
        "image-selection,point,box,drag,animated,video",
    ),
    (
        "C06",
        "P0",
        "Arkose FunCaptcha",
        "current",
        "rotation,object-match,count,coordinates,seat,train",
    ),
    ("C07", "P1", "GeeTest", "v3/v4", "visual,slider,ordered-click,audio"),
    ("C08", "P1", "AWS WAF", "current", "visible-captcha,non-interactive-challenge"),
    ("C09", "P1", "Tencent TCaptcha", "current", "one-click,slider,graphic,audio"),
    ("C10", "P1", "Alibaba CAPTCHA", "unverified", "product-version,slider,graphic"),
    ("C11", "P1", "NetEase Yidun", "unverified", "slider,click,unobserved"),
    ("C12", "P1", "Yandex SmartCaptcha", "current", "visible,invisible"),
    ("C13", "P2", "MTCaptcha", "unverified", "visual,audio"),
    ("C14", "P2", "Capy", "unverified", "matching,puzzle"),
    ("C15", "P2", "Lemin Cropped", "unverified", "cropped-puzzle"),
    ("C16", "P2", "KeyCAPTCHA", "legacy-unverified", "puzzle,availability"),
    ("C17", "P2", "Friendly Captcha", "v1/v2", "v1-proof-of-work,v2-lifecycle"),
    ("C18", "P2", "ALTCHA", "current", "owner-proof-of-work"),
    ("C19", "P2", "Prosopo Procaptcha", "unverified", "visual,non-visual"),
    ("C20", "P2", "Cutcaptcha", "unverified", "challenge-contract"),
    ("C21", "P2", "VK CAPTCHA", "unverified", "observed-visual-subtypes"),
    ("C22", "P2", "DataDome", "unverified", "captcha,access-denial"),
    ("C23", "P2", "HUMAN PerimeterX", "unverified", "challenge,behavioral-refusal"),
    (
        "C24",
        "P0",
        "Generic text",
        "v1",
        "distorted-text,case-sensitive-text,amazon-style-text,arithmetic",
    ),
    ("C25", "P1", "Generic audio", "unverified", "speech,digits,sound-reasoning"),
    (
        "C26",
        "P1",
        "Generic spatial",
        "v1",
        "binary-selection,points,ordered-click,boxes,slider,rotation,matching,drag,temporal",
    ),
]


def main() -> None:
    rows = []
    for catalog, priority, provider, version, subtypes in FAMILIES:
        for subtype in subtypes.split(","):
            arithmetic = catalog == "C24" and subtype == "arithmetic"
            rows.append(
                {
                    "catalog_id": catalog,
                    "priority": priority,
                    "provider": provider,
                    "version": version,
                    "subtype": subtype,
                    "modality": "text"
                    if catalog == "C24"
                    else "audio"
                    if catalog == "C25"
                    else "mixed",
                    "integration_mode": "local_asset"
                    if catalog >= "C24"
                    else "owner_controlled_only",
                    "mode": "local_classical"
                    if arithmetic
                    else "browser_protocol"
                    if catalog in {"C02", "C03", "C04", "C17", "C18"}
                    else "native_local_ai",
                    "implementation_state": "implemented_unverified" if arithmetic else "blocked",
                    "availability": "ready"
                    if arithmetic
                    else "disabled_by_policy"
                    if catalog <= "C23"
                    else "model_missing",
                    "model_revision": None,
                    "last_validated_date": None,
                    "test_environment": "owned-unit-fixture" if arithmetic else None,
                    "evidence": ["tests/test_sdk.py"] if arithmetic else [],
                    "metrics": [],
                    "decision": "Restricted local integer grammar."
                    if arithmetic
                    else "No native support claimed; do not implement a bypass "
                    "from a registry entry.",
                    "blockers": []
                    if arithmetic
                    else [
                        "No independently validated licensed subtype model/dataset and "
                        "owner-side fixture.",
                        "Live third-party execution is disabled; no owner-approved "
                        "origin configured.",
                    ],
                    "next_action": "Run public arithmetic regression tests."
                    if arithmetic
                    else "Supply an owned licensed labeled fixture and documented integration "
                    "contract; evaluate locally before promotion.",
                }
            )
    rows.append(
        {
            "catalog_id": "C24",
            "priority": "P0",
            "provider": "Generic text",
            "version": "PP-OCRv6-small",
            "subtype": "generic_text_line",
            "modality": "text",
            "integration_mode": "local_asset",
            "mode": "native_local_ai",
            "implementation_state": "fixture_verified",
            "availability": "model_missing",
            "model_id": "ppocr-v6-small",
            "model_revision": "b8f84f0b80c529de40b4fbb3544b84fa7233a513",
            "last_validated_date": "2026-10-01T23:35:11Z",
            "test_environment": "owned-ascii-smoke",
            "evidence": ["tests/test_gpu.py", "reports/VALIDATION.md"],
            "metrics": [],
            "decision": "Generic local OCR; not distorted CAPTCHA accuracy.",
            "blockers": [
                "Real CAPTCHA held-out dataset unavailable.",
                "Non-ASCII output issue remains unvalidated.",
            ],
            "next_action": "Execute immutable weights on owned ASCII sample; report device and "
            "limited evidence separately.",
        }
    )
    document = {
        "schema_version": 1,
        "release": "0.1.0",
        "scope": "Local perception and owner-controlled testing; no third-party protection bypass.",
        "capabilities": rows,
    }
    content = json.dumps(document, separators=(",", ":"), ensure_ascii=False) + "\n"
    for path in (
        ROOT / "reports/COVERAGE.json",
        ROOT / "src/numera_cha_solver/resources/capabilities.json",
    ):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")


if __name__ == "__main__":
    import logging

    from numera_cha_solver.logging import close, initialize

    initialize(ROOT / "logs", "build-coverage")
    try:
        main()
        logging.getLogger("numera").info("coverage inventory generated catalog_families=26")
    finally:
        close()
