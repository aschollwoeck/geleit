#!/usr/bin/env bash
# Assert the engine/core crates do not depend on the UI/app crate.
#
# Constitution P4 / guidelines §2 / ADR-0003: the engine must be UI-agnostic. The
# `app -> engine -> core` direction already makes the reverse a Cargo dependency cycle
# (which Cargo forbids); this check is belt-and-suspenders and documents the invariant.
#
# Uses `cargo tree` only (no extra tooling). Output is captured before matching so that
# (a) a `cargo tree` failure aborts via `set -e` instead of being masked by the pipeline,
# and (b) SIGPIPE from `grep -q` cannot hide a real match (no pipeline in the `if`).
set -euo pipefail

UI_CRATE="geleit-app"
ENGINE_CRATES=("geleit-core" "geleit-engine")

fail=0
for crate in "${ENGINE_CRATES[@]}"; do
    tree="$(cargo tree --package "$crate" --edges normal --prefix none)"
    names="$(printf '%s\n' "$tree" | awk '{print $1}')"
    if grep -qx "$UI_CRATE" <<<"$names"; then
        echo "BOUNDARY VIOLATION: $crate depends on $UI_CRATE" >&2
        fail=1
    fi
done

if [ "$fail" -ne 0 ]; then
    exit 1
fi
echo "Boundary OK: engine/core crates do not depend on $UI_CRATE"
