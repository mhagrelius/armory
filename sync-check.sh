#!/usr/bin/env bash
#
# Prove a sync pass against a real server, with two machines.
#
#     ./sync-check.sh
#
# **A sync that has never been contradicted has not been tested.** Everything
# in `core/src/replica.rs` is a pure function with unit tests, and everything
# in `server/` has its own — but between them sit a wire format, a transport,
# and the question of whether the two ends agree about what a row is. One
# machine can never ask that question, and there is no second platform coming
# along to shake it out by accident.
#
# So: two stores, one throwaway server on a spare port, the real server binary,
# the real client transport. It needs no network and no NAS — everything is on
# loopback and in a temporary directory, and nothing outside that directory is
# touched. That is the difference from planner's version of this, which needs a
# Postgres: armory-server keeps a SQLite file, so a throwaway server is a
# throwaway directory.
#
# Not part of ./test.sh, because it starts a process and binds a port. Run it
# after anything that touches sync.

set -euo pipefail
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

PORT="${SYNC_CHECK_PORT:-18084}"
# Thirty-two characters, which is the server's own floor. Not a secret: it
# lives for the length of this script on a loopback port.
TOKEN="0123456789abcdef0123456789abcdef"
WORK="$(mktemp -d)"
PID=""

cleanup() {
    code=$?
    [[ -n "$PID" ]] && kill "$PID" 2>/dev/null || :
    rm -rf "$WORK"
    exit $code
}
trap cleanup EXIT

echo "==> building"
cargo build -q -p armory-server --bin armory-server
cargo build -q --example sync-check

echo "==> a server of its own on 127.0.0.1:$PORT"
ARMORY_ADDR="127.0.0.1:$PORT" \
ARMORY_TOKEN="$TOKEN" \
ARMORY_DATA="$WORK/server" \
    ./target/debug/armory-server >"$WORK/server.log" 2>&1 &
PID=$!

# Wait for it rather than sleeping at it. A fixed sleep is either too short on
# a loaded machine or wasted on an idle one.
for _ in $(seq 1 50); do
    if curl -fsS "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then
        break
    fi
    if ! kill -0 "$PID" 2>/dev/null; then
        echo "the server exited before it answered:" >&2
        cat "$WORK/server.log" >&2
        exit 1
    fi
    sleep 0.1
done

if ! curl -fsS "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then
    echo "the server never answered /health:" >&2
    cat "$WORK/server.log" >&2
    exit 1
fi

echo "==> two machines"
SYNC_CHECK_URL="http://127.0.0.1:$PORT" \
SYNC_CHECK_TOKEN="$TOKEN" \
SYNC_CHECK_A="$WORK/machine-a" \
SYNC_CHECK_B="$WORK/machine-b" \
    ./target/debug/examples/sync-check

echo
echo "==> the server's own account"
curl -fsS "http://127.0.0.1:$PORT/health"
echo

echo
echo "sync-check: done."
