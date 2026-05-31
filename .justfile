# Build release binary
build:
    @cargo build --release

# Quick type/syntax check
check:
    @cargo check

# Run all unit tests
test:
    @cargo test

# Format code
fmt:
    @cargo fmt

# Run clippy lints
lint:
    @cargo clippy

# Run all CI checks: formatting, lints, tests
ci:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "checking formatting..."
    cargo fmt --check
    echo "running clippy..."
    cargo clippy -- -D warnings
    echo "running tests..."
    cargo test
    echo "all checks passed."

# Run with test.json for quick manual testing
run:
    @cargo run < test.json

# Clean build artifacts
clean:
    @cargo clean

# Tag a new version for release.
version BUMP:
    #!/usr/bin/env bash
    set -e
    current=$(tomato get package.version Cargo.toml)
    version=$(semver-bump {{ BUMP }} $current)
    tomato set package.version "$version" Cargo.toml &> /dev/null
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
    settings=~/.claude/settings.json
    if [[ -f "$settings" ]]; then
        tmp=$(mktemp)
        jq '.statusLine = {"type": "command", "command": "~/.claude/cc-statusline-rs"}' "$settings" > "$tmp" \
            && mv "$tmp" "$settings"
        echo "updated $settings"
    else
        echo '{"statusLine": {"type": "command", "command": "~/.claude/cc-statusline-rs"}}' > "$settings"
        echo "created $settings"
    fi
    echo "installed to ~/.claude/cc-statusline-rs"

# Install tools
setup:
    brew tap ceejbot/homebrew-tap
    brew install tomato semver-bump jq

RESET := "\\e[0m"
BOLD_YELLOW := "\\e[1;33m"
