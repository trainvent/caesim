# Caesim

Caesim is a safe image-library trimming CLI. It scans a folder, finds images that match a rule or Vision label, and moves matches into a review folder instead of deleting them.

## Install

```bash
cargo install --path .
```

To build a Debian package:

```bash
sudo apt install debhelper cargo rustc pkg-config
dpkg-buildpackage -b -us -uc
sudo apt install ../caesim_0.1.0_amd64.deb
```

If your Rust toolchain was installed outside apt, `dpkg-buildpackage -d` may be needed.

## Usage

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

## Cut Rules

Current local rules:

- `screenshots`
- `duplicates`
- `landscape`
- `portrait`

`explicit` is available with Vision mode, where it uses Google SafeSearch signals. `--find <label>` also enables Vision mode for label searches such as `food`, `receipt`, or `cars`.

Useful options:

- `--dry-run`: preview without moving files
- `--destination <folder>`: choose the review folder
- `--cut-dir <name>`: change the default `cut` folder name
- `--report <file>`: choose the report path

## Vision

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

Account commands use Supabase environment variables from `.env` or the shell:

- `CAESIM_SUPABASE_URL`
- `CAESIM_SUPABASE_ANON_KEY`
- optional: `CAESIM_SUPABASE_SERVICE_ROLE_KEY`

Common commands:

```bash
caesim signup --email you@example.com
caesim login --email you@example.com
caesim whoami
caesim credits balance
caesim logout
```

`caesim ai-assist` starts an interactive assistant that turns cleanup requests into safe `caesim cut` commands.

## Development

See [DEVELOPER.md](DEVELOPER.md) for implementation notes, CLI details, and test guidance.
