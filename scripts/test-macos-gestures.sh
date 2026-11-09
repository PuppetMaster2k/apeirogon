#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later

set -u
umask 077
A7N=${A7N:-a7n}
SAMPLE_SECONDS=8
LOG_LINES=1000000
FAILED=0
REPORT="./gesture-test-$(date -u +%Y%m%dT%H%M%SZ).md"
if ! (set -C; printf '# macOS gesture test\n\n' > "$REPORT"); then
    printf 'FAIL: cannot create a new report in this directory.\n' >&2
    exit 1
fi

record() {
    printf '%s: %s\n' "$1" "$2"
    if ! printf -- '- **%s:** %s\n' "$1" "$2" >> "$REPORT"; then
        printf 'FAIL: cannot write the report.\n' >&2
        exit 1
    fi
}

finish() {
    if [ "$FAILED" -eq 0 ]; then record Result PASS; else record Result FAIL; fi
    printf 'Report: %s\n' "$REPORT"
    exit "$FAILED"
}

blocker() {
    FAILED=1
    record Blocker "$1"
    finish
}
trap 'blocker "Test interrupted; remaining steps were not run."' INT TERM

# Read the entire capped log, rather than the CLI's default 50-line tail.
log_snapshot() {
    local logs count first
    logs=$("$A7N" logs -n "$LOG_LINES" 2>/dev/null) || return 1
    if [ -z "$logs" ]; then printf '0 empty\n'; return; fi
    count=$(printf '%s\n' "$logs" | awk '
        /(^|[[:space:]])kind=gesture_swipe([[:space:]]|$)/ { count++ }
        END { print count+0 }')
    # The first-line checksum detects log rotation between snapshots.
    first=$(printf '%s\n' "$logs" | sed -n '1p' | cksum)
    printf '%s %s\n' "$count" "$first"
}

measure() {
    local label=$1 expected=$2 before after before_count after_count before_first after_first
    printf '\n%s\n' "$label"
    printf 'Return input home to this Mac. Press Enter when ready: '
    IFS= read -r _ || blocker 'Input closed before the test finished.'
    before=$(log_snapshot) || blocker 'Cannot read a7n logs; no delivery verdict is possible.'
    read -r before_count before_first <<< "$before"
    printf 'Now drive Windows and perform the step within %s seconds; return home afterward.\n' "$SAMPLE_SECONDS"
    sleep "$SAMPLE_SECONDS" || blocker 'Observation window interrupted.'
    after=$(log_snapshot) || blocker 'Cannot read a7n logs; no delivery verdict is possible.'
    read -r after_count after_first <<< "$after"
    if { [ "$before_first" != empty ] && [ "$before_first" != "$after_first" ]; } ||
        [ "$after_count" -lt "$before_count" ]; then
        blocker 'Log rotated or was truncated during the step; rerun the guide.'
    fi
    NEW_LINES=$((after_count - before_count))
    if [ "$NEW_LINES" -eq "$expected" ]; then
        record "$label" "PASS ($NEW_LINES new gesture lines; expected $expected)"
    else
        FAILED=1
        record "$label" "FAIL ($NEW_LINES new gesture lines; expected $expected)"
    fi
}

judge() {
    local answer
    while :; do
        printf '\n%s [y/n]: ' "$2"
        IFS= read -r answer || blocker 'Input closed before the test finished.'
        case "$answer" in
            y|Y) record "$1" PASS; return ;;
            n|N) FAILED=1; record "$1" FAIL; return ;;
            *) printf 'Please answer y or n.\n' ;;
        esac
    done
}

[ "$(uname -s)" = Darwin ] || blocker 'Run this guide on macOS (Darwin).'
command -v "$A7N" >/dev/null 2>&1 || blocker 'a7n is not executable; put it on PATH or set A7N=/path/to/a7n.'
VERSION=$("$A7N" --version 2>/dev/null) || blocker 'Cannot run a7n --version.'
record 'a7n version' "$VERSION"
STATUS=$("$A7N" status 2>/dev/null) || blocker 'Daemon unreachable through a7n status; start it yourself before retrying.'

# Match the wording printed by a7n status without exposing peer identities.
case "$STATUS" in
    *'permission missing:'*) blocker 'a7n status reports missing input permission; grant Accessibility and Input Monitoring before retrying.' ;;
    *'input: unavailable'*) blocker 'a7n status reports input unavailable; resolve it before retrying.' ;;
esac
case "$STATUS" in
    *'input sharing: paused'*) blocker 'Input sharing is paused; resume it yourself before retrying.' ;;
esac
record Preflight PASS
record macOS "$(sw_vers -productVersion 2>/dev/null || printf unknown)"
record 'macOS build' "$(sw_vers -buildVersion 2>/dev/null || printf unknown)"
record 'Hardware model' "$(sysctl -n hw.model 2>/dev/null || printf unknown)"
for KEY in TrackpadThreeFingerHorizSwipeGesture TrackpadThreeFingerVertSwipeGesture \
    TrackpadFourFingerHorizSwipeGesture TrackpadFourFingerVertSwipeGesture; do
    VALUE=$(defaults read com.apple.AppleMultitouchTrackpad "$KEY" 2>/dev/null) || VALUE=unknown
    record "$KEY" "${VALUE:-unknown}"
done
log_snapshot >/dev/null || blocker 'Cannot read a7n logs; fix log access before retrying.'
printf '\nUse this branch build on both peers, paired with Windows and its default gesture bindings.\n'
printf 'Each Enter arms an %s-second window: switch input to Windows, do only the named action, then return home.\n' "$SAMPLE_SECONDS"
printf 'Use the requested finger count; enable the matching system swipe setting yourself if needed.\n'
printf 'Lift all fingers before each step. The guide never changes settings or restarts the daemon.\n'
for FINGERS in three four; do
    for DIRECTION in up down left right; do
        measure "Delivery: $FINGERS fingers, one swipe $DIRECTION" 1
        if [ "$NEW_LINES" -eq 0 ]; then
            record Verdict 'macOS does not deliver touches to the event tap'
            printf 'Stop: attach this report to #97, including OS/build, hardware, swipe settings, finger count and failed direction.\n'
            printf 'Also state whether the daemon ran by hand or at login; do not attach raw logs or peer identities.\n'
            finish
        fi
    done
done
measure 'Exactly once: swipe past threshold, hold fingers down and reverse, then lift' 1
for ACTION in 'two-finger scroll' pinch spread 'short three/four-finger movement'; do
    measure "No false triggers: $ACTION" 0
done
judge 'Windows actions' 'Did every up/down/left/right swipe perform Task View/show desktop/previous desktop/next desktop respectively?'
judge 'Mac suppression' 'While driving Windows, did the Mac avoid switching Spaces, opening Mission Control, or showing its desktop?'
printf '\nReturn input home and try native macOS swipes before answering.\n'
judge 'Native swipes at home' 'Do native macOS swipes still work at home?'
printf '\nDrive Windows, start a swipe, return home mid-swipe using your usual home trigger, and release all fingers.\n'
judge 'Mid-swipe return' 'After returning home mid-swipe, are all keys and modifiers released?'
finish
