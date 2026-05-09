# Caesim

Caesim is a local CLI utility for safely trimming large image libraries. Describe what to cut, and Caesim scans for matches and moves them into a review folder instead of deleting them.

## Quick Start

### Install

```bash
cargo install --path .
```

To build a Debian package you can install with `apt`:

```bash
sudo apt install debhelper cargo rustc pkg-config
dpkg-buildpackage -b -us -uc
sudo apt install ../caesim_0.1.0_amd64.deb
```

If you are building with a Rust toolchain that was installed outside apt (for example via rustup), you may need `dpkg-buildpackage -d` to skip Debian's package-level dependency check on `cargo` and `rustc`.

### Basic Usage

Move all screenshots to a `cut` folder for review:

```bash
caesim cut ./my-photos --rule screenshots --dry-run
```

When ready (remove `--dry-run`):

```bash
caesim cut ./my-photos --rule screenshots
```

Cut landscape photos into a custom folder:

```bash
caesim cut ./my-photos --rule landscape --destination ./review
```

### How It Works

1. Run a simple command describing what you want to cut.
2. Caesim scans your photo folder and finds matches.
3. Matched images move into a `cut` folder for review.
4. Nothing is hard-deleted—you can always restore from the report.
5. Get a detailed log of what was moved and why.

### Supported Rules

- `screenshots`
- `duplicates`
- `blurry`
- `dark` / `low-light`
- `landscape` / `portrait`
- And more—see [DEVELOPER.md](DEVELOPER.md) for the complete rule engine.

### With AI Assistant

Let an AI assistant help you write the command:

```bash
caesim ai-assist
```

### Vision label search

Use `--find <label>` to enable the optional Google Vision backend and search for image labels (for example: `--find cars`). Vision mode requires configuring the Python vision backend and valid Google credentials; see `DEVELOPER.md` for details.


Just describe what you want to clean up, and Caesim generates the command for you.

## Development

For architecture, CLI specification, rule engine details, build recipes, dev setup, and testing—see [DEVELOPER.md](DEVELOPER.md).
