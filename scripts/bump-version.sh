#!/bin/bash
set -e

# Bump version across Cargo.toml, tauri.conf.json, and package.json
# Usage: ./scripts/bump-version.sh 2026.3.0

if [ $# -eq 0 ]; then
    echo "Usage: $0 <version>"
    echo "Example: $0 2026.3.0"
    exit 1
fi

NEW_VERSION="$1"

# Validate version format (CalVer: YYYY.M.P)
if ! [[ $NEW_VERSION =~ ^[0-9]{4}\.[0-9]+\.[0-9]+$ ]]; then
    echo "Error: Version must be in CalVer format (YYYY.M.P), e.g., 2026.3.0"
    exit 1
fi

echo "Bumping version to $NEW_VERSION..."

# Update Cargo.toml
echo "Updating src-tauri/Cargo.toml..."
sed -i.bak "s/^version = \".*\"/version = \"$NEW_VERSION\"/" src-tauri/Cargo.toml
rm src-tauri/Cargo.toml.bak

# Update tauri.conf.json
echo "Updating src-tauri/tauri.conf.json..."
sed -i.bak "s/\"version\": \".*\"/\"version\": \"$NEW_VERSION\"/" src-tauri/tauri.conf.json
rm src-tauri/tauri.conf.json.bak

# Update package.json
echo "Updating package.json..."
sed -i.bak "s/\"version\": \".*\"/\"version\": \"$NEW_VERSION\"/" package.json
rm package.json.bak

echo "✓ Version bumped to $NEW_VERSION in all three files"
echo ""
echo "Next steps:"
echo "  1. Review changes: git diff"
echo "  2. Commit: git add -u && git commit -m \"chore(release): bump version to $NEW_VERSION\""
echo "  3. Tag: git tag v$NEW_VERSION"
echo "  4. Push: git push && git push --tags"
echo "  5. CI will build and create draft release"
echo "  6. Review draft, edit release notes, and publish"
