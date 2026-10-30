# Shared trial image: pinned Codex CLI with the exported Craft packages installed into the
# Codex home Harbor uses. Build from the repository root after exporting Codex with the
# native runtime for linux x86_64 (see evals/README.md).
FROM node:24.21.0-trixie-slim@sha256:173f125896c3b47ddf056734c7ea789d04595a6a08769a8f78e0df642781fb66

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl git jq ripgrep \
 && rm -rf /var/lib/apt/lists/*

ARG CODEX_VERSION=0.165.0
RUN npm install -g "@openai/codex@${CODEX_VERSION}" && codex --version

# Harbor runs Codex with this home and replaces config.toml from evals/codex/*.toml; the
# installed plugin cache stays and loads only when that configuration enables it.
ENV CODEX_HOME=/installed-agent/codex-home
COPY dist/codex /opt/craft/codex
RUN mkdir -p "$CODEX_HOME" \
 && printf '[marketplaces.engineering-pack]\nsource_type = "local"\nsource = "/opt/craft/codex"\n' >"$CODEX_HOME/config.toml" \
 && for package in craft-delivery craft-engineering craft-review craft-stories; do codex plugin add "$package@engineering-pack"; done \
 && rm "$CODEX_HOME/config.toml" \
 && test -x /opt/craft/codex/packages/craft-delivery/support/native-bin/x86_64-unknown-linux-gnu/native-bin/craft

RUN git config --system user.name "Eval Fixture" \
 && git config --system user.email "fixture@example.invalid" \
 && git config --system init.defaultBranch main
