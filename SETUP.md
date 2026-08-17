# Setting up another machine

Everything needed to get Armory and the collector addon onto a second (or
third) gaming PC, recording what you play and pushing it to the server.

Read `server/README.md` first if the server is not running yet. This assumes it
is, and that you know its address and token.

Throughout, the server is written as `http://nas.example.ts.net:8084`.
Substitute yours.

---

## What the machine needs

Linux, GNOME or otherwise, with WoW running under Wine or Proton. Armory finds
the install itself in the usual Battle.net, Lutris, Steam and Bottles prefixes;
if yours is somewhere else you will point at it once.

Build dependencies — Rust 1.80 or newer, and the development packages for the
four libraries Armory links:

```bash
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev libsoup-3.0-dev libsqlite3-dev
```

On Fedora:

```bash
sudo dnf install gcc pkgconf-pkg-config gtk4-devel libadwaita-devel libsoup3-devel sqlite-devel
```

Rust, if it is not there already:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

The machine also has to be on the tailnet, because that is the only route to
the server. `tailscale status` should list the NAS.

---

## 1. Get the source

```bash
git clone https://github.com/mhagrelius/armory.git ~/Projects/armory
```

## 2. Build and install the application

```bash
cd ~/Projects/armory && ./install.sh
```

Release build, then `~/.local/bin/armory` and a desktop entry. The first build
takes a few minutes. `./uninstall.sh` reverses it.

## 3. Install the collector addon

```bash
cd ~/Projects/armory && ./install-addon.sh
```

This finds the WoW install the same way Armory does and copies
`Armory_Collector` into `Interface/AddOns`. If it cannot find it, pass the path
to `_retail_`:

```bash
./install-addon.sh "$HOME/Games/battlenet/compatdata/pfx/drive_c/Program Files (x86)/World of Warcraft/_retail_"
```

It also stamps the addon's `## Interface:` line with your client's version.
That matters: WoW greys out an addon whose interface number does not match,
and an addon that silently does not load looks exactly like Armory being
broken. **Re-run `./install-addon.sh` after a WoW patch.**

## 4. Log in once, and log out

Start WoW, check `Armory_Collector` is enabled in the AddOns list, play or just
stand there, and then **log out to the character select screen or quit**.

WoW writes SavedVariables on logout and at no other time. Until you have logged
out once there is nothing on disk for Armory to read, and the application will
look empty.

## 5. Point Armory at the server

Launch Armory. If this machine has no Battle.net API client registered, take
the *Skip* path in onboarding — the addon supplies everything except the Market
tab and alts you have never logged in on.

Then **Main menu → Account & Sharing…**

| | |
|---|---|
| Address | `http://nas.example.ts.net:8084` |
| Token | the value of `ARMORY_TOKEN` from `server/.env` on the machine you deployed from |

Both or neither. An address with no token cannot authenticate and a token with
no address has nowhere to go, so Armory treats half a configuration as none and
stays off.

Press **Save**. The token goes into your login keyring, not into a file. The
field is empty on every launch afterwards because it cannot be pre-filled
without reading the secret back out to show it — the row's title says whether
one is held.

## 6. Watch the first pass

The same dialog is the triage. It shows:

- **Waiting to go up** — what this machine has recorded and the server has not
  taken yet, by kind, largest first. On a machine with years of history the
  first pass moves tens of thousands of rows in batches; the numbers count
  down.
- **This machine** — the id this installation has in the change log. Every
  machine gets its own, made once and kept in the database.
- **Last pass** — when, and what moved which way. "Nothing to do" is the
  steady state and what you want to see.

**Sync Now** runs a pass immediately. You should not need it: a pass runs at
startup, three seconds after anything is written, every five minutes as a
backstop, and immediately when another machine writes something.

---

## Checking it worked

From the new machine:

```bash
curl -s http://nas.example.ts.net:8084/health
```

`{"ok":true,"rows":N}` — `N` is how many rows the server's change log holds, so
it climbs as machines push. A refusal here is a network problem, not a token
one: `/health` takes no token.

Then, from a machine that already had the account: open **Account & Sharing…**
and check `Last pass` says something came down. Or just look at the Chronicle —
an evening recorded on the other PC should be there.

---

## What actually travels

All of it. The roster, the run and its goals, every evening the addon recorded,
the lifetime counters, the collections, the achievement criteria, provenance,
recipes, currencies, the Warband bank, watched realms and items, price history,
journal entries, and the cached API replies.

