#!/usr/bin/env bash
# Drive the S0.2 spike under strace and capture the evidence.
#
# Each adversarial vector targets a distinct 203.0.113.x host so connects are attributable:
#   .5 tracking pixel · .6 remote image · .7 INLINE SCRIPT (JS-execution oracle) ·
#   .8 onerror handler · .9/.13 interaction-only (should not fire) · .10/.11/.12 css/font/bg
# Gate:
#   RAW       → must include 203.0.113.7  (proves JS executes → removing it is load-bearing)
#   SANITIZED → no 203.0.113.x at all, and 0 non-loopback connects
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="$HERE/target/debug/s0_2_html_render"
FIX="$HERE/fixtures"
EVID="$HERE/evidence"
mkdir -p "$EVID"
export DISPLAY="${DISPLAY:-:0}"

distinct_remote_hosts() {
    grep -E 'connect\(' "$1" 2>/dev/null | grep -oE '203\.0\.113\.[0-9]+' | sort -u | tr '\n' ' '
}
nonloopback_count() {
    grep -E 'connect\(' "$1" 2>/dev/null | grep -E 'AF_INET' \
        | grep -vE 'inet_addr\("(127\.|0\.0\.0\.0)' | grep -vE '"::1"' | wc -l
}

strace_run() {
    local name="$1" fixture="$2" mode="$3" log="$EVID/$1.strace"
    strace -f -e trace=connect -o "$log" "$BIN" "$fixture" "$mode" 2>"$EVID/$name.stderr" || true
    distinct_remote_hosts "$log" >"$EVID/$name.hosts"
    nonloopback_count "$log" >"$EVID/$name.nonloopback"
    local js_engine email_script
    grep -q "init-script-ran" "$EVID/$name.stderr" && js_engine="on" || js_engine="off"
    grep -q "PAGE-SCRIPT-RAN" "$EVID/$name.stderr" && email_script="RAN" || email_script="blocked"
    echo "$name: remote hosts=[$(cat "$EVID/$name.hosts")] non-loopback=$(cat "$EVID/$name.nonloopback") js_engine=$js_engine email_script=$email_script"
}

render_only() {
    local name="$1" fixture="$2" mode="$3"
    "$BIN" "$fixture" "$mode" 2>"$EVID/$name.render.stderr" || true
    if grep -q "auto-exit" "$EVID/$name.render.stderr"; then
        echo "$name: rendered + clean exit OK"
    else
        echo "$name: NO clean exit"
    fi
}

echo "== adversarial (strace) =="
strace_run adversarial-raw "$FIX/adversarial.html" --raw
strace_run adversarial-sanitized "$FIX/adversarial.html" --sanitize

echo "== fidelity corpus (render-without-error) =="
render_only newsletter "$FIX/newsletter.html" --sanitize
render_only receipt "$FIX/receipt.html" --sanitize
render_only multipart "$FIX/multipart.html" --sanitize

echo
echo "== SUMMARY =="
echo "RAW       remote hosts: [$(cat "$EVID/adversarial-raw.hosts")]"
echo "          → includes 203.0.113.7 ? $(grep -q '203.0.113.7' "$EVID/adversarial-raw.hosts" && echo 'YES (JS executed)' || echo 'no')"
echo "SANITIZED remote hosts: [$(cat "$EVID/adversarial-sanitized.hosts")]   (expect empty)"
echo "SANITIZED non-loopback connects: $(cat "$EVID/adversarial-sanitized.nonloopback")   (expect 0)"
echo
echo "JS oracle (init-script-ran = engine on; PAGE-SCRIPT-RAN = email's script executed):"
echo "  RAW       → $(grep -q PAGE-SCRIPT-RAN "$EVID/adversarial-raw.stderr" && echo 'email script RAN (threat real)' || echo 'email script did not run')"
echo "  SANITIZED → $(grep -q PAGE-SCRIPT-RAN "$EVID/adversarial-sanitized.stderr" && echo 'email script RAN (BAD)' || echo 'email script BLOCKED') ; engine still $(grep -q init-script-ran "$EVID/adversarial-sanitized.stderr" && echo on || echo off)"
