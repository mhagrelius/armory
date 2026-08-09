# Armory

A World of Warcraft companion for the GNOME desktop, built around one problem
the existing tools do not address.

If you have played an account for a decade and want to go back and earn things
again, the account is what stands in the way. Achievements are account-wide and
already lit. Mounts and pets are collected and will not drop twice. Warbands
made most reputations account-wide, so even the grind is already done. Every
tracker in this space reads Blizzard's completion flags and therefore tells you
that you have finished, which is exactly the wrong answer.

Armory keeps its own ledger instead.

## How it works

A **run** takes a snapshot of the account on a date you choose, and measures
everything from that point.

For each achievement, one of three things is true:

- **Nobody has earned it.** The completion flag works normally. Read it.
- **An enrolled character earned it.** The run has it. Read it.
- **Somebody outside the run earned it.** The flag was set before the run began
  and will never move again — a second character completing an account-wide
  achievement produces no signal at all, no per-character copy, no second
  timestamp. This is the only case that needs work, and Armory calls it
  **poisoned**.

For a poisoned goal, Armory computes progress from the data that genuinely *is*
per character: quests completed, statistics, dungeon and raid encounters. "Complete
100 quests in Nagrand" is lit forever and useless as a signal, but the character's
own completed-quest list still grows as they quest Nagrand.

Where nothing can measure it, you mark it done yourself. Where it can never be
earned again — a mount the account already owns, a Feat of Strength — Armory says
so once and then leaves it out of the count entirely, rather than leaving a bar
that can never fill.

**Reputation gets the same treatment, and needs it more.** An account-wide
standing is not just untrustworthy, it is *stuck* — a faction the account
maxed out in 2023 is at the ceiling, so a character grinding it from nothing
has nothing to show. With the addon installed, Armory watches what each
character actually earns, session by session, and measures against that. A
character who has personally done the equivalent of Exalted counts as having
done it. The same machinery says whether a currency was earned here, moved
across the Warband, or was simply already there — and admits it when the game
does not give enough to tell.

**Enrolment is opt-in.** Every character on the account is synced, because
conditional requests make that nearly free, but only the ones you enrol are what
the run is about. The rest are kept for one purpose: explaining why something is
already spent.

## Getting started

```sh
./install.sh          # builds in release, installs under ~/.local
./install-addon.sh    # optional; copies the collector addon into your WoW install
```

There are two ways in, and the addon-only one needs nothing from Blizzard.

**Addon only.** Install the addon, log in on a character, log out. That is the
whole setup. Armory reads the roster, the achievements, the collections and the
Warband bank out of SavedVariables. You give up auction prices and any character
you have not logged in on.

**With a Battle.net client.** Adds the Market tab and fills in detail for alts
you have not played. On first run Armory walks you through creating one, which
is unavoidable and worth explaining: Battle.net does not support PKCE, and
Blizzard developer relations confirmed in October 2024 that every registered
client is a confidential client with a mandatory secret. Shipping a secret
inside the application would be a terms violation *and* would pool every user
into one 36,000-request-per-hour quota. The secret goes into your login keyring,
never into a file.

The developer portal has been unreliable since November 2025 and often answers
with a server error when creating a client. The usual cause is that client names
must be globally unique across every developer and the form does not say so —
change the name and try again. If it will not cooperate, take the addon-only
path; nothing important is behind the API.

## The collector addon

Optional, and it makes a real difference. It records what the web API has no
endpoint for:

| | Why the addon |
| --- | --- |
| Which character earned each achievement | `GetAchievementInfo`'s `earnedBy`. The web API has no attribution field at all — without it, *every* already-earned achievement has to be assumed poisoned |
| What each achievement criterion measures | The web API gives a criteria tree's shape and never its meaning |
| Where a mount or pet comes from | The journals say "Drop: Attumen the Huntsman, Karazhan". The web API says `DROP`, or nothing |
| Currencies | No endpoint exists |
| The Warband bank | No endpoint exists, and Blizzard has said none is planned |
| The roster itself | Not needed from the API at all — every character you log in on describes itself |
| What you did last night | The profile API is a logout snapshot with no history in it. It will say you have finished 4,312 quests and never which twelve of those were tonight |
| Who earned the reputation | Standings are account-wide and sit at whatever the furthest character reached. Nothing anywhere says which character did the work |

It is capture-only: it reads documented APIs and writes one SavedVariables file.
It makes no decisions, changes nothing, and automates nothing.

WoW writes SavedVariables at logout or `/reload` and at no other time, so log in
once and out again after installing it. Armory reads the file from there.

## The Chronicle

A journal, one entry an evening, written from what actually happened.

