#!/usr/bin/env bash
#
# Build armory-server, prove it starts, and push it to the NAS registry.
#
#     ARMORY_REGISTRY=nas.example.ts.net:5050 ./packaging/deploy-server.sh
#
# Tests first, then build, then a smoke test of the actual image, and only
# then a push. A registry is a place other machines pull from; getting a
# broken tag out of one is more work than not putting it there.
#
# The tag is today's date and the commit. `:latest` means a restart can quietly
# change what is holding the account, which is the wrong kind of surprise.

set -euo pipefail
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

REGISTRY="${ARMORY_REGISTRY:-nas.example.ts.net:5050}"

# Asked before the tag is built rather than after. Without a commit the
# substitution below fails under `set -e` and the script dies on git's own
# `fatal: Needed a single revision`, which says nothing about deployment to
# somebody reading it.
#
# ARMORY_TAG is deliberately not an escape from this one. It names the image,
# but the check below still needs a HEAD to compare the working tree against —
# so with no commit there is nothing to say the tag is honest about, whatever
# it is called.
if ! git rev-parse --verify --quiet HEAD >/dev/null; then
    echo "this repository has no commits, so nothing can say what would be in" >&2
    echo "the image. Commit first." >&2
    exit 1
fi

# The date says when, the commit says what. A date alone cannot answer "which
# commit is running on the NAS", which is the question actually asked when
# something is behaving oddly — and two builds in one day made it unanswerable
# rather than merely awkward.
TAG="${ARMORY_TAG:-$(date +%Y-%m-%d)-$(git rev-parse --short HEAD)}"
IMAGE="$REGISTRY/armory-server:$TAG"

# A tag naming a commit has to mean it. Untracked files are fine — CLAUDE.md
# and a local .env live beside this — but a tracked change that is not in the
# commit would make the tag a lie.
if ! git diff-index --quiet HEAD --; then
    echo "the working tree has uncommitted changes, so $TAG would not describe" >&2
    echo "what is in the image. Commit them, or set ARMORY_TAG to say so." >&2
    exit 1
fi

echo "==> ./test.sh"
./test.sh

echo "==> podman build $IMAGE"
# --format docker, not podman's OCI default: HEALTHCHECK has no place in the
# OCI image spec, so an OCI build drops it with a warning that is easy to miss.
# The compose file declares one too, but an image that cannot say whether it is
# well is worth avoiding on its own.
podman build --format docker -f server/Containerfile -t "$IMAGE" .

echo "==> smoke test"

# Start it with a token that is too short, so it gets far enough to prove the
# binary runs, reads its configuration and refuses what it should — without
# needing the real secret in a build script.
#
# Captured before it is searched, not piped into grep. Refusing is the correct
# behaviour *and* a non-zero exit, and under `pipefail` that non-zero would
# fail the pipeline — so piping made a passing smoke test look like a failing
# one, which is the worst direction for a check to be wrong in.
refusal="$(podman run --rm -e ARMORY_TOKEN=short "$IMAGE" 2>&1 || true)"
if grep -q "at least" <<<"$refusal"; then
    echo "    refuses a short token"
else
    echo "    the image did not refuse a short token, it said:" >&2
    echo "$refusal" >&2
    exit 1
fi

# And with no token at all, which is the other way a server that invents a
# default would get past its configuration and protect nothing.
missing="$(podman run --rm "$IMAGE" 2>&1 || true)"
if grep -q "ARMORY_TOKEN" <<<"$missing"; then
    echo "    refuses a missing token"
else
    echo "    the image did not refuse a missing token, it said:" >&2
    echo "$missing" >&2
    exit 1
fi

