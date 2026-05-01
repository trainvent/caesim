# Caesim

Caesim is a small CLI-style concept for trimming large image libraries.

Core idea:

1. You describe which images you do not want (for example: screenshots, duplicates, low-quality shots).
2. Caesim scans a folder and finds matches.
3. Matches are moved into a separate `cut` folder for review.
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

1. CLI command: `caesim cut <path> --rule "<text>"`
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

Or install the `caesim` command from this checkout:

```bash
cargo install --path .
caesim cut ./my-photos --rule screenshots --dry-run
```

Duplicate matching detects exact file-content duplicates and common exported-copy
names such as `image (1).svg` when `image.svg` exists. With `--vision`, Caesim
also asks Google Cloud Vision for Web Detection, OCR, labels, and dominant colors
to catch more visually similar duplicates.

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

Backboard is planned as a report assistant layer: it should read `.caesim-report.json`
after a run and explain or suggest follow-up commands, but not move files directly.

Note: the first `cargo build` / `cargo run` needs access to `crates.io` to download Rust dependencies.

## Source

Based on `https://caesim.com/` content (accessed April 23, 2026).
