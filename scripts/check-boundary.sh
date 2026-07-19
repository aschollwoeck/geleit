#!/usr/bin/env bash
# Assert the architectural boundaries hold (constitution P4 / guidelines §2 / ADR-0003, ADR-0012).
#
# Two invariants:
#
#   1. The engine/core crates must not depend on ANY UI crate. The `ui -> engine -> core` direction
#      already makes the reverse a Cargo cycle (which Cargo forbids); this documents the invariant
#      and catches a UI crate being pulled in as a dev/build dep, which Cargo would happily allow.
#
#   2. The Leptos frontend (`geleit-ui`) must not depend on ANY of our engine crates. It reaches the
#      engine ONLY over the Tauri IPC seam (`geleit-app::ipc`). This is the one that actually bites:
#      nothing in Cargo stops a component from `use geleit_store::...` and querying the database
#      straight from view code, and once one does, the seam is decorative.
#
# Uses `cargo tree` only (no extra tooling). Output is captured before matching so that
# (a) a `cargo tree` failure aborts via `set -e` instead of being masked by the pipeline,
# and (b) SIGPIPE from `grep -q` cannot hide a real match (no pipeline in the `if`).
set -euo pipefail

# The hosts + the frontend: the engine-side crates must not depend on any of these.
UI_CRATES=("geleit-app" "geleit-server" "geleit-ui")
# The engine-side, UI-agnostic crates. geleit-host is the host-agnostic command core (ADR-0014): it
# holds logic both hosts share, so it must stay as free of any host/UI as the engine crates it sits on.
ENGINE_CRATES=("geleit-core" "geleit-platform" "geleit-store" "geleit-engine" "geleit-host")

fail=0
deps_of() { cargo tree --package "$1" --edges normal --prefix none | awk '{print $1}'; }

# (1) no engine crate may depend on a UI crate
for crate in "${ENGINE_CRATES[@]}"; do
    names="$(deps_of "$crate")"
    for ui in "${UI_CRATES[@]}"; do
        if grep -qx "$ui" <<<"$names"; then
            echo "BOUNDARY VIOLATION: $crate depends on $ui" >&2
            fail=1
        fi
    done
done

# (2) the frontend may not depend on the engine — it talks IPC, nothing else
frontend_deps="$(deps_of geleit-ui)"
for engine in "${ENGINE_CRATES[@]}"; do
    if grep -qx "$engine" <<<"$frontend_deps"; then
        echo "BOUNDARY VIOLATION: geleit-ui depends on $engine — the frontend must reach the" >&2
        echo "                    engine only through the IPC seam (geleit-app::ipc), ADR-0012." >&2
        fail=1
    fi
done

if [ "$fail" -ne 0 ]; then
    exit 1
fi
echo "Boundary OK: engine crates are UI-agnostic, and geleit-ui reaches the engine only over IPC"
