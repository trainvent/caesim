# Copilot Instructions for `caesim`

## Build, test, and lint commands

This repository is currently in a **documentation phase** and does not define runnable build, test, or lint commands yet.

- No `package.json`, `pyproject.toml`, `requirements.txt`, `Makefile`, or CI workflow is present.
- No single-test command is defined yet.

## High-level architecture (MVP contract)

The product contract in `README.md` and `DEVELOPER.md` describes a local CLI utility centered on one command:

```bash
caesim cut <path> --rule "<text>" [--dry-run] [--cut-dir <name>] [--report <file>]
```

Expected execution flow:

1. Validate target path readability.
2. Recursively scan image files (`jpg`, `jpeg`, `png`, `webp`, `heic`, `tiff`, `gif`).
3. Evaluate files with a deterministic rule engine.
4. Produce candidate list.
5. Move matched files into `<path>/<cut-dir>/` unless `--dry-run`.
6. Write run report and restore mapping manifest.

Planned module boundaries (from `DEVELOPER.md`):

- `src/cli.*`: argument parsing and orchestration
- `src/scanner/*`: recursive discovery
- `src/rules/*`: rule evaluation
- `src/mover/*`: safe move + collision handling
- `src/report/*`: report + manifest writing

## Key conventions

- **Safety-first behavior**: default action is move to `cut`, never hard-delete.
- **Collision policy**: never overwrite in `cut`; append suffixes (`_1`, `_2`, ...).
- **Scope guard**: skip files that are already inside the cut directory.
- **Auditability**: always persist machine-readable outputs:
  - run report (`.caesim-report.json` by default)
  - restore manifest mapping destination back to source
- **Deterministic MVP rules first**: start with keyword + metadata heuristics (`screenshots`, `duplicates`, `blurry`, `dark/low-light`) before any model-based classifier.
- **Default option values**: `--cut-dir` defaults to `cut`; `--report` defaults to `.caesim-report.json`.
