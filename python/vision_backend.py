import base64
import json
import os
import sys
import uuid
import urllib.error
import urllib.request
from datetime import datetime, timezone
from typing import Any, Dict, List, Optional, Tuple


ImageInput = Dict[str, Any]
_VISION_CLIENT: Any = None
_STORAGE_CLIENT: Any = None


def main() -> int:
    payload = sys.stdin.buffer.read()
    if not payload:
        sys.stderr.write("No input received on stdin\n")
        return 2

    req = json.loads(payload.decode("utf-8"))
    image_inputs = request_image_inputs(req)
    features: List[str] = req.get("features", [])

    # If google-cloud-vision isn't installed or credentials aren't configured,
    # return an empty-but-well-formed response so the Rust side can proceed.
    try:
        from google.cloud import vision  # type: ignore
    except Exception as e:
        sys.stderr.write(
            f"google-cloud-vision not available ({e}); returning empty results\n"
        )
        return write_response([empty_result(p["path"]) for p in image_inputs])

    try:
        transport = os.environ.get("CAESIM_VISION_TRANSPORT")
        if transport:
            client = vision.ImageAnnotatorClient(transport=transport)
        else:
            client = vision.ImageAnnotatorClient()
    except Exception as e:
        sys.stderr.write(
            f"Vision client init failed (credentials?): {e}; returning empty results\n"
        )
        return write_response([empty_result(p["path"]) for p in image_inputs])

    want_labels = "LABEL_DETECTION" in features
    want_safe = "SAFE_SEARCH_DETECTION" in features
    want_text = "TEXT_DETECTION" in features or "DOCUMENT_TEXT_DETECTION" in features
    want_web = "WEB_DETECTION" in features
    want_properties = "IMAGE_PROPERTIES" in features

    out = analyze_image_inputs(
        client=client,
        vision=vision,
        image_inputs=image_inputs,
        want_labels=want_labels,
        want_safe=want_safe,
        want_text=want_text,
        want_web=want_web,
        want_properties=want_properties,
    )

    signal_count = sum(1 for result in out if has_any_signal(result))
    sys.stderr.write(
        f"Google Vision returned analysis signals for {signal_count}/{len(out)} images\n"
    )
    error_samples = [
        error for result in out for error in result.get("errors", [])[:1]
    ][:3]
    if error_samples:
        sys.stderr.write("Google Vision sample errors:\n")
        for error in error_samples:
            sys.stderr.write(f"- {error}\n")
    return write_response(out)


def request_image_inputs(req: Dict[str, Any]) -> List[ImageInput]:
    inputs: List[ImageInput] = []
    for image in req.get("images", []):
        if isinstance(image, str):
            inputs.append({"path": image, "content": None})
            continue
        if isinstance(image, dict):
            path = str(image.get("path") or "")
            encoded = image.get("content_base64")
            content = None
            if isinstance(encoded, str) and encoded:
                content = base64.b64decode(encoded)
            inputs.append({"path": path, "content": content})
    return inputs


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
    return analyze_image_inputs(
        client=client,
        vision=vision,
        image_inputs=[{"path": path, "content": None} for path in paths],
        want_labels=want_labels,
        want_safe=want_safe,
        want_text=want_text,
        want_web=want_web,
        want_properties=want_properties,
    )


