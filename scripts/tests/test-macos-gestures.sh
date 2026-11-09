#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later

# Check verdicts and privacy without macOS, a daemon, or real input.
set -eu
SCRIPT=$(cd "$(dirname "$0")/.." && pwd)/test-macos-gestures.sh
FIXTURE=$(mktemp -d)
trap 'rm -rf "$FIXTURE"' EXIT
mkdir "$FIXTURE/bin"
cat > "$FIXTURE/bin/mock" <<'MOCK'
#!/bin/bash
set -eu
case "${0##*/}" in
    uname) printf '%s\n' "${TEST_OS:-Darwin}" ;;
    sw_vers) printf 'test-version\n' ;;
    sysctl) printf 'test-model\n' ;;
    defaults)
        [ "${1:-}" = read ] || exit 99
        exit 1 ;;
    sleep)
        read -r step < step
        step=$((step + 1))
        printf '%s\n' "$step" > step
        read -r count < count
        increment=1
        [ "$step" -le 9 ] || increment=0
        case "$TEST_MODE:$step" in
            zero:1) increment=0 ;;
            duplicate:1) increment=2 ;;
            false_trigger:10) increment=1 ;;
        esac
        printf '%s\n' "$((count + increment))" > count ;;
    a7n)
        case "${1:-}" in
            --version) printf 'a7n test-version\n' ;;
            status)
                [ "$TEST_MODE" != unreachable ] || exit 1
                printf 'PRIVATE_PEER PRIVATE_MACHINE (this machine)\n'
                if [ "$TEST_MODE" = permission ]; then printf 'permission missing: Input Monitoring\n'; fi ;;
            logs)
                [ "$TEST_MODE" != logs ] || exit 1
                if [ "$TEST_MODE" = rotation ] && [ "$(cat step)" -gt 0 ]; then
                    printf 'kind=replaced\n'
                else printf 'kind=start\n'; fi
                read -r count < count
                for ((i=0; i<count; i++)); do printf 'kind=gesture_swipe\n'; done ;;
            *) exit 99 ;;
        esac ;;
    *) exit 99 ;;
esac
MOCK
chmod +x "$FIXTURE/bin/mock"
for TOOL in uname sw_vers sysctl defaults sleep a7n; do
    ln -s mock "$FIXTURE/bin/$TOOL"
done

for MODE in pass zero duplicate false_trigger permission unreachable logs rotation wrong_os human_no; do
    mkdir "$FIXTURE/$MODE"
    (
        cd "$FIXTURE/$MODE"
        printf '27\n' > count
        printf '0\n' > step
        for ((i=0; i<13; i++)); do printf '\n'; done > answers
        if [ "$MODE" = human_no ]; then printf 'n\ny\ny\ny\n'; else printf 'y\ny\ny\ny\n'; fi >> answers
        export TEST_MODE="$MODE" TEST_OS=Darwin
        [ "$MODE" != wrong_os ] || TEST_OS=Linux
        export PATH="$FIXTURE/bin:$PATH" A7N="$FIXTURE/bin/a7n"
        result=0
        "$SCRIPT" < answers > output 2>&1 || result=$?
        if [ "$MODE" = pass ]; then [ "$result" -eq 0 ]; else [ "$result" -ne 0 ]; fi
        reports=(gesture-test-*.md)
        [ "${#reports[@]}" -eq 1 ]
        [ -f "${reports[0]}" ]
        if grep -Eq 'PRIVATE_PEER|PRIVATE_MACHINE|kind=gesture_swipe' output "${reports[0]}"; then exit 1; fi
        if [ "$MODE" = pass ]; then
            grep -q '\*\*Result:\*\* PASS' "${reports[0]}"
            grep -q 'TrackpadFourFingerVertSwipeGesture:\*\* unknown' "${reports[0]}"
        else
            grep -q '\*\*Result:\*\* FAIL' "${reports[0]}"
        fi
        if [ "$MODE" = rotation ]; then
            grep -q 'Log rotated or was truncated' output
            if grep -q 'macOS does not deliver touches' output; then exit 1; fi
        fi
        if [ "$MODE" = human_no ]; then
            grep -q '\*\*Windows actions:\*\* FAIL' "${reports[0]}"
        fi
        if [ "$MODE" = zero ]; then
            grep -q 'macOS does not deliver touches to the event tap' output
            [ "$(cat step)" -eq 1 ]
        fi
    )
    printf 'PASS: %s\n' "$MODE"
done
