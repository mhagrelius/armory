#!/usr/bin/env bash
#
# Run the whole suite the way CI would, in the order that fails fastest.
#
#   ./test.sh            use the current session's display
#   ./test.sh --headless run under Xvfb and a private D-Bus session
#
# No test in here touches the network. The source layer is a pair of pure
# functions per endpoint, so a run is offline and deterministic. If a test ever
# needs a socket, the seam has been broken rather than the test being unlucky.
#
set -euo pipefail
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# GTK_A11Y=none skips the accessibility bus, a common source of CI hangs.
# GSETTINGS_BACKEND=memory keeps tests from touching real user state.
export GTK_A11Y=none
export GSETTINGS_BACKEND=memory
export RUST_BACKTRACE=1

# Every XDG directory the app reads, pointed somewhere disposable.
#
# XDG_DATA_HOME is armory.db. XDG_CONFIG_HOME is settings.json, and that one
# now names a sync server: a widget test registers a real ArmoryApplication,
# whose startup reads the settings and can begin a pass, so a developer with
# sync configured would have the suite push its fixtures at the real server.
xdg_home="$(mktemp -d)"
runtime_dir=""
# One trap for both throwaway directories. A second `trap … EXIT` replaces the
# first rather than adding to it.
cleanup() {
  rc=$?
  [[ -n "$runtime_dir" ]] && fusermount3 -u "$runtime_dir/doc" 2>/dev/null
  rm -rf "$xdg_home" ${runtime_dir:+"$runtime_dir"}
  exit $rc
}
trap cleanup EXIT
export XDG_DATA_HOME="$xdg_home/data"
export XDG_CONFIG_HOME="$xdg_home/config"
export XDG_STATE_HOME="$xdg_home/state"
export XDG_CACHE_HOME="$xdg_home/cache"
mkdir -p "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_STATE_HOME" "$XDG_CACHE_HOME"

# --workspace throughout, and it is not optional: without it cargo checks only
# the root package, and armory-core — which is most of the suite and the half
# that needs no display — silently stops being tested.
run=(cargo test --workspace --all-targets)
if [[ "${1:-}" == "--headless" ]]; then
  command -v xvfb-run >/dev/null || { echo "install xvfb first" >&2; exit 1; }

  # The private bus activates its own xdg-document-portal, which mounts a FUSE
  # fs at $XDG_RUNTIME_DIR/doc. Inheriting the login session's runtime dir means
  # that mount lands on /run/user/$UID/doc, on top of the real portal's; the real
  # one exits 21 and every flatpak launch fails until it is restarted. Hand the
  # session a throwaway runtime dir so its portals stay inside it.
  runtime_dir="$(mktemp -d)"
  chmod 700 "$runtime_dir"
  export XDG_RUNTIME_DIR="$runtime_dir"

  run=(xvfb-run -a dbus-run-session -- cargo test --workspace --all-targets)
fi

echo "==> cargo fmt --all --check"
cargo fmt --all --check

echo "==> cargo clippy"
cargo clippy --workspace --all-targets -- -D warnings

echo "==> luacheck (if installed)"
if command -v luacheck >/dev/null; then
  # The WoW API lives in .luacheckrc rather than on this line. It grew past the
  # point where a command-line list stayed in step with the addon.
  luacheck addon
else
  echo "    not installed, skipping"
fi

echo "==> ${run[*]}"
"${run[@]}"

echo
echo "All checks passed."
