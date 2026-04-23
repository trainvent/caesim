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
caesim cut ./my-photos --rule "screenshots"
```

## MVP Scope

1. CLI command: `caesim cut <path> --rule "<text>"`
2. File scanner (recursive image discovery)
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

Documentation phase. Build recipe is in [DEVELOPER.md](/home/leonmarq/Code/caesim/DEVELOPER.md).

## Source

Based on `https://caesim.com/` content (accessed April 23, 2026).