The rules that matter when two machines disagree:

- **A counter never goes backwards.** Tallies, earned reputation and earned
  currency take the larger of the two, whichever machine is behind. A
  reinstalled addon that starts counting from zero cannot take months of work
  off the other machines.
- **An evening is written once.** Sessions are keyed by character and start
  time, so the same evening arriving twice is the same evening.
- **A collectible merges.** The addon knows the source prose and the artwork,
  the web API knows the name and the expansion; whichever lands second does not
  flatten the other's half.
- **Everything else is last-writer-wins**, per row.

**A local expiry is not a deletion.** Armory sweeps cached API replies and price
history older than thirty days on the way out. That sweep stays on the machine
that ran it — otherwise a PC switched off for a month would come back and take
the last month off everything else.

---

## If it is not syncing

**The dialog says "Not sharing".** No address, or no token. Both.

**"The server refused the token."** The token does not match `ARMORY_TOKEN` on
the server. Copy it again from `server/.env` — it is 64 hex characters and a
truncated paste looks identical at a glance.

**"Could not reach …".** Check the tailnet: `tailscale status`, then
`curl -s http://nas.example.ts.net:8084/health`. If curl works and Armory
does not, check the address in the dialog has `http://` and the port.

**A banner across the top of the window.** Three passes in a row have failed.
It goes away on the next one that works. The account is whole on this machine
either way — nothing is waiting on the server.

**Nothing is queued and nothing arrives.** Two installations sharing a machine
id each get handed the other's rows as their own and neither pulls anything.
That happens if you copied `~/.local/share/armory/armory.db` between machines
rather than letting each build its own. Copying `~/.config/armory/settings.json`
is fine — the id is not in it. The fix is to delete the copied database and let
the machine pull the account down fresh.

**The Chronicle is empty on a new machine.** You have not logged out of WoW
yet. See step 4.

---

## More than one Battle.net account

The server holds as many accounts as you give it, each in a **separate
database**. They cannot merge — which matters, because every merge rule in
Armory is written to fold two views of *one* account together. A shared store
would fold two accounts together and call it agreement: one roster with both
sets of characters, collections added up, a run measuring a cohort drawn from
both, and not one error anywhere.

**Choosing one.** Account & Sharing… → *Account*. It lists what the server
answered plus `default`, which is where everything sent before accounts existed
lives. **New account…** at the bottom is how a name that is not there yet gets
made: letters, digits, dash, underscore and dot, because the name becomes a
directory on the server and it refuses anything else. The field says so and
will not save a name that would be turned away.

Point each machine at the account it belongs to. Two machines on the same
Battle.net account share a name; a machine on a different one gets its own.

**And which one this machine is reading.** The same dialog, *Game account* at
the top. A WoW install used by two Battle.net logins has a `WTF/Account` folder
each, holding different characters, collections and achievements — so the
folder and the server account are two halves of one answer, and getting the
first wrong fills a shared account with somebody else's roster. Changing it
re-reads the addon straight away; there is nothing to restart.

**Seeing them.** The same dialog lists every account the server holds with a
row count each, and marks which is this machine's.

**Deleting one.** The bin beside it. It confirms first, names what goes, and
says what survives — the server keeps no second copy, but **nothing local is
touched**: the whole account stays on every machine that holds it. If you
delete the account this machine syncs to, *Send Again* puts it straight back.

That is also how to swap: delete the old account, choose the new one under
*Account*, and the next pass starts it fresh. No SSH, no File Station, nothing
stopped.

**Send Again** is for one other case — after a server database has been
removed by hand. A client is then silently stuck: its cursor points past the
end of a log that no longer exists so it pulls nothing, and its seed mark says
the account has already gone up so it pushes nothing. Both ends report
themselves healthy and nothing moves again. *Send Again* clears both.

**If both accounts share one WoW install.** `WTF/Account/` then has a folder
each, and Armory picks one. It records which in `wow_account` in
`~/.config/armory/settings.json` and says so on stderr when there is more than
one; change that value to the other folder name to switch. Reading the wrong
one is not a near miss — it is a different account's characters, collections
and achievements.

## Keeping a machine up to date

```bash
cd ~/Projects/armory && git pull && ./install.sh && ./install-addon.sh
```

`install-addon.sh` is worth re-running even when the addon has not changed,
because of the interface-version stamp.
