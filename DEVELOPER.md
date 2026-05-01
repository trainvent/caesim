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
caesim cut <path> [--rule <text>] [--cut-img <image>] [--dry-run] [--cut-dir <name>] [--report <file>]
```

## 2.2 Arguments

1. `<path>`: root folder to scan
2. `--rule`: plain-language rule (stored in report); `--cut-rule` remains as a compatibility alias
2. `--cut-img`: filter out all images containing the given image (stored in report)
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

## 3.1 Complexity + Cost Gating (pre-publish, staged)

We want a gating flow before we ship anything that could incur real cost (e.g. future model-based classification, cloud execution, or any paid APIs).

For now (MVP milestone), **implement complexity estimation only**. Do **not** implement money movement, charging, or external billing.

### Complexity estimate (now)

When the user runs the command, compute and surface a deterministic estimate:

- **Complexity inputs**: number of files scanned, bytes, image types, enabled rule(s), and whether any expensive analysis would be required.
- **Complexity output**: a single numeric score (and/or tiers like `low|medium|high`) recorded in the report JSON.
- **User UX**: print the estimate before doing any irreversible action (moving files is still non-destructive, but treat it as gated).

### Wallet + cost estimate (later, last step before publishing)

Add a **Wallet** concept that users can preload with money (USD). The flow should be:

1. **Estimate first**: show an estimated dollar cost derived from complexity (and any metered operations).
2. **First confirmation**: user acknowledges the estimate.
3. **Second confirmation**: user confirms again to proceed with the paid run.
4. **Then run**: produce final results and deduct from wallet.

Until we implement the wallet, steps (2)-(4) are placeholders and should be clearly labeled as such in docs/CLI output.

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
   - common exported-copy names (e.g., `image (1).svg` when `image.svg` exists)
   - optional Google Vision duplicate signals behind `--vision`
3. Image-stat placeholders:
   - brightness threshold
   - blur score threshold

Keep rule processing modular so we can add model-based classification later.

## 4.1 AI Integration (later, optional)

Backboard is **not required** for the MVP deterministic CLI or image recognition.
Keep the product local and deterministic by default, with only the optional Python
Google Vision backend behind an explicit `--vision` flag. A later Backboard integration
may become useful in
two places:

1. **Advanced semantic cut rules (optional provider)**:
   - When rules go beyond filenames/hashes/basic image stats (e.g. “contains faces”, “contains receipts”, “private info”, “memes”, “screenshots of chats”, “bad composition”), route scoring through a provider-backed classifier/assistant.
   - Keep the local pipeline unchanged: the provider should return structured labels/tags + confidence so we can map them to deterministic `reason` strings in the report.
   - This is the main future source of “real cost”, so it must plug into the complexity/cost gating flow.

2. **Post-run assistant for cleanup + explanation**:
   - After a run completes, treat the report (`.caesim-report.json`) as the primary artifact.
   - An assistant can answer questions like “why was this cut?”, “group by reason”, “suggest a safer rule”, or “show me borderline items”, using the report contents.
   - This mode is “assistive” and should never move/delete files by itself; it only proposes actions or generates a follow-up command invocation.

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
