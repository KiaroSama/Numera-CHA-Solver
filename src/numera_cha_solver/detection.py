"""Local classification hints, never provider support inferred from a hostname."""

from .core.contracts import Input


def detect(request: Input) -> dict[str, object]:
    modality = {
        "text": "arithmetic_text",
        "image": "still_image",
        "audio": "audio",
        "browser": "browser_session",
    }[request.kind]
    return {
        "provider": "unknown",
        "modality": modality,
        "subtype": request.subtype,
        "requires": ["provider/version/subtype and licensed observation"],
        "evidence": "Input metadata only; no live page detection was performed.",
    }
