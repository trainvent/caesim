#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <version>" >&2
  echo "Example: $0 0.1.6" >&2
  exit 1
fi

VERSION="$1"
VERSION="${VERSION#v}"

if ! [[ "${VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]]; then
  echo "Invalid version '${VERSION}'. Expected a semver string like 0.1.6 or 0.1.6-rc.1." >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${REPO_ROOT}"

CURRENT_VERSION="$(perl -0ne 'print $1 if /^version = "([^"]+)"$/m' Cargo.toml)"
if [[ "${CURRENT_VERSION}" == "${VERSION}" ]]; then
  echo "Version ${VERSION} is already set; running checks and build only."
else
  RELEASE_TAG="v${VERSION}" bash ./.github/scripts/set-version.sh
fi

cargo test --test version-numbers
cargo build

echo
echo "Version bumped to ${VERSION}."
echo "Review the changes, then commit and tag as needed."
