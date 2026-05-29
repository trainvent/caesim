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

# Ensure debian packaging files reflect the new version
echo "Updating debian packaging files to ${VERSION}..."
# Replace 'Version: x.y.z' and filenames like caesim_x.y.z in all files under debian/
if [[ -d debian ]]; then
  # Use sed to update Version: lines and caesim_... filenames. Also replace lingering 0.1.0 references
  find debian -type f -exec sed -i.bak -E \
    "s/(Version: )[0-9]+\.[0-9]+\.[0-9]+/\1${VERSION}/g; \
     s/(caesim_)[0-9]+\.[0-9]+\.[0-9]+/\1${VERSION}/g; \
     s/caesim-dbgsym_[0-9]+\.[0-9]+\.[0-9]+/caesim-dbgsym_${VERSION}/g; \
     s/\(0\.1\.0\)/(${VERSION})/g; \
     s/= 0\.1\.0/= ${VERSION}/g; \
     s/0\.1\.0/${VERSION}/g" {} \; || true
  find debian -name "*.bak" -delete || true
  echo "Debian packaging files updated (no commits made)."
else
  echo "No debian/ directory found; skipping packaging updates."
fi

cargo test --test version-numbers
cargo build

echo
echo "Version bumped to ${VERSION}."
echo "Review the changes, then commit and tag as needed."
