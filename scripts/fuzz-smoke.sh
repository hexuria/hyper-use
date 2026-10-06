#!/usr/bin/env bash
# Run every cargo-fuzz target for N seconds (default 60) on the dated nightly in $NIGHTLY.
# Skips cleanly while no fuzz/ crate exists. Crashes land in fuzz/artifacts/<target>/.
set -euo pipefail
cd "$(dirname "$0")/.."
secs="${1:-60}"
: "${NIGHTLY:?set NIGHTLY to a dated nightly, e.g. nightly-2026-10-01}"
if [[ ! -f fuzz/Cargo.toml ]]; then echo "fuzz: no fuzz/ crate yet, skipping"; exit 0; fi
# Pin the target to the toolchain host: a prebuilt (musl) cargo-fuzz otherwise defaults to its
# own build triple, which has no sanitizer support.
host="$(rustc "+$NIGHTLY" -vV | sed -n 's/^host: //p')"
for target in $(cargo "+$NIGHTLY" fuzz list); do
  echo "::group::fuzz $target (${secs}s)"
  corpus="fuzz/corpus/$target"; mkdir -p "$corpus"
  seeds="fuzz/seeds/$target"
  args=("$corpus"); [[ -d "$seeds" ]] && args+=("$seeds")
  cargo "+$NIGHTLY" fuzz run --target "$host" "$target" "${args[@]}" -- -max_total_time="$secs" -rss_limit_mb=2048
  echo "::endgroup::"
done
