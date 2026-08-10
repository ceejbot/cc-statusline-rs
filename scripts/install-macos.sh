#!/usr/bin/env bash
# Install cc-statusline-rs on macOS via Homebrew, then point Claude Code at it.
# This is sugar for two commands you can run yourself:
#     brew install ceejbot/tap/cc-statusline-rs
#     cc-statusline-rs setup
set -euo pipefail

if ! command -v brew >/dev/null 2>&1; then
    echo "Homebrew is required: see https://brew.sh" >&2
    exit 1
fi

brew install ceejbot/tap/cc-statusline-rs
"$(brew --prefix)/bin/cc-statusline-rs" setup
echo "done. Claude Code picks up the statusline on its next session."