def analyze_image_inputs(
    *,
    client: Any,
    vision: Any,
    image_inputs: List[ImageInput],
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
        return [empty_result(image["path"]) for image in image_inputs]

    out: List[Dict[str, Any]] = []
    chunk_size = int(os.environ.get("CAESIM_VISION_CHUNK_SIZE", "8"))
    timeout = float(os.environ.get("CAESIM_VISION_TIMEOUT", "20"))
    chunk_size = max(1, chunk_size)
    for start in range(0, len(image_inputs), chunk_size):
        chunk_inputs = image_inputs[start : start + chunk_size]
        requests = []
        request_paths = []
        for image in chunk_inputs:
            path = image["path"]
            content = image.get("content")
            if content is None and not os.path.exists(path):
                out.append(empty_result(path))
                continue
            if not is_google_vision_supported(path):
                result = empty_result(path)
                result["errors"].append("unsupported_by_google_vision")
                out.append(result)
                continue
            if content is None:
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
            end = min(start + len(chunk_inputs), len(image_inputs))
            sys.stderr.write(
                f"Google Vision analyzing images {start + 1}-{end}/{len(image_inputs)} "
                f"({len(requests)} request(s), timeout={timeout:g}s)\n"
            )
            sys.stderr.flush()
            response = client.batch_annotate_images(
                requests=requests,
                retry=None,
                timeout=timeout,
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


def vision_http(request: Any) -> Tuple[str, int, Dict[str, str]]:
    try:
        path = getattr(request, "path", "") or ""
        method = getattr(request, "method", "POST")
        if path == "/v1/analyze-batch" and method == "POST":
            return start_async_batch(request)
        if path.startswith("/v1/status/") and method == "GET":
            return batch_status(request, path.rsplit("/", 1)[-1])

        req = request.get_json(silent=True) or {}
        try:
            authenticate_request(request)
        except AuthError as e:
            return json_response({"error": str(e)}, 401)
        image_inputs = request_image_inputs(req)
        features: List[str] = req.get("features", [])

        from google.cloud import vision  # type: ignore

        client = vision_client(vision)

        results = analyze_image_inputs(
            client=client,
            vision=vision,
            image_inputs=image_inputs,
            want_labels="LABEL_DETECTION" in features,
            want_safe="SAFE_SEARCH_DETECTION" in features,
            want_text="TEXT_DETECTION" in features
            or "DOCUMENT_TEXT_DETECTION" in features,
            want_web="WEB_DETECTION" in features,
            want_properties="IMAGE_PROPERTIES" in features,
        )
        return (
            json.dumps({"results": results}),
            200,
            {"Content-Type": "application/json"},
        )
    except Exception as e:
        body = json.dumps({"error": str(e), "results": []})
        return body, 500, {"Content-Type": "application/json"}


def start_async_batch(request: Any) -> Tuple[str, int, Dict[str, str]]:
    try:
        user = authenticate_request(request)
    except AuthError as e:
        return json_response({"error": str(e)}, 401)

    req = request.get_json(silent=True) or {}
    customer_id = user.get("id") or request_customer_id(request, req)
    if not customer_id:
        return json_response({"error": "customer_id is required"}, 400)

    feature_names = req.get("features") or ["LABEL_DETECTION"]
    if not isinstance(feature_names, list) or not feature_names:
        return json_response({"error": "features must be a non-empty list"}, 400)

    source_uris = validated_source_uris(req)
    if not source_uris:
        return json_response(
            {"error": "provide image_uris or gcs_input_uri with supported GCS images"},
            400,
        )

    input_bucket_name = required_env("INPUT_BUCKET_NAME")
    output_bucket_name = required_env("OUTPUT_BUCKET_NAME")
    batch_id = str(req.get("batch_id") or uuid.uuid4())
    clean_customer = clean_path_part(customer_id)
    clean_batch = clean_path_part(batch_id)

    storage = storage_client()
    processing_uris = copy_to_processing_bucket(
        storage=storage,
        source_uris=source_uris,
        input_bucket_name=input_bucket_name,
        prefix=f"{clean_customer}/{clean_batch}",
    )

    from google.cloud import vision  # type: ignore

    client = vision_client(vision)
    features = vision_features(vision, feature_names)
    output_uri = f"gs://{output_bucket_name}/{clean_customer}/{clean_batch}/"
    requests = [
        vision.AsyncAnnotateImageRequest(
            image=vision.Image(
                source=vision.ImageSource(gcs_image_uri=processing_uri)
            ),
            features=features,
        )
        for processing_uri in processing_uris
    ]
    operation = client.async_batch_annotate_images(
        requests=requests,
        output_config=vision.OutputConfig(
            gcs_destination=vision.GcsDestination(uri=output_uri),
            batch_size=int(os.environ.get("CAESIM_VISION_OUTPUT_BATCH_SIZE", "50")),
        ),
    )

    operation_name = getattr(getattr(operation, "operation", operation), "name", "")
    manifest = {
        "batch_id": batch_id,
        "customer_id": customer_id,
        "operation_name": operation_name,
        "state": "submitted",
        "features": feature_names,
        "image_count": len(processing_uris),
        "input_prefix": f"gs://{input_bucket_name}/{clean_customer}/{clean_batch}/",
        "output_prefix": output_uri,
        "submitted_at": datetime.now(timezone.utc).isoformat(),
    }
    write_manifest(
        storage=storage,
        output_bucket_name=output_bucket_name,
        name=f"{clean_customer}/{clean_batch}/job.json",
        manifest=manifest,
    )
    return json_response(manifest, 202)


def batch_status(request: Any, batch_id: str) -> Tuple[str, int, Dict[str, str]]:
    try:
        user = authenticate_request(request)
    except AuthError as e:
        return json_response({"error": str(e)}, 401)

    customer_id = user.get("id")
    if not customer_id:
        customer_id = request.args.get("customer_id") if hasattr(request, "args") else None
    if not customer_id:
        return json_response({"error": "customer_id query parameter is required"}, 400)

    output_bucket_name = required_env("OUTPUT_BUCKET_NAME")
    clean_customer = clean_path_part(customer_id)
    clean_batch = clean_path_part(batch_id)
    prefix = f"{clean_customer}/{clean_batch}/"
    bucket = storage_client().bucket(output_bucket_name)
    blobs = list(bucket.list_blobs(prefix=prefix, max_results=100))
    json_outputs = [
        f"gs://{output_bucket_name}/{blob.name}"
        for blob in blobs
        if blob.name.endswith(".json") and not blob.name.endswith("/job.json")
    ]
    manifest_blob = bucket.blob(f"{prefix}job.json")
    manifest = {}
    if manifest_blob.exists():
        manifest = json.loads(manifest_blob.download_as_text())

    state = "complete" if json_outputs else "running"
    if not manifest and not json_outputs:
        state = "not_found"

    return json_response(
        {
            "batch_id": batch_id,
            "customer_id": customer_id,
            "state": state,
            "operation_name": manifest.get("operation_name"),
            "output_prefix": f"gs://{output_bucket_name}/{prefix}",
            "result_uris": json_outputs,
            "image_count": manifest.get("image_count"),
            "submitted_at": manifest.get("submitted_at"),
        },
        200 if state != "not_found" else 404,
    )


def proxy_auth_ok(request: Any) -> bool:
    expected = os.environ.get("CAESIM_VISION_PROXY_TOKEN")
    if not expected:
        return False
    auth_header = request.headers.get("Authorization", "")
    return auth_header == f"Bearer {expected}"


class AuthError(Exception):
    pass


def authenticate_request(request: Any) -> Dict[str, Any]:
    if proxy_auth_ok(request):
        return {}

    token = bearer_token(request)
    project_url = supabase_project_url()
    service_key = required_env("SERVICE_ROLE_KEY")
    url = f"{project_url}/auth/v1/user"
    auth_request = urllib.request.Request(
        url,
        headers={
            "apikey": service_key,
            "Authorization": f"Bearer {token}",
        },
        method="GET",
    )
    try:
        with urllib.request.urlopen(auth_request, timeout=10) as response:
            raw = response.read().decode("utf-8")
            return json.loads(raw)
    except urllib.error.HTTPError as e:
        detail = e.read().decode("utf-8", errors="replace")
        raise AuthError(f"auth lookup failed: {e.code} {detail}") from e
    except Exception as e:
        raise AuthError(f"auth lookup failed: {e}") from e


def bearer_token(request: Any) -> str:
    session_header = request.headers.get("X-Caesim-Session", "")
    if session_header.strip():
        return session_header.strip()

    auth_header = request.headers.get("Authorization", "")
    parts = auth_header.split(" ", 1)
    if len(parts) != 2 or parts[0].lower() != "bearer" or not parts[1].strip():
        raise AuthError("missing bearer token")
    return parts[1].strip()


def supabase_project_url() -> str:
    value = os.environ.get("SUPABASE_URL") or os.environ.get("PROJECT_URL")
    if not value:
        raise AuthError("missing SUPABASE_URL or PROJECT_URL")
    return value.rstrip("/")


def request_customer_id(request: Any, req: Dict[str, Any]) -> str:
    return str(req.get("customer_id") or request.headers.get("X-Customer-Id") or "")


def validated_source_uris(req: Dict[str, Any]) -> List[str]:
    uris = req.get("image_uris") or []
    if not isinstance(uris, list):
        uris = []
    out = [uri for uri in uris if isinstance(uri, str) and is_supported_gcs_uri(uri)]

    folder = req.get("gcs_input_uri")
    if isinstance(folder, str) and folder.startswith("gs://"):
        out.extend(list_supported_gcs_images(folder))

    return sorted(set(out))


def list_supported_gcs_images(folder_uri: str) -> List[str]:
    bucket_name, prefix = parse_gcs_uri(folder_uri)
    bucket = storage_client().bucket(bucket_name)
    return [
        f"gs://{bucket_name}/{blob.name}"
        for blob in bucket.list_blobs(prefix=prefix)
        if is_google_vision_supported(blob.name)
    ]


def copy_to_processing_bucket(
    *,
    storage: Any,
    source_uris: List[str],
    input_bucket_name: str,
    prefix: str,
) -> List[str]:
    input_bucket = storage.bucket(input_bucket_name)
    processing_uris = []
    for index, source_uri in enumerate(source_uris):
        source_bucket_name, source_name = parse_gcs_uri(source_uri)
        source_bucket = storage.bucket(source_bucket_name)
        source_blob = source_bucket.blob(source_name)
        ext = os.path.splitext(source_name)[1].lower()
        dest_name = f"{prefix}/{index:06d}{ext}"
        source_bucket.copy_blob(source_blob, input_bucket, dest_name)
        processing_uris.append(f"gs://{input_bucket_name}/{dest_name}")
    return processing_uris


def write_manifest(
    *, storage: Any, output_bucket_name: str, name: str, manifest: Dict[str, Any]
) -> None:
    blob = storage.bucket(output_bucket_name).blob(name)
    blob.upload_from_string(
        json.dumps(manifest, sort_keys=True),
        content_type="application/json",
    )


def vision_client(vision: Any) -> Any:
    global _VISION_CLIENT
    if _VISION_CLIENT is None:
        transport = os.environ.get("CAESIM_VISION_TRANSPORT")
        _VISION_CLIENT = (
            vision.ImageAnnotatorClient(transport=transport)
            if transport
            else vision.ImageAnnotatorClient()
        )
    return _VISION_CLIENT


def storage_client() -> Any:
    global _STORAGE_CLIENT
    if _STORAGE_CLIENT is None:
        from google.cloud import storage  # type: ignore

        _STORAGE_CLIENT = storage.Client()
    return _STORAGE_CLIENT


def vision_features(vision: Any, feature_names: List[str]) -> List[Any]:
    features = []
    for name in feature_names:
        try:
            feature_type = getattr(vision.Feature.Type, str(name))
        except AttributeError:
            raise ValueError(f"unsupported feature: {name}")
        features.append(vision.Feature(type_=feature_type))
    return features


def required_env(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        raise RuntimeError(f"{name} environment variable is required")
    return value


def parse_gcs_uri(uri: str) -> Tuple[str, str]:
    if not uri.startswith("gs://"):
        raise ValueError(f"not a GCS URI: {uri}")
    rest = uri[5:]
    bucket, _, name = rest.partition("/")
    if not bucket or not name:
        raise ValueError(f"GCS URI must include bucket and object/prefix: {uri}")
    return bucket, name


def is_supported_gcs_uri(uri: str) -> bool:
    if not uri.startswith("gs://"):
        return False
    try:
        _, name = parse_gcs_uri(uri)
    except ValueError:
        return False
    return is_google_vision_supported(name)


def clean_path_part(value: str) -> str:
    return "".join(c if c.isalnum() or c in ("-", "_") else "_" for c in value)[:120]


def json_response(body: Dict[str, Any], status: int) -> Tuple[str, int, Dict[str, str]]:
    return json.dumps(body), status, {"Content-Type": "application/json"}


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
