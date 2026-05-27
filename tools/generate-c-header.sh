#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cbindgen \
  "$repo_root/rust/binette" \
  -q \
  -c "$repo_root/cbindgen.toml" \
  -o "$repo_root/swift/probes/Sources/CBinette/include/binette.h"
