# armory-server

One account, shared between one person's machines. It takes rows, applies them
with exactly the rules a client would, keeps a log of what landed, and hands
each machine everything in that log it did not write itself.

It is never the only copy. Every machine keeps the whole account locally and
works with the NAS switched off; this is where the machines meet, not where the
account lives.

## What it is not

It does not plan a run, evaluate a criterion, decide whether a goal is poisoned,
cost a craft, or write a journal entry. All of that is the client's, where it
already is and where it is already tested. A server that starts answering "what
is left to do" is a second Armory that can disagree with the first.

It is also not a second implementation of the store. It opens the same SQLite
schema through the same `armory-core`, so `save_collected`'s merge and a tally's
`MAX` on conflict have one definition rather than two that can part.

## Routes

| | |
|---|---|
| `GET /health` | Is it up, and how much is held. No token, so the container healthcheck needs no secret. |
| `POST /push` | Apply these rows, stamped with the pushing machine. |
| `GET /pull?since=&limit=` | Everything in the log above `since` that this machine did not write. |
| `GET /wait?since=` | Park until there is something above `since`, or fifty seconds. |

Everything but `/health` needs `Authorization: Bearer $ARMORY_TOKEN`, and
everything but `/health` needs `X-Armory-Machine: <id>`.

The token is checked as a guard before the route table rather than route by
route, so a route added later is authenticated by default — forgetting to add
one to a list should fail closed. A wrong token gets `401 unauthorized` and
nothing about which part was wrong. A path nobody serves is a `404`, but only
once the token is right: an unauthenticated caller learns nothing about what
routes exist. A known route with the wrong method is a `405`.

**The machine header is refused only on `/push`**, with `400 no
X-Armory-Machine header` — a row with nobody's name on it comes straight back to
whoever sent it, forever. `/pull` and `/wait` take it as a filter instead, so
omitting it there is not an error: it just hands a machine back its own writes,
which is the same failure arriving quietly.

```
GET  /health                              → 200 {"ok":true,"rows":41207}
POST /push       {"rows":[…]}             → 200 {"written":12,"removed":0,
                                                 "kept":3,"unreadable":0,
                                                 "cursor":41219}
GET  /pull?since=0&limit=2000             → 200 {"parcel":{"rows":[…]},
                                                 "cursor":41219,"more":true}
GET  /wait?since=41219                    → 200 {"changed":false,"cursor":41219}
```

`since` defaults to 0 and `limit` to 2000, which is also its ceiling — a larger
one is clamped rather than refused. A first sync is an account's whole history
and runs to tens of thousands of rows, so `more` is what stops a client that
took one batch from looking like one that finished. `kept` counts rows both
sides already agreed on; it is the commonest outcome and not a failure.

`/wait` gives up after fifty seconds and answers `changed: false` with the
current cursor, so a client that was told nothing still learns where the log
ends. Long enough that an idle client is not reconnecting constantly, short
enough that nothing between here and a gaming PC decides a silent connection is
a dead one.

## Setting it up

Build and push the image from a workstation:

```sh
./packaging/deploy-server.sh
```

Then, on the NAS, **make the data folder first**: File Station → Create →
Create folder → `/volume1/docker/armory-server/data`. Synology's Docker refuses
to create a missing bind-mount source where vanilla Docker would, and the
project will not start without it — `Bind mount failed: … does not exist`.

Container Manager → Project → Create, name it `armory-server`, path
`/volume1/docker/armory-server`, source **Upload docker-compose.yml**, and
upload `server/docker-compose.yml`.

**Uncheck "Start the project once it is created."** The compose file requires
`ARMORY_TOKEN`, and it arrives in a `.env` that does not exist yet — starting
first just fails.

`deploy-server.sh` leaves a filled-in `server/.env` behind on the workstation,
with the token and the tag it just pushed. **Upload that file itself** through
File Station rather than making a fresh one on the NAS: the token is sixty-four
hex characters, and a truncated paste is indistinguishable from a correct one
until a client is refused. Then Action → Build.

Keep the workstation copy. It is the only place the token is written down, and
every client needs it — see `SETUP.md`.

**Do not type the compose file into Container Manager's editor.** It carries the
previous line's indentation onto the next and adds what you type to it, so six
lines in everything is nested under everything else. The `YAML Configurations`
tab on an existing project is read-only, including when it is stopped.

## Checking it, rather than believing the dot

```sh
curl -s http://nas.example.ts.net:8084/health

curl -s -H "Authorization: Bearer $ARMORY_TOKEN" \
     -H "X-Armory-Machine: laptop" \
     'http://nas.example.ts.net:8084/pull?since=0&limit=1'
```

`/health` answering `{"ok":true,"rows":0}` is a server that started and has
nothing — which is right on the first day and wrong on the second. That row
count is the whole reason `/health` says more than `ok`: "up" and "up and
holding the account" are different answers, and only one of them is worth a
green dot.

Prove it can write, because a bind mount it cannot write to looks exactly like
one it can until something is pushed:

```sh
curl -s -X POST -H "Authorization: Bearer $ARMORY_TOKEN" \
     -H "X-Armory-Machine: probe" \
     -d '{"rows":[]}' \
     http://nas.example.ts.net:8084/push
```

An empty parcel writes nothing but takes the store's lock and opens the
database, so a `200` here is the file being readable and a `500 could not
apply` is not. Then look for `armory.db` in File Station under
`/volume1/docker/armory-server/data`.

The status dot lies in both directions — amber for a missing `curl`, green for a
container that cannot write. And **a blank Log tab means nothing**: when a
container exits, go straight to `sudo docker logs armory-server` over SSH.

## Three things learned the hard way here

**The image name in the compose file is `localhost:5050/…`,** even though the
push went to the NAS's tailnet name. A registry stores repositories by name
rather than by hostname, so it is the same image, and Docker accepts a registry
reached over localhost on plain HTTP without being configured to. Naming the NAS
means editing the daemon config over SSH for no gain.

**`user: "0:0"`, and here it is not a precaution.** /volume1 carries Synology's
own ACLs, which override POSIX ownership; a container uid that is not a DSM user
cannot be granted write access through them, and a `chown -R 10001:10001` on the
mount reports success while every write still fails. planner-server carries the
same line against a mount it does not have. This one has the account in it.

**`ARMORY_BIND` cannot be the NAS's Tailscale address.** Tailscale runs there as
a DSM package in userspace, so `100.x.y.z` is on no local interface and Docker
refuses the container with `bind: cannot assign requested address`. It listens
on every interface instead, which is the LAN and the tailnet and nothing else,
because nothing is forwarded on the router. The token crosses the LAN in clear
text; on this network that is the accepted trade, and it is the same one
brain-server and planner-server make.
