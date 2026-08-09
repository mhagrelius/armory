#!/usr/bin/env bash
#
# Build a .deb into dist/.
#
set -euo pipefail
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

APP_ID="us.hagreli.Armory"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
ARCH="$(dpkg --print-architecture)"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

cargo build --release

install -Dm755 target/release/armory "$STAGE/usr/bin/armory"
install -Dm644 "data/$APP_ID.desktop" "$STAGE/usr/share/applications/$APP_ID.desktop"
install -Dm644 "data/$APP_ID.metainfo.xml" "$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
install -Dm644 "data/icons/$APP_ID.svg" \
  "$STAGE/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg"
install -Dm644 "data/icons/$APP_ID-symbolic.svg" \
  "$STAGE/usr/share/icons/hicolor/symbolic/apps/$APP_ID-symbolic.svg"

# The addon ships alongside rather than inside the game: nothing here knows
# where WoW is, and writing into a Wine prefix from a package's postinst would
# be a surprise. install-addon.sh copies it when asked.
install -Dm644 addon/Armory_Collector/Armory_Collector.toc \
  "$STAGE/usr/share/armory/addon/Armory_Collector/Armory_Collector.toc"
install -Dm644 addon/Armory_Collector/Armory_Collector.lua \
  "$STAGE/usr/share/armory/addon/Armory_Collector/Armory_Collector.lua"

mkdir -p "$STAGE/DEBIAN"
cat > "$STAGE/DEBIAN/control" <<CONTROL
Package: armory
Version: $VERSION
Section: games
Priority: optional
Architecture: $ARCH
Depends: libgtk-4-1 (>= 4.22), libadwaita-1-0 (>= 1.9), libsoup-3.0-0, libsqlite3-0
Recommends: gnome-keyring
Maintainer: Matthew Hagrelius <matthew@hagreli.us>
Description: World of Warcraft companion for the GNOME desktop
 Tracks a WoW account and measures a run - a replay of content the account
 already remembers - rather than reading Blizzard's completion flags.
CONTROL

mkdir -p dist
dpkg-deb --build --root-owner-group "$STAGE" "dist/armory_${VERSION}_${ARCH}.deb"
echo "==> dist/armory_${VERSION}_${ARCH}.deb"
