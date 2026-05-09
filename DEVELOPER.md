# Developer Build Recipe

This recipe is based on the current `caesim.com` concept page: a local tool that moves unwanted images into a `cut` folder for review.

## 1. Product Contract

Build a safe image-library trimming utility with:

1. One primary command: `caesim cut <target> --rule "<description>"`
2. Non-destructive default behavior (move, do not delete)
3. Reviewable output (`cut` folder + manifest/log)
4. Direct Supabase Auth for account/session handling; no hosted Rust backend in the product path

## 1.1 MVP Scope

The MVP delivers:

1. CLI command: `caesim cut <path> --rule "<text>" [--destination <folder>]`
2. File scanner (recursive image discovery for common raster formats: `jpg`, `jpeg`, `png`, `webp`, `heic`, `tiff`, `gif`)
3. Rule matcher (initially simple heuristics + tags)
4. Move matched files into `<path>/cut/`
5. Generate a run report:
   - scanned count
   - matched count
   - moved file list
6. Support `--dry-run` mode for safe preview

## 1.2 Safety Requirements (MVP)

1. **Default action is move, not delete.** Files are never permanently deleted.
2. **Never overwrite existing files in `cut`.** Auto-rename collisions using suffixes (`_1`, `_2`, ...).
3. **Keep a manifest log** to restore files if needed.
4. **Skip moving files already inside `cut`** (scope guard to prevent recursion).
5. **Abort cleanly with partial-run report** on failures.

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
   - optional Google Vision duplicate signals behind `--find`
3. Image-stat placeholders:
   - brightness threshold
   - blur score threshold

Keep rule processing modular so we can add model-based classification later.

## 4.1 AI Integration (later, optional)

Backboard is **not required** for the MVP deterministic CLI or image recognition.
Keep the product local and deterministic by default, with only the optional Python
Google Vision backend behind an explicit `--find` flag. A later Backboard integration
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

## 11. Dev Quickstart (Rust + optional Python Vision)

### Building from source

```bash
cargo build
```

### Debian package build

Build a local `.deb` that can be installed with `apt`:

```bash
sudo apt install debhelper cargo rustc libssl-dev pkg-config
dpkg-buildpackage -b -us -uc
sudo apt install ../caesim_0.1.0_amd64.deb
```

If `cargo` and `rustc` come from rustup instead of apt, Debian's build-dependency check may still fail. In that case, use `dpkg-buildpackage -d` locally or build inside a Debian environment where those toolchains are installed as packages.

Run the compiled binary:

```bash
cargo run -- cut ./my-photos --rule screenshots --dry-run
```

Or install the CLI globally:

```bash
cargo install --path .
caesim cut ./my-photos --rule screenshots --dry-run
```

### Local-only command (manual, deterministic)

```bash
cargo run -- cut ./my-photos --rule screenshots --dry-run
```

This runs without any AI assistance or Vision API—purely local heuristics.

### Google Cloud Vision (optional backend)

This repo includes a Python backend (`python/vision_backend.py`) that the Rust CLI can optionally call via the **Google Cloud Vision API** for richer image analysis.

**Setup:**

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
```

**Configure Google credentials:**
- Enable the Vision API in your GCP project.
- Use Application Default Credentials (recommended) or set `GOOGLE_APPLICATION_CREDENTIALS`.

**Run with Vision enabled:**

```bash
cargo run -- cut ./my-photos --rule explicit --find safety --dry-run
cargo run -- cut ./my-photos --rule duplicates --find --dry-run
cargo run -- cut ./my-photos --find cars --dry-run
```

### AI-assisted command creation (Backboard)

Use **Backboard** to generate commands from natural language:

```bash
cargo run -- ai-assist
```

Describe what you want to clean up (e.g., "remove all screenshots"), and Backboard will suggest a `caesim` command before execution.

**Setup:**
- Set `BACKBOARD_API_KEY_CAESIM` in your environment.

**Custom prompt:**
You can customize the AI behavior by setting `BACKBOARD_PROMPT` in your `.env`:

```bash
BACKBOARD_PROMPT="Translate user's request into a caesim command; return JSON with 'command' and 'explanation'"
```

### Supabase Auth (account & session management)

Caesim uses **Supabase Auth** directly for account management and session handling—no custom backend required.

**Setup:**

```bash
export CAESIM_SUPABASE_URL="https://<project-ref>.supabase.co"
export CAESIM_SUPABASE_SERVICE_ROLE_KEY="<service-role-key>"
```

The service-role key is only needed if you want Caesim to manage credit balances in a `users` table. Keep it on a trusted machine only.

**Commands:**

```bash
caesim signup              # Create a new account
caesim login               # Sign in with password
caesim login --otp         # Sign in with one-time password
caesim whoami              # Show current user session
```

**Email confirmation:**
If you want Supabase to send a code instead of a browser link, edit the `auth.email.template.confirmation` email template and use `{{ .Token }}` in the body (instead of the default `{{ .ConfirmationURL }}`).

### Dependencies

The first `cargo build` / `cargo run` requires access to `crates.io` to download Rust dependencies. Ensure you have internet access and Cargo installed.

## 12. Sources and Credits

- **Concept reference**: `https://caesim.com/` (accessed April 23, 2026). The site frames Caesim as an image-library trimming concept and explicitly states "Not a product."
- **Supabase Rust guide**: https://docs.rs/supabase-lib-rs/
- **Tools in use**:
  - Backboard.io for AI-assisted command creation
  - Google Cloud Vision API for image recognition (optional)
- **Credits**: Codex and GitHub Copilot were used in development
