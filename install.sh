#!/bin/sh
# solenv installer — downloads a prebuilt binary from the GitHub Releases,
# verifies its SHA-256 against the release's SHA256SUMS, and installs it.
#
#   curl -fsSL https://raw.githubusercontent.com/AkashJana18/solenv/master/install.sh | bash
#
# Environment overrides:
#   SOLENV_VER      tag to install instead of latest          (e.g. v0.1.2)
#   SOLENV_PREFIX   install directory                         (default ~/.local/bin)
#   SOLENV_BASE     full download base URL                    (default GitHub Releases)
set -eu

die() {
    echo "error: $*" >&2
    exit 1
}

REPO="AkashJana18/solenv"
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Darwin) os=darwin ;;
    Linux)  os=linux ;;
    *) die "unsupported OS '$OS' (expected Darwin or Linux)" ;;
esac

case "$ARCH" in
    x86_64)        arch="x86_64" ;;
    arm64|aarch64) arch="aarch64" ;;
    *) die "unsupported architecture '$ARCH'" ;;
esac

case "$os-$arch" in
    darwin-aarch64) TRIPLE="aarch64-apple-darwin" ;;
    darwin-x86_64)  TRIPLE="x86_64-apple-darwin" ;;
    linux-x86_64)   TRIPLE="x86_64-unknown-linux-gnu" ;;
    linux-aarch64)  TRIPLE="aarch64-unknown-linux-gnu" ;;
esac

VER="${SOLENV_VER:-latest}"
if [ -n "${SOLENV_BASE:-}" ]; then
    BASE="$SOLENV_BASE"
elif [ "$VER" = "latest" ]; then
    BASE="https://github.com/$REPO/releases/latest/download"
else
    BASE="https://github.com/$REPO/releases/download/$VER"
fi

BIN_URL="$BASE/solenv-$TRIPLE"
SUMS_URL="$BASE/SHA256SUMS"

command -v curl >/dev/null 2>&1 || die "this installer requires 'curl'"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Downloading solenv-$TRIPLE ($VER) ..."
curl -fsSL "$BIN_URL" -o "$TMP/solenv" || die "failed to download $BIN_URL"
curl -fsSL "$SUMS_URL" -o "$TMP/SHA256SUMS" || die "failed to download $SUMS_URL (missing SHA256SUMS on this release?)"

if command -v sha256sum >/dev/null 2>&1; then
    LOCAL="$(sha256sum "$TMP/solenv" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
    LOCAL="$(shasum -a 256 "$TMP/solenv" | awk '{print $1}')"
else
    die "neither 'sha256sum' nor 'shasum' found"
fi

EXPECTED="$(awk -v f="solenv-$TRIPLE" '$2 == f {print $1; exit}' "$TMP/SHA256SUMS")"
[ -z "$EXPECTED" ] && die "no checksum found for solenv-$TRIPLE in SHA256SUMS"
[ "$LOCAL" != "$EXPECTED" ] && die "checksum mismatch for solenv-$TRIPLE (aborting)"
echo "Checksum verified ($LOCAL)"

INSTALL_DIR="${SOLENV_PREFIX:-$HOME/.local/bin}"
mkdir -p "$INSTALL_DIR"
install -m 0755 "$TMP/solenv" "$INSTALL_DIR/solenv"
echo "Installed solenv to $INSTALL_DIR/solenv"
"$INSTALL_DIR/solenv" --version

case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *) echo "NOTE: '$INSTALL_DIR' is not in your PATH. Add this to your shell profile:"
       echo "    export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
esac