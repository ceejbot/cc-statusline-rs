#!/usr/bin/env bash
# Install cc-statusline-rs on Linux: download the static musl build for this
# architecture, verify it, drop it in ~/.claude/, and point Claude Code at it.
set -euo pipefail

REPO="ceejbot/cc-statusline-rs"
BINARY="cc-statusline-rs"
BASE_URL="${CC_STATUSLINE_BASE_URL:-https://github.com/$REPO/releases/latest/download}"

case "$(uname -m)" in
    x86_64 | amd64) arch="x86_64" ;;
    aarch64 | arm64) arch="aarch64" ;;
    *)
        echo "unsupported architecture: $(uname -m)" >&2
        exit 1
        ;;
esac
# musl builds are fully static, so they run on any distro.
target="${arch}-unknown-linux-musl"
tarball="$BINARY-$target.tar.gz"

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT
cd "$workdir"

echo "downloading $tarball ..."
curl -fsSL -O "$BASE_URL/$tarball"
curl -fsSL -O "$BASE_URL/$tarball.sha256"

# The .sha256 asset holds the bare hex digest; sha256sum -c wants "digest  file".
echo "$(cat "$tarball.sha256")  $tarball" | sha256sum -c - >/dev/null

tar xzf "$tarball"
chmod +x "$BINARY"
mkdir -p "$HOME/.claude"
# mv, not cp: renaming replaces the directory entry even while a running
# statusline holds the old inode open; cp would fail with ETXTBSY.
mv -f "$BINARY" "$HOME/.claude/$BINARY"

"$HOME/.claude/$BINARY" setup
echo "done. Claude Code picks up the statusline on its next session."
