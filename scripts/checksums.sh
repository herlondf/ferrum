#!/usr/bin/env bash
# Writes SHA256SUMS for every release file in dist/ (or the folder given).
# get.sh, get.ps1 and the in-app updater check downloads against it.
set -euo pipefail
cd "${1:-$(dirname "$0")/../dist}"
mapfile -t files < <(find . -maxdepth 1 -type f ! -name SHA256SUMS -printf '%f\n' | LC_ALL=C sort)
[[ ${#files[@]} -gt 0 ]] || { echo "no release files in $PWD" >&2; exit 1; }
sha256sum "${files[@]}" > SHA256SUMS
cat SHA256SUMS
