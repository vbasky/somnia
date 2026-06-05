#!/usr/bin/env bash
#
# Release somnia to crates.io.
#
# The GitHub Release (notes pulled from CHANGELOG.md) and the crates.io publish
# both happen automatically in CI (`.github/workflows/release.yml`) when the tag
# is pushed. This script just bumps versions, validates, tags, and pushes.
#
# Usage:
#   scripts/release.sh <version>      e.g.  scripts/release.sh 0.2.0
#
# Steps:
#   1. Bump the workspace version + internal dependency pins
#   2. Validate (fmt, clippy, test, build)
#   3. Commit, tag, push  (push triggers the Release workflow)
#
# Notes:
#   * Add the new version's section to CHANGELOG.md BEFORE running — the release
#     notes published to GitHub are extracted from it, and the tree must be clean.
#   * CARGO_REGISTRY_TOKEN must be set as a repo secret for CI to publish.

set -euo pipefail

VERSION="${1:?usage: scripts/release.sh <version>   e.g. 0.2.0}"
TAG="v${VERSION}"

cd "$(git rev-parse --show-toplevel)"

# Workspace crates in dependency order (a crate appears after everything it depends on).
CRATES=(
  somnia-core
  somnia-derive
  somnia
)

# ── pre-flight ─────────────────────────────────────────────────────────────
[ "$(git rev-parse --abbrev-ref HEAD)" = "main" ] || { echo "✗ not on main"; exit 1; }
[ -z "$(git status --porcelain)" ]                  || { echo "✗ working tree not clean — commit or stash first"; exit 1; }
git rev-parse "$TAG" >/dev/null 2>&1                 && { echo "✗ tag $TAG already exists"; exit 1; }
command -v cargo >/dev/null                          || { echo "✗ cargo not found"; exit 1; }

grep -q "^## \[${VERSION}\] - " CHANGELOG.md         || { echo "✗ CHANGELOG.md has no '## [${VERSION}] - ...' section — add it first"; exit 1; }

CUR=$(grep -m1 '^version = ' Cargo.toml | sed -E 's/version = "([^"]+)"/\1/')
echo "==> releasing somnia  ${CUR}  ->  ${VERSION}"

# ── bump workspace version + internal dep pins ─────────────────────────────
# Workspace package version (root Cargo.toml [workspace.package]).
perl -i -pe "s/^version = \"\Q${CUR}\E\"/version = \"${VERSION}\"/" Cargo.toml
# Internal { path = "...", version = "X" } dep pins across all crate manifests.
for f in crates/*/Cargo.toml; do
  perl -i -pe "s/(version = )\"\Q${CUR}\E\"/\${1}\"${VERSION}\"/g" "$f"
done

# ── validate before tagging ────────────────────────────────────────────────
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace   # final manifest + compile validation

# ── commit, tag, push ──────────────────────────────────────────────────────
git add Cargo.toml crates/*/Cargo.toml CHANGELOG.md
git commit -m "release: ${TAG}"
git tag -a "${TAG}" -m "somnia ${VERSION}"
git push origin main
git push origin "${TAG}"

echo "✓ tag ${TAG} pushed — CI is creating the GitHub Release and publishing to crates.io"
