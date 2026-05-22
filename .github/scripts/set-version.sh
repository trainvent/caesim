#!/usr/bin/env bash
set -euo pipefail
# Determine version from tag (GITHUB_REF expected in CI)
if [ -z "${GITHUB_REF:-}" ]; then
  echo "GITHUB_REF is not set; expecting refs/tags/vX.Y.Z" >&2
  exit 1
fi
VERSION="${GITHUB_REF#refs/tags/}"
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
