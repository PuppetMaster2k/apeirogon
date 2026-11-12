#!/usr/bin/env bash
# Authorises host SSH keys for Herdr. HERDR_SSH_KEY names one public key file
# in the host's ~/.ssh (e.g. id_ed25519.pub); unset, all *.pub keys are used.
set -euo pipefail

mkdir -p ~/.ssh
chmod 700 ~/.ssh

if [ ! -d /tmp/host-ssh ]; then
    echo "No host ~/.ssh mounted; skipping SSH keys."
    exit 0
fi

if [ -n "${HERDR_SSH_KEY:-}" ]; then
    keys=("/tmp/host-ssh/${HERDR_SSH_KEY}")
else
    keys=(/tmp/host-ssh/*.pub)
fi

cat "${keys[@]}" > ~/.ssh/authorized_keys 2>/dev/null || true
if [ -s ~/.ssh/authorized_keys ]; then
    chmod 600 ~/.ssh/authorized_keys
    echo "Authorised ${#keys[@]} host SSH key file(s) for Herdr."
else
    rm -f ~/.ssh/authorized_keys
    echo "Note: no public SSH key found in the host's ~/.ssh." >&2
fi