The addon records each session as it goes: where you went and in what order,
which quests you turned in, what fell over and what did not, what dropped, what
sold, who was in the party. Alongside every turn-in it keeps **the quest text
the game put on your screen** — the premise you were given and what was said
when you handed it back. That is the part no endpoint returns and no summary
elsewhere improves on, and it is why Armory needs to fetch nothing from a wiki
to know what an evening was about.

**And what NPCs told you when you asked** — gossip text is captured too, kept
apart from the ambient lines because most of it is a shopkeeper's greeting and
some of it is a storyline. Nothing tries to guess which; the journal's
instructions tell the model to use what carries the evening and quietly ignore
the rest.

**And what the world said while you were in it** — the NPC muttering to another
NPC, the boss mid-pull, the escort narrating itself. All of it scripted, all of
it written by somebody at Blizzard, none of it in any API. Only NPCs: what your
party said stays between you.

It records rather more than that. Which **campaign** each quest belonged to, so
a dozen turn-ins read as a chapter of one story rather than a dozen errands —
with Blizzard's own summary of that storyline attached. What **killed you**,
from the combat log, so a death is "a Gorian Warlock at Halaa" instead of a
coordinate. Which **instance** and at what difficulty, so a +18 keystone does
not look like walking through a front door. **Named rares**, gear you actually
upgraded, professions that improved, reputation ranks crossed, and a count of
how much you killed.

**Where the gold went**, which a purse total cannot tell you. Down forty gold
reads the same whether nothing happened or you earned three hundred questing and
spent three hundred and forty at the auction house. The addon watches which
window was open when the money moved, so an evening's card says *164g from quest
rewards, 81g found* against *240g on auction purchases, 14g on repairs* —
vendors, the mailbox, trade, flights, trainers, transmog and the barber all told
apart, and repairs separated from anything else you bought from the same
merchant. Gold an alt mailed you is marked as a transfer rather than counted as
income, because it is money the account already had.

**Three numbers and a name.** The longest single fight of the evening, the
hardest hit you took and who landed it, and how close you came to dying — the
things you would actually retell. And where a piece of gear came from: nothing
in the game connects an item to the boss that dropped it, so Armory puts the
loot back together with the kill and the card says the belt came off Durn.

**What you have ever done.** Two flasks tonight is barely worth saying; four
hundred and twelve of them is a character. The same goes for who keeps turning
up in your party, which boss you have pulled eleven times, where the hours
actually went, what keeps killing you, how far you have walked and how many
delves you have finished at what tier. The game keeps none of it, so Armory
keeps its own — shown as a row of counters above the cards when you are looking
at one character's evenings.

Every session gets a card whether or not anything is written about it: the
route, the storyline, the quests, the deaths, the books. That card is the
feature. On top of it, Armory asks a **`llama-server` on your own machine** to
write the evening up in the character's voice — two or three paragraphs of
first-person journal, built only from the log and told in as many words not to
invent anything that is not in it.

That is the same local server [Familiar](../familiar) talks to, at
`http://127.0.0.1:8080` unless you say otherwise. No API key, no bill, and
nothing about your evenings leaves the machine. It writes each new session
automatically; turn that off in Menu → *Journal Setup…* if you would rather
press the button yourself.

## The market

Two views of the same page. **Browse** is the whole commodity market as it
stands right now — search it, sort it by price, by how much is listed, by how
much the whole listed stock is worth, or by what is actually selling. Open
anything to see the shape of its order book: what one costs, what you would be
paying once you had bought through the cheap end, and the real middle.

That costs nothing, because Armory was already downloading the whole auction
file every hour and discarding most of it.

**Watching** an item is the other thing, and it is what gives you a history.
Blizzard publishes none at all, so the first snapshot after you ask is where
yours starts and the days before it cannot be recovered — the page says so
rather than showing you an empty chart. Watch it from the item you were already
looking at.

Item names arrive a few hundred at a time over successive syncs, because the
auction house gives Armory item ids and there is no way to look up thousands of
names at once. Until one arrives you get the id, dimmed. It fills in from the
top of whatever you are looking at.

## Crafting flips

Which of your characters should make what, and whether it is actually worth it.

Armory costs every recipe your characters know against the market — reagents at
the cheapest quality that has a price, minus the auction house's cut — and then
does the thing most crafting calculators do not: it ranks by **what has actually
been selling**, not by margin. A four-hundred-gold profit on something nobody
buys is forty unsold flasks. Blizzard publishes no sale data at all, so this is
inferred from stock quietly disappearing between hourly snapshots, and it is
labelled as inferred.

It will not guess at things it cannot see. Every figure assumes a one-star
craft, because what quality yours lands at depends on skill, specialisation and
luck and Armory reads none of them. A recipe with a reagent nobody has listed is
reported as *unpriced* rather than quietly dropped or costed as if the reagent
were free. Reagents already in your Warband bank are shown beside the row and
deliberately not taken off the cost — that read has not been confirmed against a
stocked bank yet, and a wrong number you can see beats a wrong number folded
into a margin.

