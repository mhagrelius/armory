#!/usr/bin/env bash
#
# Copy the collector addon into every WoW client under one install.
#
# Finds the install the way Armory does, or takes a path — to the folder that
# holds `_retail_`, `_classic_era_` and the rest, or to any one of those:
#   ./install-addon.sh
#   ./install-addon.sh "/path/to/World of Warcraft"
#   ./install-addon.sh "/path/to/World of Warcraft/_retail_"
#
# Every client folder that has a WTF directory gets a copy. The same addon
# runs on all of them; what differs is which APIs answer, and the addon asks
# before calling.
#
set -euo pipefail
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

ADDON="Armory_Collector"

find_wow() {
  local inside="drive_c/Program Files (x86)/World of Warcraft"
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
    if [[ -f "$prefix/$inside/.build.info" ]]; then
      printf '%s' "$prefix/$inside"
      return 0
    fi
  done
  return 1
}

ROOT="${1:-$(find_wow || true)}"
if [[ -z "$ROOT" ]]; then
  echo "Could not find a WoW install. Pass the path to World of Warcraft:" >&2
  echo "  ./install-addon.sh \"/path/to/World of Warcraft\"" >&2
  exit 1
fi
# A client folder was passed rather than the install: step up to the install,
# so the other clients beside it are found too.
if [[ -d "$ROOT/WTF" && -f "$(dirname "$ROOT")/.build.info" ]]; then
  ROOT="$(dirname "$ROOT")"
fi
if [[ ! -f "$ROOT/.build.info" ]]; then
  echo "$ROOT does not look like a WoW install (no .build.info)." >&2
  exit 1
fi

# Every client's version, from .build.info: one row per installed product,
# with the version as e.g. 12.0.7.68887. Collected first, because each copy
# of the .toc lists every installed client's interface number.
#
# WoW greys out and refuses to load an addon whose Interface number does not
# match the client, unless "Load out of date AddOns" is ticked — and a
# data-capture addon that silently does not load looks exactly like Armory
# being broken. The .toc takes a comma-separated list, and a client picks the
# entry that is its own, so one file serves the retail client and the Classic
# ones beside it. The interface number is major*10000 + minor*100 + patch.
INTERFACES=""
VERSIONS=""
while IFS='|' read -r -a row; do
  version="${row[12]:-}"
  if [[ "$version" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+) ]]; then
    interface=$(printf '%d%02d%02d' "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}" "${BASH_REMATCH[3]}")
    INTERFACES="${INTERFACES:+$INTERFACES, }$interface"
    VERSIONS="${VERSIONS:+$VERSIONS, }$version"
  fi
done < <(tail -n +2 "$ROOT/.build.info")

installed=0
for client in "$ROOT"/_*_; do
  [[ -d "$client/WTF" || -d "$client/Interface" ]] || continue
  target="$client/Interface/AddOns/$ADDON"
  mkdir -p "$target"
  # Every Lua file in the folder, not a named list. The .toc says which of
  # them the client loads; an installer that names them too is a second list
  # to keep in step, and the failure when it drifts is a file that silently
  # never loads.
  cp "addon/$ADDON/$ADDON.toc" "addon/$ADDON/"*.lua "$target/"
  if [[ -n "$INTERFACES" ]]; then
    sed -i "s/^## Interface:.*/## Interface: $INTERFACES/" "$target/$ADDON.toc"
  fi
  echo "Installed $ADDON to $target"
  installed=$((installed + 1))
done

if [[ "$installed" -eq 0 ]]; then
  echo "No client folder under $ROOT has a WTF directory; nothing installed." >&2
  exit 1
fi
if [[ -n "$INTERFACES" ]]; then
  echo "Matched your clients: WoW $VERSIONS (interface $INTERFACES)"
fi
echo
echo "Restart WoW if it is running, then log in once and log out — the addon"
echo "writes its file on logout, which is the only time WoW saves one."
