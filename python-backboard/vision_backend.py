import json
import os
import sys
from typing import Any, Dict, List, Optional


def main() -> int:
    payload = sys.stdin.buffer.read()
    if not payload:
        sys.stderr.write("No input received on stdin\n")
        return 2

    req = json.loads(payload.decode("utf-8"))
    images: List[str] = req.get("images", [])
    features: List[str] = req.get("features", [])

    # If google-cloud-vision isn't installed or credentials aren't configured,
    # return an empty-but-well-formed response so the Rust side can proceed.
    try:
        from google.cloud import vision  # type: ignore
    except Exception as e:
        sys.stderr.write(
            f"google-cloud-vision not available ({e}); returning empty results\n"
        )
        return write_response([empty_result(p) for p in images])

    try:
        client = vision.ImageAnnotatorClient()
    except Exception as e:
        sys.stderr.write(
            f"Vision client init failed (credentials?): {e}; returning empty results\n"
        )
        return write_response([empty_result(p) for p in images])

    want_labels = "LABEL_DETECTION" in features
    want_safe = "SAFE_SEARCH_DETECTION" in features
    want_text = "TEXT_DETECTION" in features or "DOCUMENT_TEXT_DETECTION" in features

    out = []
    for path in images:
        out.append(
            analyze_image(
                client=client,
                vision=vision,
                path=path,
                want_labels=want_labels,
                want_safe=want_safe,
                want_text=want_text,
            )
        )

    return write_response(out)


def analyze_image(
    *,
    client: Any,
    vision: Any,
    path: str,
    want_labels: bool,
    want_safe: bool,
    want_text: bool,
) -> Dict[str, Any]:
    if not os.path.exists(path):
        return empty_result(path)

    try:
        with open(path, "rb") as f:
            content = f.read()
    except Exception:
        return empty_result(path)

    image = vision.Image(content=content)

    labels: List[str] = []
    safe_search: Optional[Dict[str, str]] = None
    text: Optional[str] = None

    if want_labels:
        try:
            resp = client.label_detection(image=image)
            labels = [a.description for a in (resp.label_annotations or []) if a.description]
        except Exception:
            labels = []

    if want_safe:
        try:
            resp = client.safe_search_detection(image=image)
            ss = resp.safe_search_annotation
            if ss is not None:
                safe_search = {
                    "adult": likelihood_name(vision, ss.adult),
                    "violence": likelihood_name(vision, ss.violence),
                    "racy": likelihood_name(vision, ss.racy),
                    "medical": likelihood_name(vision, ss.medical),
                    "spoof": likelihood_name(vision, ss.spoof),
                }
        except Exception:
            safe_search = None

    if want_text:
        try:
            resp = client.text_detection(image=image)
            annotations = resp.text_annotations or []
            if annotations:
                text = annotations[0].description
        except Exception:
            text = None

    return {"path": path, "labels": labels, "safe_search": safe_search, "text": text}


def likelihood_name(vision: Any, value: int) -> str:
    # vision.Likelihood(value).name is available in newer clients; keep a fallback.
    try:
        return vision.Likelihood(value).name
    except Exception:
        mapping = {
            0: "UNKNOWN",
            1: "VERY_UNLIKELY",
            2: "UNLIKELY",
            3: "POSSIBLE",
            4: "LIKELY",
            5: "VERY_LIKELY",
        }
        return mapping.get(int(value), "UNKNOWN")


def empty_result(path: str) -> Dict[str, Any]:
    return {"path": path, "labels": [], "safe_search": None, "text": None}


def write_response(results: List[Dict[str, Any]]) -> int:
    sys.stdout.write(json.dumps({"results": results}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

