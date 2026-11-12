#!/usr/bin/env bash
# Runs once when the container is created (and in prebuilds): tools only.
set -euo pipefail

# Pinned so every rebuild gets the same tools; bump deliberately.
CLAUDE_CODE_VERSION="2.1.322"
CODEX_VERSION="0.167.2"
HERDR_INSTALL_URL="https://releases.herdr.dev/core/install.sh"

echo "=== Volumes: owned by vscode ==="
sudo chown -R vscode: /home/vscode/target /usr/local/cargo/registry /usr/local/cargo/git

echo "=== Cross-check tools: Windows target, Xvfb, Wine ==="
sudo apt-get update
sudo apt-get install -y --no-install-recommends gcc-mingw-w64-x86-64 xvfb wine64
# rust-toolchain.toml selects the toolchain; add the targets this repo checks.
rustup target add x86_64-pc-windows-gnu aarch64-apple-darwin

echo "=== Claude Code and Codex CLI ==="
npm install -g "@anthropic-ai/claude-code@${CLAUDE_CODE_VERSION}" "@openai/codex@${CODEX_VERSION}"

echo "=== herdr-core (hc) ==="
installer="$(mktemp)"
curl -fsSL "$HERDR_INSTALL_URL" -o "$installer"
if [ -n "${HERDR_INSTALL_SHA256:-}" ]; then
    echo "${HERDR_INSTALL_SHA256}  ${installer}" | sha256sum --check --quiet
else
    echo "Warning: HERDR_INSTALL_SHA256 is not set; the herdr installer is not verified." >&2
    echo "         Its SHA-256 is $(sha256sum "$installer" | cut -d' ' -f1)" >&2
fi
sh "$installer"
rm -f "$installer"

echo "=== SSH: keys only ==="
printf 'PasswordAuthentication no\nKbdInteractiveAuthentication no\nPermitRootLogin no\n' \
    | sudo tee /etc/ssh/sshd_config.d/10-keys-only.conf >/dev/null
