#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
header="$repo_root/swift/probes/Sources/CBinette/include/binette.h"

if [[ "${1:-}" == "--verify" ]]; then
  cbindgen "$repo_root/rust/binette" -q -c "$repo_root/cbindgen.toml" --verify -o "$header"
else
  cbindgen "$repo_root/rust/binette" -q -c "$repo_root/cbindgen.toml" -o "$header"
fi
