#!/usr/bin/env bash
set -e

echo "=== 1. Installiere Claude Code & OpenAI Codex CLI ==="
npm install -g @anthropic-ai/claude-code @openai/codex

echo "=== 2. Installiere herdr-core (hc) ==="
curl -fsSL https://releases.herdr.dev/core/install.sh | sh

echo "=== 3. Richte SSH-Keys für Herdr ein ==="
mkdir -p ~/.ssh
chmod 700 ~/.ssh

if [ -d "/tmp/host-ssh" ]; then
    cat /tmp/host-ssh/*.pub > ~/.ssh/authorized_keys 2>/dev/null || true
    if [ -s ~/.ssh/authorized_keys ]; then
        chmod 600 ~/.ssh/authorized_keys
        echo "Öffentliche SSH-Keys vom Host wurden erfolgreich übernommen."
    else
        echo "Hinweis: Keine .pub Keys unter ~/.ssh auf dem Host gefunden."
    fi
fi

echo "=== Setup abgeschlossen! ==="