# The compose file says `read_only: true`, and that claim is checked here
# rather than discovered on the NAS. Everything this writes — the database, its
# WAL, its shared-memory file — is under ARMORY_DATA, so a read-only root with
# a writable volume has to be enough. A service that needs to write elsewhere
# fails on the Synology in a way that looks exactly like the ACL problem and is
# not, and finding that out over SSH costs an evening.
#
# `--user 0:0` because that is what the compose file says, and running it any
# other way tests a configuration nothing deploys. It is also the only way this
# passes under rootless podman: the image's uid 10001 lands on a subuid that
# does not own the host directory, and the container exits with
#
#     could not open /var/lib/armory/armory.db: unable to open database file
#
# — the same shape as the Synology ACL failure, from an unrelated cause. Two
# ways of arriving at one error message is a good reason to test the deployed
# configuration rather than a convenient one.
#
# The same run answers /health from inside the container, which is the
# healthcheck the compose file will use.
echo "    read-only root with a writable volume"
volume="$(mktemp -d)"
container="armory-server-smoke-$$"
token="0123456789abcdef0123456789abcdef"
# Not `--rm` on the run below: a container that exited immediately is the case
# worth reading the logs of, and `--rm` takes them away before anything can.
# The trap is what stops that costing a stray container.
trap 'podman rm -f "$container" >/dev/null 2>&1 || true; rm -rf "$volume"' EXIT

# Published on an ephemeral loopback port, because the check below has to make
# a real request and the image carries no curl to make one from inside.
podman run --detach --name "$container" \
    --read-only \
    --user 0:0 \
    -p 127.0.0.1::8084 \
    -e ARMORY_TOKEN="$token" \
    -v "$volume:/var/lib/armory:Z" \
    "$IMAGE" >/dev/null

started=""
for _ in $(seq 1 30); do
    if podman exec "$container" /usr/local/bin/armory-server --health >/dev/null 2>&1; then
        started="yes"
        break
    fi
    sleep 1
done

if [ -z "$started" ]; then
    echo "    the image did not answer its own healthcheck, it said:" >&2
    podman logs "$container" >&2 || true
    podman rm -f "$container" >/dev/null 2>&1 || true
    exit 1
fi
echo "    answers --health"

# A store is opened the first time a request names its account, and never at
# startup — `Server::health` touches none on purpose, so a container that has
# only been asked whether it is well leaves the volume empty and a file check
# here would be testing the order of two unrelated events. One authenticated
# pull is what makes the mount answer, and it proves the token, the routing and
# the account path on the way past.
address="$(podman port "$container" 8084 | head -n 1)"
if ! curl -fsS -m 10 \
    -H "Authorization: Bearer $token" \
    -H "X-Armory-Machine: smoke" \
    "http://127.0.0.1:${address##*:}/pull?since=0" >/dev/null; then
    echo "    the image did not answer an authenticated pull on $address, it said:" >&2
    podman logs "$container" >&2 || true
    exit 1
fi
echo "    opens an account on request"

podman rm -f "$container" >/dev/null 2>&1 || true

# The account has to have landed in the volume rather than on the root
# filesystem, or the mount is decorative and the NAS loses everything on the
# next Build. A client that names no account is talking about `default`, which
# is where `accounts::adopt_old_store` puts a pre-account database too.
if [ -f "$volume/accounts/default/armory.db" ]; then
    echo "    keeps the account in the volume"
else
    echo "    nothing was written to the volume; the mount is not where the" >&2
    echo "    database went, and a restart on the NAS would lose it." >&2
    ls -laR "$volume" >&2
    exit 1
fi

echo "==> podman push $IMAGE"
# --tls-verify=false because the registry speaks plain HTTP. It is reachable
# only over the tailnet, which is what makes that acceptable.
podman push --tls-verify=false "$IMAGE"

cat <<EOF

Pushed $IMAGE

Next, on the NAS:
  1. File Station → make /volume1/docker/armory-server/data if it is not there.
     Synology's Docker refuses to create a missing bind-mount source, where
     vanilla Docker would, and the project will not start without it.
  2. Container Manager → Project → armory-server
  3. Set ARMORY_SERVER_IMAGE in its .env to:

       localhost:5050/armory-server:$TAG

     localhost, not the tailnet name — a registry stores repositories by name
     rather than by hostname, so it is the same image, and Docker accepts a
     registry reached over localhost on plain HTTP without being configured
     to. Naming the NAS means editing the daemon config over SSH for no gain.
  4. Build.

Then check it answers rather than trusting the status dot:
  curl -s http://nas.example.ts.net:8084/health
EOF
