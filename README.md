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

Caesim writes a JSON report after each run. By default it is saved in your XDG cache directory (or `$HOME/.cache/caesim`) so it does not clutter the scanned folder. Report filenames are now prefixed with the run id, for example: `1779436443-Random_Images.caesim-report.json`. You can still override the path with `--report`.

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

Vision mode requires Google credentials and the Python Vision backend dependencies.
Credit charging is now opt-in via `--charge-vision-credits`.

```bash
cargo run -- settings vision
caesim cut ./photos --find receipt --dry-run
```

If needed, install the Python dependency:

```bash
pip install -r requirements.txt
```

You can point Caesim at a custom local backend with `CAESIM_VISION_BACKEND`.
To use the Cloud Run Vision function instead, set `CAESIM_VISION_URL` to the
function URL and sign in with `caesim login`; the CLI sends your Caesim session
token to the function. `CAESIM_VISION_BEARER_TOKEN` is only needed for legacy
Cloud Run IAM/proxy-token smoke tests.

The Cloud Run backend also includes an async GCS batch API for larger hosted
runs. See [DEVELOPER.md](DEVELOPER.md) for the bucket env vars and status
endpoint.

If you still want billing-style credit deduction for a run, add:

```bash
caesim cut ./photos --find receipt --charge-vision-credits
```

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

## Payments

The Supabase schema now includes a Stripe-ready payment foundation:

- `public.users` carries billing metadata such as `stripe_customer_id`.
- `public.credit_ledger` records every credit delta with source metadata.
- `public.payment_events` stores idempotent payment events for later webhook replay.
- `public.refund_events` records refund webhooks and credit reversals.
- The credit gateway creates Stripe Checkout Sessions and verifies `checkout.session.completed` and `charge.refunded` webhooks.

Credits are sold in 1000-credit packs. The default pack price is `$1.69`, configured with `CREDIT_PACK_PRICE_CENTS=169`.

```bash
caesim credits buy --credits 1000
caesim credits purchases
caesim credits refund-request <purchase-id>
```

See [supabase/README.md](supabase/README.md) and [supabase/functions/credit-gateway/README.md](supabase/functions/credit-gateway/README.md) for the request shape.

## Legal Policies

Policy drafts live in [legal](legal):

- [Terms of Service](legal/terms-of-service.md)
- [Privacy Policy](legal/privacy-policy.md)
- [Refund Policy](legal/refund-policy.md)
- [Credit Policy](legal/credit-policy.md)

The CLI asks for Terms and Privacy acceptance during signup, and Refund and Credit Policy acceptance before creating Stripe Checkout.

## Version Bumps

To bump the release version in one step, run:

```bash
./scripts/bump-version.sh 0.1.6
```

This updates [Cargo.toml](Cargo.toml), [Cargo.lock](Cargo.lock), and [debian/changelog](debian/changelog), then runs the version checks and a build.

## Documentation

- [CONTRIBUTING.md](CONTRIBUTING.md) for contributor workflow and pull request expectations.
- [DEVELOPER.md](DEVELOPER.md) for architecture, implementation notes, and release details.
