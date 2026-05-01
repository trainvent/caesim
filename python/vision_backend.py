import json
import os
import sys
from typing import Any, Dict, List, Optional, Tuple


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
    want_web = "WEB_DETECTION" in features
    want_properties = "IMAGE_PROPERTIES" in features

    out = analyze_images(
        client=client,
        vision=vision,
        paths=images,
        want_labels=want_labels,
        want_safe=want_safe,
        want_text=want_text,
        want_web=want_web,
        want_properties=want_properties,
    )

    signal_count = sum(1 for result in out if has_any_signal(result))
    sys.stderr.write(
        f"Google Vision returned analysis signals for {signal_count}/{len(out)} image(s)\n"
    )
    error_samples = [
        error for result in out for error in result.get("errors", [])[:1]
    ][:3]
    if error_samples:
        sys.stderr.write("Google Vision sample errors:\n")
        for error in error_samples:
            sys.stderr.write(f"- {error}\n")
    return write_response(out)


def analyze_images(
    *,
    client: Any,
    vision: Any,
    paths: List[str],
    want_labels: bool,
    want_safe: bool,
    want_text: bool,
    want_web: bool,
    want_properties: bool,
) -> List[Dict[str, Any]]:
    features = []
    if want_labels:
        features.append(vision.Feature(type_=vision.Feature.Type.LABEL_DETECTION))
    if want_safe:
        features.append(vision.Feature(type_=vision.Feature.Type.SAFE_SEARCH_DETECTION))
    if want_text:
        features.append(vision.Feature(type_=vision.Feature.Type.TEXT_DETECTION))
    if want_web:
        features.append(vision.Feature(type_=vision.Feature.Type.WEB_DETECTION))
    if want_properties:
        features.append(vision.Feature(type_=vision.Feature.Type.IMAGE_PROPERTIES))

    if not features:
        return [empty_result(path) for path in paths]

    out: List[Dict[str, Any]] = []
    chunk_size = 8
    for start in range(0, len(paths), chunk_size):
        chunk_paths = paths[start : start + chunk_size]
        requests = []
        request_paths = []
        for path in chunk_paths:
            if not os.path.exists(path):
                out.append(empty_result(path))
                continue
            if not is_google_vision_supported(path):
                result = empty_result(path)
                result["errors"].append("unsupported_by_google_vision")
                out.append(result)
                continue
            try:
                with open(path, "rb") as f:
                    content = f.read()
            except Exception as e:
                result = empty_result(path)
                result["errors"].append(f"read: {e}")
                out.append(result)
                continue

            requests.append(
                vision.AnnotateImageRequest(
                    image=vision.Image(content=content),
                    features=features,
                )
            )
            request_paths.append(path)

        if not requests:
            continue

        try:
            response = client.batch_annotate_images(
                requests=requests,
                retry=None,
                timeout=20,
            )
        except Exception as e:
            for path in request_paths:
                result = empty_result(path)
                result["errors"].append(f"batch_annotate_images: {e}")
                out.append(result)
            continue

        for path, annotation in zip(request_paths, response.responses):
            out.append(
                response_to_result(
                    vision=vision,
                    path=path,
                    annotation=annotation,
                    want_labels=want_labels,
                    want_safe=want_safe,
                    want_text=want_text,
                    want_web=want_web,
                    want_properties=want_properties,
                )
            )

    return out


def response_to_result(
    *,
    vision: Any,
    path: str,
    annotation: Any,
    want_labels: bool,
    want_safe: bool,
    want_text: bool,
    want_web: bool,
    want_properties: bool,
) -> Dict[str, Any]:
    errors: List[str] = []
    if getattr(annotation, "error", None) and annotation.error.message:
        errors.append(f"annotate: {annotation.error.message}")

    labels = (
        [a.description for a in (annotation.label_annotations or []) if a.description]
        if want_labels
        else []
    )

    safe_search: Optional[Dict[str, str]] = None
    if want_safe and annotation.safe_search_annotation is not None:
        ss = annotation.safe_search_annotation
        safe_search = {
            "adult": likelihood_name(vision, ss.adult),
            "violence": likelihood_name(vision, ss.violence),
            "racy": likelihood_name(vision, ss.racy),
            "medical": likelihood_name(vision, ss.medical),
            "spoof": likelihood_name(vision, ss.spoof),
        }

    text: Optional[str] = None
    if want_text:
        annotations = annotation.text_annotations or []
        if annotations:
            text = annotations[0].description

    web_full_matches: List[str] = []
    web_partial_matches: List[str] = []
    web_best_guess_labels: List[str] = []
    if want_web and annotation.web_detection is not None:
        web = annotation.web_detection
        web_full_matches = [
            img.url for img in (web.full_matching_images or []) if img.url
        ]
        web_partial_matches = [
            img.url for img in (web.partial_matching_images or []) if img.url
        ]
        web_best_guess_labels = [
            label.label for label in (web.best_guess_labels or []) if label.label
        ]

    dominant_colors: List[str] = []
    if want_properties and annotation.image_properties_annotation is not None:
        props = annotation.image_properties_annotation
        colors = props.dominant_colors.colors if props.dominant_colors else []
        dominant_colors = [color_bucket(c.color, c.score) for c in colors[:6]]

    return {
        "path": path,
        "labels": labels,
        "safe_search": safe_search,
        "text": text,
        "web_full_matches": web_full_matches,
        "web_partial_matches": web_partial_matches,
        "web_best_guess_labels": web_best_guess_labels,
        "dominant_colors": dominant_colors,
        "errors": errors,
    }


def analyze_image(
    *,
    client: Any,
    vision: Any,
    path: str,
    want_labels: bool,
    want_safe: bool,
    want_text: bool,
    want_web: bool,
    want_properties: bool,
) -> Dict[str, Any]:
    return analyze_images(
        client=client,
        vision=vision,
        paths=[path],
        want_labels=want_labels,
        want_safe=want_safe,
        want_text=want_text,
        want_web=want_web,
        want_properties=want_properties,
    )[0]


def color_bucket(color: Any, score: float) -> str:
    rgb: Tuple[int, int, int] = (
        bucket_color_component(getattr(color, "red", 0)),
        bucket_color_component(getattr(color, "green", 0)),
        bucket_color_component(getattr(color, "blue", 0)),
    )
    score_bucket = round(float(score), 2)
    return f"{rgb[0]:03d},{rgb[1]:03d},{rgb[2]:03d}:{score_bucket:.2f}"


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
    return {
        "path": path,
        "labels": [],
        "safe_search": None,
        "text": None,
        "web_full_matches": [],
        "web_partial_matches": [],
        "web_best_guess_labels": [],
        "dominant_colors": [],
        "errors": [],
    }


def is_google_vision_supported(path: str) -> bool:
    _, ext = os.path.splitext(path.lower())
    return ext in {".jpg", ".jpeg", ".png", ".webp", ".gif", ".bmp", ".tif", ".tiff"}


def has_any_signal(result: Dict[str, Any]) -> bool:
    return bool(
        result.get("labels")
        or result.get("safe_search")
        or result.get("text")
        or result.get("web_full_matches")
        or result.get("web_partial_matches")
        or result.get("web_best_guess_labels")
        or result.get("dominant_colors")
    )


def bucket_color_component(value: int) -> int:
    value = max(0, min(255, int(value)))
    return round(value / 32) * 32


def write_response(results: List[Dict[str, Any]]) -> int:
    sys.stdout.write(json.dumps({"results": results}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
