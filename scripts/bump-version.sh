#!/usr/bin/env bash
# Sets the version in Cargo.toml (and Cargo.lock). The next push to main that
# passes CI publishes it as a release: scripts/bump-version.sh 0.2.0
set -euo pipefail
cd "$(dirname "$0")/.."
v="${1:?usage: scripts/bump-version.sh X.Y.Z}"
[[ $v =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "not a version: $v" >&2; exit 1; }
sed -i "0,/^version = \".*\"/s//version = \"$v\"/" Cargo.toml
sed -i "/^name = \"ferrum\(-core\)\?\"$/{n;s/^version = \".*\"/version = \"$v\"/}" Cargo.lock
grep -n "^version" Cargo.toml | head -1
echo "Commit and push to main: CI publishes v$v when the tests and builds pass."
