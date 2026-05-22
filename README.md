# Caesim

Caesim is a safe image-library trimming CLI. It scans a folder, finds images that match a rule or Vision label, and moves matches into a review folder instead of deleting them.

## Install

```bash
cargo install --path .
```

Or download and install the ready made package

## Quick Start

Preview a run first:

```bash
caesim cut ./photos --rule screenshots --dry-run
```

Move matches into `./photos/cut`:

```bash
caesim cut ./photos --rule screenshots
```

Use a custom destination:

```bash
caesim cut ./photos --rule landscape --destination ./review
```

Search by Vision label:

```bash
caesim cut ./photos --find receipt --dry-run
```

Caesim writes a JSON report after each run. By default it is saved as `.caesim-report.json` in the scanned folder.

You can restore a previous cut from that report with:

```bash
caesim cut undo --report ./photos/.caesim-report.json
```

## Rules

Current local rules:

- `screenshots`
- `duplicates`
- `explicit`
- `landscape`
- `portrait`

`explicit` uses Google SafeSearch signals when Vision mode is enabled. `--find <label>` also enables Vision mode for label searches such as `receipt` or `cars`.

Useful options:

- `--dry-run`: preview without moving files
- `--destination <folder>`: choose the review folder
- `--cut-dir <name>`: change the default `cut` folder name
- `--report <file>`: choose the report path

## Vision Mode

Vision mode requires a local Caesim session, credits, Google credentials, and the Python Vision backend dependencies.

```bash
caesim vision
caesim login
caesim credits balance
```

If needed, install the Python dependency:

```bash
pip install -r requirements.txt
```

You can point Caesim at a custom backend with `CAESIM_VISION_BACKEND`.

## Accounts and AI Assist

For Supabase environment variables and session setup, see [DEVELOPER.md](DEVELOPER.md).

Common commands:

```bash
caesim signup --email you@example.com
caesim login --email you@example.com
caesim whoami
caesim credits balance
caesim logout
```

`caesim ai-assist` starts an interactive assistant that turns cleanup requests into safe `caesim cut` commands.

## Documentation

- [CONTRIBUTING.md](CONTRIBUTING.md) for contributor workflow and pull request expectations.
- [DEVELOPER.md](DEVELOPER.md) for architecture, implementation notes, and release details.
