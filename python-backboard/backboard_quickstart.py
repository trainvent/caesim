import argparse
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any, Dict, Optional


BASE_URL = "https://app.backboard.io/api"


def main() -> int:
    load_dotenv(Path(".env"))

    parser = argparse.ArgumentParser(
        description="Run the Backboard quickstart against the Caesim API key."
    )
    parser.add_argument(
        "--message",
        default="In one sentence, what can you help with for the Caesim project?",
    )
    parser.add_argument(
        "--name",
        default=os.environ.get("BACKBOARD_API_NAME", "Caesim"),
        help="Assistant name to create.",
    )
    parser.add_argument(
        "--system-prompt",
        default=(
            "You are a concise code assistant for the Caesim project. "
            "Answer practically and ask for missing context when needed."
        ),
    )
    args = parser.parse_args()

    api_key = (
        os.environ.get("BACKBOARD_API_KEY_CAESIM")
        or os.environ.get("BACKBOARD_API_KEY")
        or os.environ.get("BACKBOARD_API_KEY2")
    )
    if not api_key:
        sys.stderr.write(
            "Missing BACKBOARD_API_KEY_CAESIM, BACKBOARD_API_KEY, or BACKBOARD_API_KEY2\n"
        )
        return 2

    client = BackboardClient(api_key)

    assistant = client.post_json(
        "/assistants",
        {
            "name": args.name,
            "system_prompt": args.system_prompt,
        },
    )
    assistant_id = require_field(assistant, "assistant_id")
    print(f"Created assistant: {assistant_id}")

    thread = client.post_json(f"/assistants/{assistant_id}/threads", {})
    thread_id = require_field(thread, "thread_id")
    print(f"Created thread: {thread_id}")

    result = client.post_form(
        f"/threads/{thread_id}/messages",
        {
            "content": args.message,
            "stream": "false",
            "memory": "Auto",
        },
    )
    print(f"Assistant: {result.get('content', json.dumps(result, indent=2))}")

    return 0


class BackboardClient:
    def __init__(self, api_key: str) -> None:
        self.api_key = api_key

    def post_json(self, path: str, payload: Dict[str, Any]) -> Dict[str, Any]:
        body = json.dumps(payload).encode("utf-8")
        request = urllib.request.Request(
            f"{BASE_URL}{path}",
            data=body,
            headers={
                "Content-Type": "application/json",
                "X-API-Key": self.api_key,
            },
            method="POST",
        )
        return self._send(request)

    def post_form(self, path: str, payload: Dict[str, str]) -> Dict[str, Any]:
        body = urllib.parse.urlencode(payload).encode("utf-8")
        request = urllib.request.Request(
            f"{BASE_URL}{path}",
            data=body,
            headers={
                "Content-Type": "application/x-www-form-urlencoded",
                "X-API-Key": self.api_key,
            },
            method="POST",
        )
        return self._send(request)

    def _send(self, request: urllib.request.Request) -> Dict[str, Any]:
        try:
            with urllib.request.urlopen(request, timeout=90) as response:
                raw = response.read().decode("utf-8")
        except urllib.error.HTTPError as exc:
            detail = exc.read().decode("utf-8", errors="replace")
            raise RuntimeError(f"Backboard API returned HTTP {exc.code}: {detail}") from exc
        except urllib.error.URLError as exc:
            raise RuntimeError(f"Could not reach Backboard API: {exc}") from exc

        try:
            parsed = json.loads(raw)
        except json.JSONDecodeError as exc:
            raise RuntimeError(f"Backboard API returned non-JSON response: {raw}") from exc
        if not isinstance(parsed, dict):
            raise RuntimeError(f"Expected JSON object, got: {parsed!r}")
        return parsed


def require_field(payload: Dict[str, Any], field: str) -> str:
    value = payload.get(field)
    if not isinstance(value, str) or not value:
        raise RuntimeError(f"Response did not include {field}: {payload}")
    return value


def load_dotenv(path: Path) -> None:
    if not path.exists():
        return
    for line in path.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#") or "=" not in stripped:
            continue
        key, value = stripped.split("=", 1)
        os.environ.setdefault(key.strip(), value.strip().strip("\"'"))


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RuntimeError as exc:
        sys.stderr.write(f"{exc}\n")
        raise SystemExit(1)
