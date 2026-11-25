#!/usr/bin/env bash
# Stage the CADCraft Nightly-only handoff for the Arch packager.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
source_tree="$1"
work="$2"
destination="$3"
basename="$4"
stage="$work/$basename"

mkdir -p "$work"
cp -R "$source_tree" "$stage"
mkdir "$stage/bin/.cadcraft-nightly"
mv "$stage/bin/cadcraft" "$stage/bin/.cadcraft-nightly/cadcraft"
cp "$here/nightly-arch-launch.sh" "$stage/bin/cadcraft"
chmod 755 "$stage/bin/cadcraft"
tar -C "$work" -czf "$destination/$basename.tar.gz" "$basename"
echo "wrote $destination/$basename.tar.gz"
