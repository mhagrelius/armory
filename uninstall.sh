#!/usr/bin/env bash
#
# Reverse ./install.sh. Leaves your data and settings alone.
#
set -euo pipefail

APP_ID="us.hagreli.Armory"
PREFIX="${PREFIX:-$HOME/.local}"

rm -f "$PREFIX/bin/armory"
rm -f "$PREFIX/share/applications/$APP_ID.desktop"
rm -f "$PREFIX/share/metainfo/$APP_ID.metainfo.xml"
rm -f "$PREFIX/share/icons/hicolor/scalable/apps/$APP_ID.svg"
rm -f "$PREFIX/share/icons/hicolor/symbolic/apps/$APP_ID-symbolic.svg"

if command -v update-desktop-database >/dev/null; then
  update-desktop-database -q "$PREFIX/share/applications" || :
fi

echo "Removed. Your settings and database are still in:"
echo "  ${XDG_CONFIG_HOME:-$HOME/.config}/armory"
echo "  ${XDG_DATA_HOME:-$HOME/.local/share}/armory"
echo "Delete those by hand if you want them gone."
