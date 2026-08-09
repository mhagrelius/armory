#!/usr/bin/env bash
#
# Copy the collector addon into a WoW install.
#
# Finds the install the way Armory does, or takes a path:
#   ./install-addon.sh
#   ./install-addon.sh "/path/to/World of Warcraft/_retail_"
#
set -euo pipefail
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

ADDON="Armory_Collector"

find_wow() {
  local inside="drive_c/Program Files (x86)/World of Warcraft/_retail_"
  local prefix
  for prefix in \
    "$HOME/Games/battlenet/compatdata/pfx" \
    "$HOME/Games/battle-net/compatdata/pfx" \
    "$HOME/Games/battlenet/pfx" \
    "$HOME/Games/wow/pfx" \
    "$HOME/.wine" \
    "$HOME/.local/share/lutris/prefixes/battlenet" \
    "$HOME/.var/app/com.usebottles.bottles/data/bottles/bottles/battlenet"
  do
    if [[ -d "$prefix/$inside/WTF" ]]; then
      printf '%s' "$prefix/$inside"
      return 0
    fi
  done
  return 1
}

WOW="${1:-$(find_wow || true)}"
if [[ -z "$WOW" ]]; then
  echo "Could not find a WoW install. Pass the path to _retail_:" >&2
  echo "  ./install-addon.sh \"/path/to/World of Warcraft/_retail_\"" >&2
  exit 1
fi
if [[ ! -d "$WOW/WTF" ]]; then
  echo "$WOW does not look like a WoW install (no WTF folder)." >&2
  exit 1
fi

TARGET="$WOW/Interface/AddOns/$ADDON"
mkdir -p "$TARGET"
# Every Lua file in the folder, not a named list. The .toc says which of them
# the client loads; an installer that names them too is a second list to keep in
# step, and the failure when it drifts is a file that silently never loads.
cp "addon/$ADDON/$ADDON.toc" "addon/$ADDON/"*.lua "$TARGET/"

# Stamp the .toc with the installed client's interface version.
#
# WoW greys out and refuses to load an addon whose Interface number does not
# match the client, unless "Load out of date AddOns" is ticked — and a
# data-capture addon that silently does not load looks exactly like Armory
# being broken. The version lives in .build.info as e.g. 12.0.7.68887, and the
# interface number is major*10000 + minor*100 + patch.
BUILD_INFO="$(dirname "$WOW")/.build.info"
if [[ -f "$BUILD_INFO" ]]; then
  VERSION="$(awk -F'|' 'NR==2 {print $13}' "$BUILD_INFO")"
  if [[ "$VERSION" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+) ]]; then
    INTERFACE=$(printf '%d%02d%02d' "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}" "${BASH_REMATCH[3]}")
    sed -i "s/^## Interface:.*/## Interface: $INTERFACE/" "$TARGET/$ADDON.toc"
    echo "Matched your client: WoW $VERSION (interface $INTERFACE)"
  fi
fi

echo "Installed $ADDON to:"
echo "  $TARGET"
echo
echo "Restart WoW if it is running, then log in once and log out — the addon"
echo "writes its file on logout, which is the only time WoW saves one."
