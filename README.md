# Caesim

Caesim is a small CLI-style concept for trimming large image libraries.

Core idea:

1. You describe which images you do not want (for example: screenshots, duplicates, landscape, portrait, low-quality shots).
2. Caesim scans a folder and finds matches.
3. Matches are moved into a separate `cut` folder for review, or to a custom folder with `--destination`.
4. Nothing is hard-deleted by default.

This keeps cleanup fast while preserving a safety review step.

## Product Direction

The public site frames Caesim as:

1. Local utility with a simple install/run flow
2. “Cut folder” workflow instead of immediate deletion
3. A concept page, not a production SaaS product

Example command concept:

```bash
caesim cut ./my-photos --rule screenshots
```

## MVP Scope

1. CLI command: `caesim cut <path> --rule "<text>" [--destination <folder>]`
2. File scanner (recursive image discovery for common raster formats plus SVG)
3. Rule matcher (initially simple heuristics + tags)
4. Move matched files into `<path>/cut/`
5. Generate a run report:
   - scanned count
   - matched count
   - moved file list
6. Add `--dry-run` mode

## Safety Rules

1. Default action is move, not delete.
2. Never overwrite existing files in `cut` (auto-rename collisions).
3. Keep a manifest log to restore files if needed.

## Status

Early executable MVP. Build recipe is in [DEVELOPER.md](/home/leonmarq/Code/caesim/DEVELOPER.md).

## Install / Run

Build the local executable:

```bash
cargo build
```

Run it from the repo:

```bash
cargo run -- cut ./my-photos --rule screenshots --dry-run
```

Cut landscape photos into a review folder:

```bash
cargo run -- cut ./my-photos --rule landscape --destination ./review --dry-run
```

Cut portrait photos:

```bash
cargo run -- cut ./my-photos --rule portrait --dry-run
```

Send matches into a custom folder:

```bash
cargo run -- cut ./my-photos --rule screenshots --destination ./review --dry-run
```

Or install the `caesim` command from this checkout:

```bash
cargo install --path .
caesim cut ./my-photos --rule screenshots --dry-run
```

Duplicate matching detects exact file-content duplicates and common exported-copy names such as `image (1).svg` when `image.svg` exists. With `--vision`, Caesim uses Google Cloud Vision signals for richer matching. `duplicates` asks for web matches, OCR, labels, and dominant colors; object matching now goes through a separate `--contains` flag.

## Dev quickstart (Rust + optional Python Vision)

### Local-only run (no AI)

```bash
cargo run -- cut ./my-photos --rule screenshots --dry-run
```

### Google Cloud Vision (optional backend)

This repo includes a small Python backend (`python/vision_backend.py`) that the Rust CLI can call via **Cloud Vision API**.

- Install deps:

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
```

- Configure Google credentials (recommended: Application Default Credentials) and enable the Vision API in your GCP project.
- Run with Vision enabled:

```bash
cargo run -- cut ./my-photos --rule explicit --vision --dry-run
```

Use Vision-assisted duplicate detection:

```bash
cargo run -- cut ./my-photos --rule duplicates --vision --dry-run
```

Use Vision label detection for object-style filtering:

```bash
cargo run -- cut ./my-photos --contains cars --vision --dry-run
```

Backboard is available as an interactive assistant mode:

```bash
cargo run -- --ai-assist
```

It asks for a natural-language request, sends it to Backboard, and shows a suggested `caesim` command before execution.
Set `BACKBOARD_API_KEY_CAESIM` in your environment before using it.

You can customize the assistant's system prompt by setting `BACKBOARD_PROMPT` in your `.env`. Example:

```bash
# BACKBOARD_PROMPT="Translate user's request into a caesim command; return JSON with 'command' and 'explanation'"
```

Note: the first `cargo build` / `cargo run` needs access to `crates.io` to download Rust dependencies.

## Source

Based on `https://caesim.com/` content (accessed April 23, 2026).
