import argparse
import asyncio
import os
import sys
from pathlib import Path
from typing import Any, AsyncIterable


def main() -> int:
    load_dotenv(Path(".env"))

    parser = argparse.ArgumentParser(
        description="Create a Backboard assistant/thread and send a first message."
    )
    parser.add_argument(
        "--message",
        default="Hello! Tell me how you can help with the Caesim project.",
    )
    parser.add_argument(
        "--name",
        default=os.environ.get("BACKBOARD_API_NAME", "Caesim"),
        help="Assistant name to create.",
    )
    parser.add_argument(
        "--system-prompt",
        default=(
            "You are a practical code assistant for the Caesim project. "
            "Be concise, concrete, and ask for missing context when needed."
        ),
    )
    parser.add_argument("--provider", default="openai", help="LLM provider.")
    parser.add_argument("--model", default="gpt-4o", help="Model name.")
    parser.add_argument(
        "--memory",
        default="Auto",
        choices=["Auto", "Readonly", "off"],
        help="Memory Lite mode.",
    )
    parser.add_argument(
        "--stream",
        action="store_true",
        help="Stream the assistant response as it is generated.",
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

    return asyncio.run(run(args, api_key))


async def run(args: argparse.Namespace, api_key: str) -> int:
    try:
        from backboard import BackboardClient
    except ImportError:
        sys.stderr.write(
            "Missing Backboard SDK. Install it with: pip install -r requirements.txt\n"
        )
        return 2

    client = BackboardClient(api_key=api_key)

    assistant = await client.create_assistant(
        name=args.name,
        system_prompt=args.system_prompt,
    )
    assistant_id = get_attr(assistant, "assistant_id", "assistantId")
    print(f"Created assistant: {assistant_id}")

    thread = await client.create_thread(assistant_id)
    thread_id = get_attr(thread, "thread_id", "threadId")
    print(f"Created thread: {thread_id}")

    response = await client.add_message(
        thread_id=thread_id,
        content=args.message,
        llm_provider=args.provider,
        model_name=args.model,
        memory=args.memory,
        memory_response_citation=False,
        stream=args.stream,
    )

    if args.stream:
        full_content = await print_stream(response)
        print(f"\nFull response: {full_content}")
    else:
        print(f"Assistant: {get_attr(response, 'content')}")

    return 0


async def print_stream(stream: AsyncIterable[Any]) -> str:
    full_content = ""
    async for chunk in stream:
        chunk_type = get_field(chunk, "type")
        if chunk_type == "content_streaming":
            content_piece = get_field(chunk, "content") or ""
            full_content += content_piece
            print(content_piece, end="", flush=True)
        elif chunk_type == "run_ended":
            break
    return full_content


def get_attr(value: Any, *names: str) -> Any:
    for name in names:
        if hasattr(value, name):
            return getattr(value, name)
        if isinstance(value, dict) and name in value:
            return value[name]
    raise RuntimeError(f"Could not find any of {names!r} on {value!r}")


def get_field(value: Any, name: str) -> Any:
    if isinstance(value, dict):
        return value.get(name)
    return getattr(value, name, None)


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
