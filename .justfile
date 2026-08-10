# list recipes
_help:
    just --list

# Run clippy lints
@clippy:
    cargo clippy --all-targets -- -D warnings

# Run all unit tests
@test:
    cargo nextest run --workspace --all-targets --future-incompat-report

# Format code
@fmt:
    cargo +nightly fmt

# Run all CI checks: formatting, lints, tests
ci: test clippy
    cargo +nightly fmt --check

# Run with test.json for quick manual testing
run:
    @cargo run < test.json

# Tag a new version for release.
version BUMP:
    #!/usr/bin/env bash
    set -e
    current=$(tomato get package.version Cargo.toml)
    version=$(semver-bump {{ BUMP }} $current)
    tomato set package.version "$version" Cargo.toml &> /dev/null
    tomato set version "$version" .formulaic.toml &> /dev/null
    cargo generate-lockfile
    git commit Cargo.toml Cargo.lock -m "v${version}"
    git tag "v${version}"
    printf "Release tagged for version {{ BOLD_YELLOW }}v${version}{{ RESET }}\n"

# Build, sign, install to ~/.claude/, and configure settings.json
install:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --release
    mkdir -p ~/.claude
    cp target/release/cc-statusline-rs ~/.claude/cc-statusline-rs
    chmod +x ~/.claude/cc-statusline-rs
    xattr -cr ~/.claude/cc-statusline-rs
    codesign -fs - ~/.claude/cc-statusline-rs
    ~/.claude/cc-statusline-rs setup
    echo "installed to ~/.claude/cc-statusline-rs"

# Install tools
setup:
    brew tap ceejbot/homebrew-tap
    brew install tomato semver-bump

RESET := "\\e[0m"
BOLD_YELLOW := "\\e[1;33m"
