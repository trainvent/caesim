#!/usr/bin/env bash
set -euo pipefail
# Determine version from RELEASE_TAG first, then GITHUB_REF in CI.
TAG="${RELEASE_TAG:-${GITHUB_REF:-}}"
if [ -z "${TAG}" ]; then
  echo "Missing release tag; set RELEASE_TAG or run from a tag push" >&2
  exit 1
fi
VERSION="${TAG#refs/tags/}"
VERSION="${VERSION#v}"
echo "Setting package version to ${VERSION}"

# Update Cargo.toml version (simple sed replacement)
if grep -q '^version = ' Cargo.toml; then
  sed -i '0,/^version = ".*"/s//version = "'"${VERSION}"'"/' Cargo.toml
else
  echo "version = \"${VERSION}\"" >> Cargo.toml
fi

# Write debian/changelog
cat > debian/changelog <<EOF
caesim (${VERSION}) unstable; urgency=medium

  * Release ${VERSION}.

 -- Trainvent <dev@trainvent.com>  $(date -R)
EOF

echo "Wrote debian/changelog and updated Cargo.toml"
