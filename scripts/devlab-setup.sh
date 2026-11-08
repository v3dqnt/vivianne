#!/usr/bin/env bash
# Prepares a Linux box (a Nebius CPU Devlab) to run Vivianne unattended and
# to start Nebius jobs. Safe to re-run. Not yet tested on a real Devlab.
set -euo pipefail

echo "== system packages"
sudo apt-get update
sudo apt-get install -y curl xvfb libwebkit2gtk-4.1-0 libgtk-3-0 \
  libayatana-appindicator3-1 libsecret-1-0 gnome-keyring dbus-x11

echo "== nebius CLI (official installer, docs.nebius.com/cli/install)"
if ! command -v nebius >/dev/null 2>&1; then
  curl -sSL https://artifacts.nebius.cloud/cli/install.sh | bash
fi
export PATH="$HOME/.nebius/bin:$PATH"
nebius version

echo
echo "Next, by hand (they need your browser sign-in):"
echo "  1. nebius profile create      # sign in, pick the project"
echo "  2. nebius ai job list         # confirms jobs work (empty is fine)"
echo "  3. install the Vivianne .deb, then follow docs/devlab-24x7.md for the service"
