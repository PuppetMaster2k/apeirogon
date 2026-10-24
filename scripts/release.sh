#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail

plan() {
    local tag base since=0 range=HEAD milestones subject features='' fixes=''
    local major minor patch version
    tag=$(git describe --tags --abbrev=0 --match 'v[0-9]*.[0-9]*.[0-9]*' 2>/dev/null || true)
    if [[ -n "$tag" ]]; then
        [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Invalid release tag: $tag" >&2; return 1; }
        base=${tag#v}
        since=$(git show -s --format=%ct "$tag^{commit}")
        range="$tag..HEAD"
    else
        base=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"])')
    fi
    if [[ ${MILESTONES_CLOSED_AT+x} ]]; then
        milestones=$(printf '%s' "$MILESTONES_CLOSED_AT" | jq -Rn '[inputs | select(length > 0) | {closed_at: ., title: .}]')
    else
        milestones=$(gh api --paginate --slurp 'repos/{owner}/{repo}/milestones?state=closed' | jq 'add')
    fi
    milestones=$(printf '%s' "$milestones" | jq -r --argjson since "$since" \
        '.[] | select((.closed_at | fromdateiso8601) > $since) | "- " + .title')
    while IFS= read -r subject; do
        [[ "$subject" == 'chore: release '* ]] && continue
        if [[ "$subject" =~ ^feat(\([^\)]*\))?!?: ]]; then
            features+="- $subject"$'\n'
        elif [[ "$subject" =~ ^(fix|perf)(\([^\)]*\))?!?: ]]; then
            fixes+="- $subject"$'\n'
        fi
    done < <(git log --format=%s "$range")
    [[ "$base" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Invalid base version: $base" >&2; return 1; }
    IFS=. read -r major minor patch <<< "$base"
    if [[ -n "$milestones" ]]; then
        version="$((major + 1)).0.0"
    elif [[ -n "$features" ]]; then
        version="$major.$((minor + 1)).0"
    elif [[ -n "$fixes" ]]; then
        version="$major.$minor.$((patch + 1))"
    else
        return 0
    fi
    printf 'version=%s\n\n' "$version"
    [[ -z "$features" ]] || printf '### Features\n\n%s\n' "$features"
    [[ -z "$fixes" ]] || printf '### Fixes\n\n%s\n' "$fixes"
    [[ -z "$milestones" ]] || printf '### Milestones\n\n%s\n\n' "$milestones"
    return 0
}

apply() {
    local version=$1 notes=$2
    [[ "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || { echo 'Expected X.Y.Z' >&2; return 1; }
    [[ -f "$notes" ]] || { echo "Missing notes: $notes" >&2; return 1; }
    python3 - "$version" <<'PY'
import pathlib
import re
import sys

path = pathlib.Path('Cargo.toml')
text, count = re.subn(r'(\[workspace\.package\]\s*\n(?:(?!\[)[^\n]*\n)*?version\s*=\s*)"[^"]+"',
                      lambda match: match[1] + '"' + sys.argv[1] + '"', path.read_text())
if count != 1:
    sys.exit('Expected one workspace.package version')
path.write_text(text)
PY
    cargo update --workspace
    python3 - "$version" "$notes" <<'PY'
import datetime
import pathlib
import sys

path = pathlib.Path('CHANGELOG.md')
old = path.read_text() if path.exists() else '# Changelog\n'
header, separator, body = old.partition('\n')
if header != '# Changelog':
    sys.exit('Expected # Changelog header')
notes = pathlib.Path(sys.argv[2]).read_text().strip()
date = datetime.datetime.now(datetime.timezone.utc).date()
path.write_text(f'# Changelog\n\n## {sys.argv[1]} - {date}\n\n{notes}\n\n' + body.lstrip())
PY
}

case "${1:-}" in
    plan) [[ $# == 1 ]] && plan ;;
    apply) [[ $# == 3 ]] && apply "$2" "$3" ;;
    *) echo 'Usage: release.sh plan | apply X.Y.Z NOTES_FILE' >&2; exit 1 ;;
esac
