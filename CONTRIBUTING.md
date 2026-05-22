# Contributing to Caesim

Thanks for helping improve Caesim. This project is a local Rust CLI for safely trimming image libraries, so contributions should keep the tool deterministic, non-destructive, and easy to review.

## What Belongs Here

Use this guide for anything that changes the codebase:

- CLI behavior and flags
- rule matching and image scanning
- move/collision handling
- report and manifest output
- auth, Vision, or AI-assisted flows
- release packaging and GitHub Actions

If you are changing user-facing usage notes, install steps, or examples, update [README.md](README.md). If you are changing implementation details or architecture, update [DEVELOPER.md](DEVELOPER.md).

## Before You Start

- Check the current docs so your change matches the project contract.
- Prefer a small, focused change over broad refactors.
- Do not introduce destructive behavior. Caesim should move files, not delete them.
- Preserve backward compatibility where practical, especially for `caesim cut` and existing aliases.

## Suggested Workflow

1. Create a branch for the change.
2. Make the smallest change that solves the problem.
3. Add or update tests when behavior changes.
4. Update docs if the user experience changes.
5. Validate locally before opening a pull request.

## Code Expectations

- Keep the CLI behavior predictable and explicit.
- Use clear error messages, especially around file paths, rules, and auth state.
- Keep rule evaluation deterministic unless a feature explicitly depends on Vision or AI.
- Preserve safety features such as dry-run mode, collision-safe moves, and skip-inside-cut guards.
- Avoid adding new dependencies unless they solve a real problem.

## Validation

The repository is Rust-based, so the main local checks are:

```bash
cargo test
cargo run -- cut ./assets/test/Random_Images --rule screenshots --dry-run
```

If your change touches packaging or release behavior, also sanity-check the Debian build path described in [DEVELOPER.md](DEVELOPER.md).

## Pull Requests

Please include:

- a short description of what changed
- the reason for the change
- any user-visible impact
- the validation you ran

If the change affects CLI output or docs, include an example command or before/after output when helpful.

## Reporting Issues

When filing an issue, include:

- the command you ran
- the full error message
- your OS and Rust version if relevant
- whether the problem happens in `--dry-run`
- a minimal example path or file set if possible

## Release Notes

Release-related changes should stay consistent with the current Debian packaging workflow and GitHub Actions release process. If you modify either, update the relevant docs in [README.md](README.md) and [DEVELOPER.md](DEVELOPER.md).
