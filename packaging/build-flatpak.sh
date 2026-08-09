#!/usr/bin/env bash
#
# Build and install the Flatpak locally.
#
# Everything Armory links is in org.gnome.Sdk already - gtk4, libadwaita,
# libsoup3, sqlite - so there are no extra modules here.
#
set -euo pipefail
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

command -v flatpak-builder >/dev/null || {
  echo "install flatpak-builder first" >&2; exit 1; }

# Cargo cannot reach the network inside the sandbox, so the crate sources are
# vendored into a generated manifest fragment first.
if [[ ! -f packaging/flatpak/cargo-sources.json ]]; then
  command -v flatpak-cargo-generator >/dev/null || {
    echo "need flatpak-cargo-generator (from flatpak-builder-tools) to vendor crates" >&2
    exit 1; }
  flatpak-cargo-generator Cargo.lock -o packaging/flatpak/cargo-sources.json
fi

flatpak-builder --force-clean --user --install \
  .flatpak-build packaging/flatpak/us.hagreli.Armory.yaml

echo "==> flatpak run us.hagreli.Armory"
