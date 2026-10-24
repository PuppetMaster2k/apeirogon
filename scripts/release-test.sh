#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail

release_script=$(cd "$(dirname "$0")" && pwd)/release.sh
temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export MILESTONES_CLOSED_AT=''
export GIT_AUTHOR_DATE=2025-01-01T00:00:00Z GIT_COMMITTER_DATE=2025-01-01T00:00:00Z
cd "$temporary"
git init -q
git config user.name 'Release test'
git config user.email 'release-test@example.invalid'
printf '[workspace.package]\nversion = "0.1.0"\n' > Cargo.toml
git add Cargo.toml
git commit -qm 'chore: initial workspace'

assert_version() {
    local expected=$1 actual
    actual=$("$release_script" plan)
    if [[ -z "$expected" ]]; then
        [[ -z "$actual" ]]
    else
        [[ "$actual" == "version=$expected"$'\n\n'* ]]
    fi
}

# Arrange: no release tag, and only maintenance commits.
# Act / Assert: maintenance produces no output.
assert_version ''

# Arrange
git commit --allow-empty -qm 'fix(core): handle disconnect (#1)'
# Act / Assert: the workspace version is the initial base.
assert_version 0.1.1
git tag v0.1.1

# Arrange
git commit --allow-empty -qm 'perf(core): reduce allocations (#2)'
# Act / Assert
assert_version 0.1.2
git tag v0.1.2

# Arrange
git commit --allow-empty -qm 'feat(core): share input (#3)'
git commit --allow-empty -qm 'feat: share clipboard (#4)'
# Act
notes=$("$release_script" plan)
# Assert: features win over fixes; subjects retain PRs, newest first.
[[ "$notes" == "version=0.2.0"$'\n\n### Features\n\n- feat: share clipboard (#4)\n- feat(core): share input (#3)' ]]
MILESTONES_CLOSED_AT=2025-01-02T00:00:00Z
assert_version 1.0.0
MILESTONES_CLOSED_AT=''
git tag v0.2.0

# Arrange
git commit --allow-empty -qm 'feat(core)!: change input representation (#5)'
# Act / Assert: breaking markers do not select a major release.
assert_version 0.3.0
git tag v0.3.0
git commit --allow-empty -qm 'feat!: replace clipboard representation (#6)'
assert_version 0.4.0
git tag v0.4.0

# Arrange
git commit --allow-empty -qm 'chore: release 0.4.0'
MILESTONES_CLOSED_AT=2024-12-31T23:59:59Z
# Act / Assert: old closures and release commits are ignored.
assert_version ''
MILESTONES_CLOSED_AT=2025-01-01T00:00:00Z
assert_version ''
MILESTONES_CLOSED_AT=2025-01-02T00:00:00Z
# Act / Assert: a new milestone closure alone selects major.
assert_version 1.0.0
[[ $("$release_script" plan) == *$'### Milestones\n\n- 2025-01-02T00:00:00Z' ]]

# Arrange: exercise apply with a real, dependency-free Cargo workspace.
printf '[workspace]\nresolver = "2"\nmembers = ["app"]\n[workspace.package]\nversion = "0.1.0"\nedition = "2024"\n' > Cargo.toml
mkdir -p app/src
printf '[package]\nname = "release-test"\nversion.workspace = true\nedition.workspace = true\n' > app/Cargo.toml
printf 'fn main() {}\n' > app/src/main.rs
printf '# Changelog\n\n## 0.1.0 - 2025-01-01\n\nOld notes.\n' > CHANGELOG.md
printf '### Fixes\n\n- fix: handle disconnect (#1)\n' > notes.md
# Act
"$release_script" apply 0.1.1 notes.md
# Assert: metadata, lockfile, notes, and previous entries agree.
python3 - <<'PY'
import datetime
import pathlib
import tomllib

assert tomllib.loads(pathlib.Path('Cargo.toml').read_text())['workspace']['package']['version'] == '0.1.1'
assert tomllib.loads(pathlib.Path('Cargo.lock').read_text())['package'][0]['version'] == '0.1.1'
date = datetime.datetime.now(datetime.timezone.utc).date()
assert pathlib.Path('CHANGELOG.md').read_text() == f'''# Changelog

## 0.1.1 - {date}

### Fixes

- fix: handle disconnect (#1)

## 0.1.0 - 2025-01-01

Old notes.
'''
PY
printf 'Release self-check passed.\n'