One thing is needed from you: **open each character's profession window once.**
The game will not tell an addon what somebody can make until you do, and there
is no way around it.

## Several machines

The addon runs where you play, which is not always where you want to sit and
read about it. If Armory is on more than one machine, a small server on your own
network keeps them level: tonight's evening, the counters, the collections and
the run turn up everywhere, whichever PC recorded them.

Each machine keeps the whole account in its own database and works with the
server switched off. Nothing lives only on the server — it is where the machines
meet, not where the account is kept.

Turn it on in Menu → *Sharing…*: an address and a token, both or neither. An
address with no token cannot authenticate and a token with no address has
nowhere to go. The token goes into your login keyring. The same page shows what
is waiting to go up, which is the answer to "did tonight's session reach the
server" — and a button for when you do not want to wait.

`server/README.md` is how to stand the server up, and what to check when the
status dot is green and it still is not working.

## What is there

A sidebar of nine places. **Run** is the home page — progress against the
baseline, the observable goals closest to finishing, and the ones only you can
settle. **Chronicle** is the journal above. **Mounts**, **Pets**, **Toys** and
**Decor** each get a page of their own: the whole catalogue as a grid of
Blizzard's own artwork, searched as you type and filtered by whether you have it
and where it comes from, with the count on the row that opens it. **Roster** is
every character with opt-in enrolment, class crests and what the addon sees.
**Reputations** is where each character stands, with the standings The War
Within synced across your Warband marked rather than counted as theirs.
**Market** is prices for the realms and items you opted into — and, above them,
the things you have not collected that are for sale right now.

The pictures cost nothing. Blizzard's render service addresses a mount or pet by
the creature display id the addon already recorded and a class crest by the
class's own name, so both are URLs rather than requests — no token, no quota. A
toy's icon, an achievement's icon and a character's portrait each need one call
and fill in over successive syncs.

## What is not there yet

- **AllTheThings and drop rates.** Collections show Blizzard's one word —
  "Drop" — and link to Wowhead for the rest. Turning that into "from this boss,
  1 in 100, weekly lockout" is the next real increment.
- **Transmog.** The appearance endpoints exist and the dedup problem is
  interesting; neither is built.
- **The weekly vault and raid lockouts.** Blizzard exposes neither, and the
  addon does not read them yet.
- **A house editor.** The API returns placed objects and several web tools
  already render them. Armory tracks the decor collection and stops there.
- **Writing back to the game.** The addon channel only runs one way so far.
- **The Warband bank.** The addon reads it and came back empty, which is either
  an empty bank or the wrong `Enum.BagIndex`. Unverified either way.

## What this deliberately does not do

- **Automate anything in game.** No posting, no cancelling, no timed scans.
- **Scrape Wowhead.** Its terms forbid automated access and its robots.txt names
  the crawlers. Armory links out to it and fetches nothing.
- **Send your journal anywhere.** Entries are written by a model on your own
  machine. The lore in one comes from the quest text and campaign summaries the
  game itself displayed, so there is nothing to fetch and nothing to upload.
- **Pretend to be live.** Blizzard's profile data is a snapshot written when a
  character logs out. Every view says when it was taken.
- **Support Classic.** Those auction endpoints have been answering 404 since
  April 2026 with no fix in sight.
- **Keep a backlog of the impossible.** Spent goals are reported once and then
  filtered out.

## Development

```sh
./test.sh                      # fmt, clippy -D warnings, then the whole workspace
./test.sh --headless           # under Xvfb and a private D-Bus session
./sync-check.sh                # two machines against a real server, on loopback
cargo run --example preview -- /tmp/preview [dark]
```

Three crates. `armory-core`, in `core/`, links no GTK and opens no socket — a
source is a pair of pure functions that build a request and parse a body, so
every endpoint, every failure shape and every classification is checkable with
no display and no network. The root package is the GTK shell: `ui/http.rs` is
the only file that performs a request, and `ui/redirect.rs` the only one that
listens on a socket. `armory-server`, in `server/`, is the core's store with a
socket in front of it and no second opinion about anything.

No test touches the network. `sync-check.sh` is separate from `test.sh` because
it starts a process and binds a port, both on loopback and in a temporary
directory. `packaging/deploy-server.sh` builds the server image, proves it
starts, and pushes it.

`DESIGN.md` is the reasoning. `RESEARCH.md` is the evidence underneath it —
what each data source will and will not give us, with the constraints that
follow.

## Licence

GPL-3.0-or-later.

Not affiliated with or endorsed by Blizzard Entertainment. World of Warcraft is
a trademark of Blizzard Entertainment, Inc.
