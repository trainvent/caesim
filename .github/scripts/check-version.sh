#!/usr/bin/env bash
set -euo pipefail

# Determine version from RELEASE_TAG first, then GITHUB_REF in CI.
TAG="${RELEASE_TAG:-${GITHUB_REF:-}}"
if [[ -z "${TAG}" ]]; then
  echo "Missing release tag; set RELEASE_TAG or run from a tag push" >&2
  exit 1
fi

VERSION="${TAG#refs/tags/}"
VERSION="${VERSION#v}"

EXPECTED_TOML='version = "'
EXPECTED_LOCK='name = "caesim"'
EXPECTED_CHANGELOG="caesim (${VERSION})"

if ! grep -q "^version = \"${VERSION}\"$" Cargo.toml; then
  echo "Cargo.toml version does not match release tag ${VERSION}" >&2
  exit 1
fi

if ! perl -0ne 'exit(!(m{\[\[package\]\]\nname = "caesim"\nversion = "'"${VERSION}"'"}))' Cargo.lock; then
  echo "Cargo.lock root package version does not match release tag ${VERSION}" >&2
  exit 1
fi

if ! grep -q "^${EXPECTED_CHANGELOG}" debian/changelog; then
  echo "debian/changelog does not mention release ${VERSION}" >&2
  exit 1
fi

echo "Version metadata matches release tag ${VERSION}"
