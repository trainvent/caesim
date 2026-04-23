# Developer Build Recipe

This recipe is based on the current `caesim.com` concept page: a local tool that moves unwanted images into a `cut` folder for review.

## 1. Product Contract

Build a safe image-library trimming utility with:

1. One primary command: `caesim cut <target> --rule "<description>"`
2. Non-destructive default behavior (move, do not delete)
3. Reviewable output (`cut` folder + manifest/log)

## 2. CLI Specification (MVP)

## 2.1 Command

```bash
caesim cut <path> --rule "<text>" [--dry-run] [--cut-dir <name>] [--report <file>]
```

## 2.2 Arguments

1. `<path>`: root folder to scan
2. `--rule`: plain-language rule (stored in report)
3. `--dry-run`: preview matches without moving files
4. `--cut-dir`: default `cut`
5. `--report`: default `.caesim-report.json`

## 3. Execution Flow

1. Validate path exists and is readable.
2. Discover image files recursively (`jpg`, `jpeg`, `png`, `webp`, `heic`, `tiff`, `gif`).
3. Score files against the rule matcher.
4. Produce candidate list.
5. If not `--dry-run`, move candidates to `<path>/<cut-dir>/`.
6. Write report manifest.

## 4. Rule Engine (MVP First Pass)

Start simple and deterministic:

1. Keyword rules:
   - `screenshots`
   - `duplicates`
   - `blurry`
   - `dark` / `low-light`
2. Metadata-based checks where available:
   - filename patterns (e.g., screenshot naming)
   - exact hash duplicates
3. Image-stat placeholders:
   - brightness threshold
   - blur score threshold

Keep rule processing modular so we can add model-based classification later.

## 5. Data Outputs

## 5.1 Report JSON

```json
{
  "run_id": "2026-04-23T15:00:00Z",
  "target_path": "./my-photos",
  "rule": "screenshots",
  "dry_run": false,
  "scanned_count": 18240,
  "matched_count": 2148,
  "moved_count": 2148,
  "cut_dir": "./my-photos/cut",
  "entries": [
    {
      "source": "./my-photos/a.png",
      "destination": "./my-photos/cut/a.png",
      "reason": "screenshot_pattern"
    }
  ]
}
```

## 5.2 Restore Manifest

Also write a compact mapping file for easy rollback:

```json
{ "./my-photos/cut/a.png": "./my-photos/a.png" }
```

## 6. Safety Requirements

1. Never permanently delete files in MVP.
2. Collision strategy: append suffix (`_1`, `_2`, ...).
3. Skip moving files already inside `cut`.
4. Abort cleanly with partial-run report on failures.

## 7. Suggested Implementation Plan

1. `src/cli.ts` or `src/cli.py` command parser
2. `src/scanner/*` recursive file discovery
3. `src/rules/*` rule evaluation engine
4. `src/mover/*` safe move + collision handling
5. `src/report/*` JSON manifest writer
6. `tests/*` with fixture library

## 8. Test Matrix (MVP)

1. Path validation errors
2. Empty folder
3. Screenshot rule hits
4. Duplicate detection
5. Dry-run no file moves
6. Collision renaming
7. Interrupted run still writes report

## 9. Definition of Done

1. Command runs locally on a sample folder.
2. Files are moved to `cut` safely.
3. Report contains deterministic results.
4. `--dry-run` produces identical match list without moving files.
5. Tests cover core safety behavior.

## 10. Source Note

Reference used: `https://caesim.com/` (accessed April 23, 2026). The page explicitly presents Caesim as an image-library trimming concept and states “Not a product.”
