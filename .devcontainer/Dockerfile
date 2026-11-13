# Everything the container needs that does not depend on the workspace or the
# host; Docker caches each step, so a rebuild only redoes what changed.
FROM mcr.microsoft.com/devcontainers/rust:2.1.5-1-bookworm@sha256:4d944fc913385b14c0507bc4636749a773543067c2f5fc885b6fd4a42e59f006

# Pinned so every rebuild gets the same tools; bump deliberately.
ARG NODE_MAJOR=26
ARG CLAUDE_CODE_VERSION=2.1.322
ARG CODEX_VERSION=0.167.2

# Cross-check tools (Windows target, Xvfb, Wine) and shell tools.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      gcc-mingw-w64-x86-64 xvfb wine64 fzf bat ripgrep \
 && rm -rf /var/lib/apt/lists/* \
 && ln -s /usr/bin/batcat /usr/local/bin/bat

RUN curl -fsSL "https://deb.nodesource.com/setup_${NODE_MAJOR}.x" | bash - \
 && apt-get install -y --no-install-recommends nodejs \
 && rm -rf /var/lib/apt/lists/* \
 && npm install -g "@anthropic-ai/claude-code@${CLAUDE_CODE_VERSION}" "@openai/codex@${CODEX_VERSION}"

RUN curl -fsSL https://starship.rs/install.sh | sh -s -- --yes \
 && chsh -s /usr/bin/zsh vscode

# The sshd feature installs the server; this only allows key logins.
RUN mkdir -p /etc/ssh/sshd_config.d \
 && printf 'PasswordAuthentication no\nKbdInteractiveAuthentication no\nPermitRootLogin no\n' \
      > /etc/ssh/sshd_config.d/10-keys-only.conf

USER vscode
# rust-toolchain.toml picks the toolchain; add the targets this repo checks.
COPY rust-toolchain.toml /tmp/toolchain/
RUN cd /tmp/toolchain \
 && rustup toolchain install \
 && rustup target add x86_64-pc-windows-gnu aarch64-apple-darwin

RUN curl -fsSL https://releases.herdr.dev/core/install.sh | sh \
 && echo 'eval "$(starship init zsh)"' >> ~/.zshrc
USER root
