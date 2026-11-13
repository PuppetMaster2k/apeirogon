#!/usr/bin/env bash
# Runs once when the container is created. Tools live in the Dockerfile; this
# only fixes what exists at runtime: volumes are mounted owned by root.
set -euo pipefail

sudo chown -R vscode: /home/vscode/.claude /home/vscode/.codex /home/vscode/.config/herdr /home/vscode/target /usr/local/cargo/registry /usr/local/cargo/git
