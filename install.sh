#!/usr/bin/env bash
#
# Build in release and install under ~/.local. ./uninstall.sh reverses it.
#
set -euo pipefail
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

APP_ID="us.hagreli.Armory"
PREFIX="${PREFIX:-$HOME/.local}"

echo "==> cargo build --release"
cargo build --release

echo "==> installing to $PREFIX"
install -Dm755 target/release/armory "$PREFIX/bin/armory"
install -Dm644 "data/$APP_ID.desktop" "$PREFIX/share/applications/$APP_ID.desktop"
install -Dm644 "data/$APP_ID.metainfo.xml" \
  "$PREFIX/share/metainfo/$APP_ID.metainfo.xml"
install -Dm644 "data/icons/$APP_ID.svg" \
  "$PREFIX/share/icons/hicolor/scalable/apps/$APP_ID.svg"
install -Dm644 "data/icons/$APP_ID-symbolic.svg" \
  "$PREFIX/share/icons/hicolor/symbolic/apps/$APP_ID-symbolic.svg"

if command -v update-desktop-database >/dev/null; then
  update-desktop-database -q "$PREFIX/share/applications" || :
fi
if command -v gtk-update-icon-cache >/dev/null; then
  gtk-update-icon-cache -qtf "$PREFIX/share/icons/hicolor" || :
fi

echo
echo "Installed. Run 'armory', or find it in your applications."
echo
echo "The collector addon is optional and installed separately:"
echo "  ./install-addon.sh"
