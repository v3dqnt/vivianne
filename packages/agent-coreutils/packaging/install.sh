#!/bin/sh
# curl -fsSL https://github.com/agent-coreutils/agent-coreutils/releases/latest/download/install.sh | sh
# Installs the acu binary for this machine into ~/.local/bin (or $ACU_INSTALL_DIR).
# Template: written against the release layout in .github/workflows/release.yml, not yet tried on a real release.
set -eu
REPO="${ACU_REPO:-agent-coreutils/agent-coreutils}"
DIR="${ACU_INSTALL_DIR:-$HOME/.local/bin}"
os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
case "$arch" in
  x86_64 | amd64) arch=x64 ;;
  arm64 | aarch64) arch=arm64 ;;
  *) echo "unsupported architecture: $arch" >&2; exit 1 ;;
esac
case "$os" in
  darwin) key="darwin-$arch" ;;
  linux)
    libc=gnu
    if ldd --version 2>&1 | grep -qi musl; then libc=musl; fi
    key="linux-$arch-$libc" ;;
  *) echo "unsupported OS: $os (on Windows use: npm i -g agent-coreutils, or scoop)" >&2; exit 1 ;;
esac
url="https://github.com/$REPO/releases/latest/download/release-acu-$key"
mkdir -p "$DIR"
echo "downloading $url"
curl -fsSL "$url" -o "$DIR/acu.tmp"
chmod +x "$DIR/acu.tmp"
mv "$DIR/acu.tmp" "$DIR/acu"
echo "installed $("$DIR/acu" --version) to $DIR/acu"
case ":$PATH:" in
  *":$DIR:"*) ;;
  *) echo "note: add $DIR to your PATH" ;;
esac
