# Armory — design for review

A companion for a World of Warcraft account that has been played for a decade and
is about to be replayed. `RESEARCH.md` is the evidence behind every claim here.

## Scope

One Battle.net account, US region, retail only. Twenty-three characters across
five realms, of which a much smaller **enrolled cohort** is what the application
actually reasons about.

Eight capabilities, in build order:

1. **Roster** — the enrolled cohort in one view. Web API only.
2. **Collector addon** — built second, because everything below is better with it
   and two of them are impossible without it.
3. **The run** — the soft reset as a first-class object.
4. **Achievements** — criteria-level progress, read from the flag where the flag
   still means something and recomputed only where the account has poisoned it.
5. **Collections** — what is left, what to tackle next, and where to read about it.
6. **Bags, banks, Warband bank, currencies** — surfaced from the addon.
7. **Auction house** — opt-in realms, opt-in watchlist.
8. **The chronicle** — a journal of what actually happened, an evening at a time.

Classic is not a target: its auction endpoints have been answering 404 since April
2026 with no fix in sight.

## The enrolled cohort

Every character on the account is synced — `If-Modified-Since` makes twenty-three
characters cost almost nothing, and a character that has not logged out since the
last sync costs one 304. But sync is not enrolment.

**Enrolment is explicit and per character.** The cohort is the set the run is
about, the set the interface shows, and the set every "which of my characters
should do this" answer ranges over. Everyone else stays in the database for
exactly one purpose: explaining why something is already owned. When a mount
cannot be re-collected because Aeltor looted it in 2016, Aeltor has to still be
there to say so, without cluttering a view about Somechar.

So there are two populations, and they are not the same list:

- **The cohort** — what the run is measured against.
- **The account's history** — what the game remembers, which is what stands in the
  way.

## The run

### Poisoned goals, and only those

Most of the account is not a problem. Three cases, and only the third needs any
work at all:

- **Not yet earned by anyone.** The flag and `earnedBy` behave exactly as
  designed. A cohort character completes it, it lights up, `earnedBy` names them.
  Read it and move on.
- **Already earned, by an enrolled character.** The run has it. Read it and move
  on.
- **Already earned, by someone outside the cohort.** The flag is permanently
  useless. It was set before the run began and will never change again, because
  **a second character completing an account-wide achievement produces no new
  signal at all** — no per-character shadow copy, no second timestamp, no event.
  `earnedBy` names whoever earned it *first*, and will go on naming them however
  many times the content is replayed.

Only the third case is **poisoned**. Everything else is a flag read, and the
expensive machinery below never runs against it. On a genuinely fresh Battle.net
account nothing is poisoned and this section costs nothing whatsoever.

Poisoning is decided once, at baseline, per run. It cannot appear later: an
achievement earned during the run is earned by a cohort character by definition.

### Recomputing a poisoned goal

For a poisoned goal, and only for a poisoned goal, **Armory evaluates the criteria
against a character's own primary data instead of the completion flag.**

"Complete 100 quests in Nagrand" is lit account-wide and dead as a signal. But the
character's own `/quests/completed` list still grows as they quest Nagrand, and
that list is per character. Recomputing the criterion against it yields real,
moving progress for a character the game considers to have finished years ago.

The primary data that is genuinely per character:

| Source | Answers |
| --- | --- |
| `/quests/completed`, `C_QuestLog.GetAllCompletedQuestIDs()` | quest and storyline criteria |
| `/achievements/statistics` | anything counted — kills, uses, distances |
| `/encounters/{dungeons,raids}` | boss and instance criteria |
| `/professions` | recipe and skill criteria |
| `/reputations` | faction criteria, subject to the Warband caveat below |

### The three buckets

Every *poisoned* goal is classified once, from its criteria tree, into one of three
states. Unpoisoned goals never enter this classification — they are tracked by
their flag like any normal achievement. The classification is the product.

**Observable.** Every criterion resolves against per-character primary data.
Progress is computed, a bar moves, and no one has to remember anything. This is
larger than it first looks, because most achievements decompose into quests,
statistics and encounters.

**Attestable.** No per-character signal exists, but a person knows whether they
did it. Armory offers a manual mark — character, date, done — and treats it as
truth thereafter. Holiday bosses, one-off world events, anything whose only record
was the account-wide flag itself.

**Excluded.** No signal, no reasonable attestation, or permanently spent. An
account-wide collectible already owned. A bind-on-pickup mount that will not drop
again for this account. A Feat of Strength. Removed content. These are filtered
out of the run rather than left sitting in a backlog forever as a reproach.

Exclusion is a feature and it needs defending. A backlog that lists things which
cannot be done is not a backlog, it is noise, and every existing tool in this space
produces exactly that. Armory says what is spent, once, in a report — and then
stops mentioning it.

The user can move a goal between buckets. Automatic classification is a starting
position, not a verdict; the criteria data is irregular enough that it will get
some wrong, and being overruled is cheaper than being argued with.

### Provenance: who actually earned it

The run's whole method is deciding which signals can be trusted about *this*
cohort. Achievements were the first case. Reputation and currency are the second
and they are worse, because neither carries even the flawed attribution
`earnedBy` gives an achievement.

**Reputation.** The War Within syncs most standings across the Warband, and
Dragonflight's renown works the same way. A fresh character reads Exalted and
Renown 25 with factions it has never met. Refusing to count that is correct —
and it leaves the goal permanently unmeasurable, because the standing was
already at the ceiling before the run began and *cannot move* however much work
is done. A person replaying the game can genuinely earn the equivalent of
Exalted and have nothing to show for it.

**Currency.** Three ways an amount can arrive on a character, and only one of
them is work: earned here, moved across the Warband, or already there.

So Armory records the work itself. The addon snapshots every faction and every
currency at login and diffs at logout, accumulating per-character totals that
survive across sessions. **The attribution is sound because you can only play
one character at a time**: one client, one character, so a rise between a login
and a logout is that character's doing. Anything that moved while they were
logged out was somebody else's and is deliberately invisible.

That gives a second, honest number beside the account's standing — and
`PrimaryData::earned_reputations` is what a poisoned reputation criterion is
measured against. A character whose own work covers the requirement stops being
inherited and counts.

Two disciplines carry over from the rest of the design. **With no addon there is
no observation, and the answer is zero rather than the account's standing** —
falling back would be precisely the inflation the rule exists to prevent.
And where the game genuinely cannot say, Armory does not guess: a transferable
currency whose `totalEarned` the client does not maintain is `Origin::Unclear`,
said out loud, because a confident wrong attribution is the one failure that
would make the whole application worthless.

### Contaminated by Warbands

Reputation deserves its own state. The War Within made most reputations
account-wide, synced to the furthest-progressed character, so a criterion that
looks observable through `/reputations` may be answering with a value some
unenrolled character earned in 2023.

Armory reports this rather than silently mis-scoring it: a reputation criterion
whose standing exceeds what the cohort could have earned is marked **inherited**,
shown as such, and left to the user to attest or exclude. The alternative — trust
the number — quietly inflates run progress, which is the one failure mode that
would make the whole thing worthless.

### The baseline

A run begins with a date and a snapshot: every collection, every achievement,
every character's primary data, frozen. Progress is the delta from that snapshot.
This is what makes "excluded because already owned" a decidable question, and it
is why the baseline is immutable once taken.

More than one run can exist. More than one Battle.net account can exist, too — a
genuinely clean reset means a separate account rather than a separate character or
license, since collections are shared across licenses under one login. Armory
holds several and can diff them.

## Sources

### The seam

`model/source/*` builds request URLs and parses response bodies. `ui/http.rs`
performs the requests. Nothing under `model/` opens a socket, so every source,
every failure shape and every classification is checkable with no display and no
network — Sleeve's seam, and it is the reason that application's test suite exists
at all.

`model/store.rs` touches a local SQLite file and `model/addon/*` reads Lua from
disk. Both are deterministic, neither goes near the network, and both are tested
against real files. The seam worth defending is the network one.

The seam is now a crate boundary as well. `model/` is the `armory-core` crate
under `core/`, links no toolkit, and is re-exported from the shell as
`armory::model` so every path above still reads the way it did; the root package
is the GTK application and `server/` is `armory-server`. The split is there
because the server needs the schema and the merge rules and cannot link
libadwaita to reach them — see *Sharing an account between machines*.
`./test.sh` runs `--workspace`, without which cargo checks the root package only
and the half that needs no display silently stops being tested.

### Outcome

Sources answer with the four-variant `Outcome` Sleeve established: `Found`,
`Empty`, `Stale`, `Failed`. `Empty` is not `Stale`. A character with no mounts and
a mounts parser that has stopped understanding the response must not look alike,
because the second one silently empties a collection view and makes the run look
finished.

### Credentials

Battle.net does not support PKCE and never has; Blizzard developer relations
confirmed in October 2024 that every registered client is confidential and a
secret is mandatory. Shipping a secret is a terms violation and pools every user
into one 36,000/hour quota.

So Armory walks the user through registering their own client, and stores the id
and secret in the GNOME keyring via libsecret. The redirect is a loopback listener
on a fixed port, registered as `http://127.0.0.1:PORT/callback` — Blizzard accepts
loopback and rejects custom schemes.

The onboarding page has to survive a broken portal. Client creation has been
returning 500s continuously since November 2025; the usual cause is that client
names must be globally unique across all developers and the UI does not say so.
The page says so.

Refresh tokens are inconsistent — documented as unavailable by Blizzard staff, yet
present in some responses. Armory assumes they do not work and re-prompts, and
treats a working refresh as a bonus rather than a contract.

### The 30-day obligation

The API terms mandate a maximum 30-day TTL on data obtained through the API. This
is not a suggestion to be routed around; it is a scheduled deletion in
`model/store.rs`, and it is the reason auction history has a horizon.

## The addon

`Armory_Collector`, free and open, unobfuscated, capture-only. The 2018 add-on
policy demands the first three and the EULA bans automation; a data-capture addon
is what TSM, Auctionator, Altoholic and WoWthing Collector have done unchallenged
for years. The November 2025 addon disarmament work targets real-time combat
decision-making and does not touch this.

### Two channels, both files

An addon cannot open a socket or read a file — the sandbox strips `io`, `os`,
`debug`, `require` and `loadfile`. So:

- **In:** the addon writes SavedVariables at logout or `/reload`; Armory watches
  `WTF/Account/PLAYER1/SavedVariables/Armory_Collector.lua` and the per-character
  paths beneath it, and parses Lua.
- **Out:** Armory writes a Lua file into the same place *before* the game loads.
  This is how the TSM desktop app delivers pricing, and it is how a shopping list
  or a run's current goals get in front of the player.

The install is auto-detected under Wine and Proton prefixes.
`~/Games/battlenet/compatdata/pfx/drive_c/Program Files (x86)/World of Warcraft/_retail_`
is the one on this machine; Lutris, Steam and Bottles layouts get probed too.

Files are written atomically and never while WoW is running, because the game
rewrites SavedVariables wholesale on exit and would clobber them.

There is a Lua VM ceiling of 262,144 unique literal values per file. Capture is
chunked per character to stay well under it.

### What it captures that the API cannot

Bags, bank and reagent bank via `C_Container.GetContainerItemInfo`. The **Warband
bank** via `C_Bank` and `Enum.BankType.Account`. Currencies via `C_CurrencyInfo`,
including the `isAccountWide` and `isAccountTransferable` flags that decide
whether a currency is even a per-character axis anymore. Profession recipes and
knowledge via `C_TradeSkillUI`. Bulk quest completion via
`C_QuestLog.GetAllCompletedQuestIDs()`. Per-source transmog state via
`C_TransmogCollection.GetAppearanceSources`.

And `GetAchievementInfo`'s `wasEarnedByMe` and `earnedBy` — which character
originally earned each account-wide achievement. **This is what decides
poisoning**, and therefore what keeps recomputation down to the small set that
needs it. Without the addon every already-earned achievement has to be assumed
poisoned, because the web API exposes no attribution at all; with it, the ones an
enrolled character earned are simply read like any other flag. The addon does not
make the run possible so much as it makes the run cheap.

## Sharing an account between machines

One person, several machines: a gaming PC where the addon writes SavedVariables,
a laptop that is not the gaming PC, and a small server on the LAN so the two
agree. Every machine keeps the whole account in its own SQLite file and works
with the server switched off. It is where the machines meet, not where the
account lives.

### A change log and a cursor, not a merge

**Armory's data is recorded, not co-edited.** An addon writes down what
happened, the API answers what is held, and a person makes a handful of
decisions on top: enrol a character, attest a goal, watch an item. Almost none
of it can genuinely conflict — two machines reading the same logout snapshot
produce the same rows — and the merges that settle what little can were already
written and already tested in the store: a tally takes the larger count, a
collectible merges field by field, an evening is written once and never again.

So neither of the shapes the siblings use. Brain's three-way merge exists
because two people edit the same note. Planner's base snapshot exists because a
stale write has to be refused. Here there is one person, and the arbitration is
a `MAX` in a SQL upsert that predates any of this.

What is left is bookkeeping. Every write notes the row it touched in a `change`
table — scope, key, an autoincrementing `seq`. A row written twice keeps one
entry at the later `seq`, so the log is the size of the data rather than the
size of the history. A client pushes what it has waiting and pulls everything
above its cursor; the server does the same thing in reverse. `core/src/sync.rs`
is the vocabulary — which tables travel, and how each column settles when both
sides have one — and `core/src/replica.rs` is what reads a row out of SQLite and
writes one back.

### The server is the same store

`armory-server` opens the same schema through the same `armory-core`. It plans
no run, evaluates no criterion, costs no craft and writes no journal entry: it
takes rows, applies them with exactly the rules a client would, keeps a log of
what landed, and hands each machine everything in that log it did not write
itself. A server that starts answering "what is left to do" is a second Armory
that can disagree with the first.

**This parts from brain-server and planner-server, which both take Postgres**,
and the argument that put them there points the other way here. Planner's whole
server job is one atomic refuse-a-stale-write, which against a directory of files
is a lock plus a read-modify-write; a real database is the honest way to get it.
Armory's arbitration is not one rule but a set of them per table, every one
arrived at from evidence, and every one already written in SQL against SQLite
with tests around it. Porting that would buy concurrency this does not have —
one person, three machines, a mutex around the store — at the cost of the
property worth the most, which is that both ends run the same code and
`save_collected`'s merge cannot come to have two definitions.

That is also why there is a workspace. The core is a crate rather than a
directory because the server cannot link libadwaita to reach a schema; it is
re-exported as `armory::model`, so every path in the application and its tests
still means what it did. `server/README.md` covers the routes, the auth and
standing one up.

### Recording is a trigger, not a call at each write site

The log is kept by SQLite triggers generated from `sync::TABLES`, one set a
table, rather than by a `note()` at the end of every write. There are two dozen
ways to write to this store and each of them would have to remember; the
twenty-fifth, added in a year, would not, and what that looks like is one table
that quietly stops travelling between machines. As a trigger, recording is a
property of the table and a new writer gets it without knowing this exists.

Two things fall out of it. `WHEN old.c IS NOT new.c` means an upsert that writes
the values already there logs nothing. And the trigger builds the key with
`json_array`, in exactly the encoding `serde_json` produces for the same values,
so the key is read straight back out with no agreement to maintain between the
SQL and the Rust.

### A wholesale table rewrite had to go

Every table that is *replaced* rather than merged used to be written the obvious
way: `DELETE FROM criterion`, then insert the lot. Invisible to a store nobody
else reads, and ruinous to a log — it leaves the table exactly as it was and
tells the log that fifty thousand rows moved. One addon read would queue the
whole account to be sent to every other machine, saying nothing, every time
SavedVariables changed.

`store::reconcile` is the shape those writers take now: upsert every row, then
delete whatever is not among them, narrowed by `within` to one realm or one
recipe or one kind where the replacement is only of that part — a snapshot
replaces a realm and must not empty the others while it is there. An upsert
whose values match writes nothing and the triggers stay quiet, so a repeat write
costs one statement a row and enqueues none of them. The keys are gathered into
a temporary table rather than an `IN (?, ?, …)` list, because these lists run to
tens of thousands and SQLite's variable ceiling is a few hundred: the list form
fails at the point an account gets large, which is the point nobody is testing.

**A pass that is not empty when nothing happened means something is re-uploading
the account on a timer**, and there is a test for exactly that.

### The stamp a row is judged by is not itself news

Four tables are guarded on a timestamp their writer already keeps —
`detail.fetched_at`, `snapshot.seen_at`, `entry.written_at`,
`response.fetched_at` — so an arriving row lands only if it is at least as recent
as the one held. Those same four columns carry `Rule::Stamp`: written like any
other value, never the reason a row travels. The correspondence is the point,
and there is a test that no guard column is anything else.

Without it every one of those tables re-sends itself on a timer.
`store_response` writes `fetched_at` on every conditional request, so a body that
came back unchanged would travel again in full; a realm's snapshot rewrites
`seen_at` on tens of thousands of rows every hour, so an idle market would push a
realm an hour to announce that nothing had happened.

It costs one thing. A row whose stamp moved and whose contents did not stays on
the machine that refreshed it, so another machine's copy is stamped when the row
last *changed* rather than when it was last confirmed.

### Every change carries the machine that wrote it

A pull excludes the caller's own. Without that, a client's first push comes
straight back down as a pull — fifty thousand rows it already has, applied to no
effect, on every machine's first day. The id is a random string made once per
install and kept in the database beside the cursor rather than in
`settings.json`: copying a settings file between two machines to set them both up
is a thing somebody will reasonably do, and two installations sharing an id means
each is handed the other's rows as its own and neither ever pulls anything.

The same flag decides whether a write is recorded at all, and it is off for
exactly two things. Applying what was pulled, because a pulled row enqueued for
pushing is a row two machines hand each other forever. And the local expiry
sweep, because **a sweep is one machine's housekeeping and not a statement that
the data is gone.** With recording on, a laptop that had been switched off for a
month would come back, sweep, and take that month off everything else — and
nothing about it would look like a bug.

### What does not sync

Nothing, by category. There is no opt-out list to keep in step with the schema:
a table is either in `sync::TABLES` or it does not travel, and a test says which
of those is true for every table in the store. The only ones left out are the
change log itself and the small `sync_state` table beside it that holds the
cursor and this installation's id.

The single exception is a size ceiling rather than a kind of data. A cached body
over `MAX_BODY` — four megabytes — is left where it is. Every profile,
catalogue, media and game-data body Armory holds is a few hundred kilobytes at
most and passes comfortably; one class of body does not, and that is a connected
realm's auction dump: tens of megabytes, replaced every hour, which between three
machines is gigabytes a day to re-send something both ends can fetch in seconds
and which is *already* reduced into the `snapshot` and `price` rows that do
travel. The row is dropped whole rather than truncated, because half a response
is worse than none — the cache would answer with it — and nothing depends on its
presence: a missing body is the ordinary cache miss.

### Proving it, with two machines

`./sync-check.sh` starts a throwaway server on loopback and drives two real
stores through the real client transport. Everything in `replica` is a pure
function with unit tests and everything in `armory-server` has its own, but
between them sit a wire format, a transport and the question of whether the two
ends agree about what a row is. One machine can never ask that, and there is no
second platform coming along to shake it out by accident. It is not part of
`./test.sh` because it starts a process and binds a port.

### What this gives up

Nothing detects a real conflict. For a column that simply takes the arriving
value — a run's name, a goal's attestation, which run is the current one — the
last write to reach the server wins, the other is gone, and nothing anywhere
records that there were two. That is the right trade for data that is recorded,
where the two machines were going to write the same row anyway. It is the wrong
one for a decision somebody made twice, offline, in two places, and there is no
warning when it happens.

## Collections

The API says what is collected. It deliberately does not say where anything came
from: `/collections/transmogs` returns appearance IDs with no source item, which
Blizzard has confirmed is intentional, and `/data/wow/mount/{id}` gives one coarse
word for mounts and nothing at all for pets and toys.

So the catalogue comes from elsewhere:

- **AllTheThings** (MIT) — the obtain logic: quest chains, vendors, drop hierarchy,
  crafting. Parsed from its Lua tables. It has no stable schema and drifts every
  patch, so the parser reports `Stale` rather than guessing, and the version it was
  built against is recorded.
- **Rarity** (GPLv2) — estimated drop rates, the best bulk figures obtainable
  legitimately.
- **wago.tools** — DB2 CSV exports for the catalogue itself: `Mount`,
  `BattlePetSpecies`, `ItemAppearance`, `TransmogSet`, `Achievement`,
  `CriteriaTree`.

**Wowhead is scraped by nobody here.** Its robots.txt names `anthropic-ai` and
`ClaudeBot`, and its terms forbid automated access. Linking to it is a different
act entirely and is what "point me at a guide" means: a deep link to
`wowhead.com/mount=…` next to every goal, alongside anything else worth reading.
Reading a page is the user's business; fetching it is not ours.

Icons come from `/data/wow/media/{type}/{id}`, which returns a hotlinkable
`render-us.worldofwarcraft.com` URL. That is the sanctioned path.

### What to tackle next

Uncollected items are ranked for the cohort, not in the abstract. The inputs:
whether any enrolled character can actually obtain it, estimated drop rate,
whether it is lockout-gated and when that lockout resets, how much of the
prerequisite chain is already done, and whether it is on a seasonal or removal
deadline.

The ranking shows its working, the way Sleeve's does. A recommendation that cannot
be interrogated is a recommendation that gets ignored the first time it is wrong.

## The chronicle

Everything above is a question about *state*: what is owned, what is left, what
it is worth. The chronicle is the only part of Armory about *time* — what
happened, in what order, on one evening.

That is not a small distinction, because it changes where the data can come
from. **Blizzard's profile API has no history in it at all.** It is a logout
snapshot: it will say a character has completed 4,312 quests and never which
twelve of them were finished tonight, never which zone they were standing in,
never that they died twice to the same rare. No amount of syncing produces a
sequence. So the chronicle is fed entirely by the addon, which has a useful
consequence — it works with no Battle.net client, no token, no quota, and no
thirty-day term, because none of it was obtained through the API.

### Three steps, and only the first two are required

**Session.** What the addon recorded, in order: zone changes with their
subzones, instances entered and at what difficulty, quest accepts and turn-ins
with the campaign each belongs to, levels, deaths and what caused them,
encounters won and lost, named rares, keystones with their timer, rare-and-above
loot, auction-house mail, achievements, new collectibles, appearances, gear
upgrades, profession skill-ups, party members — plus two session totals, a kill
count and the reputation ranks crossed. Kept per character, because a session is
a sequence and twenty-three characters' worth in the account file would run at
the Lua literal ceiling the collector already works around.

**Digest.** The same evening rolled up — a route rather than sixty zone events,
two books of money rather than four hundred deltas. This is what a card shows,
and it is complete on its own.

**Entry.** The prose, written by a language model from the digest.

The third step is optional and the second is not. An evening that is never
written up is still recorded, still shown and still worth having. That ordering
is the whole design: the card leads with the facts, and the write button is an
ordinary button rather than the page's suggested action, because the page is
not trying to sell anybody an entry.

### The story, in the game's own words

`GetQuestText` and `GetRewardText` are the sentences the game put on the screen
— the premise the quest giver gave, and what was said at the turn-in. No
endpoint returns them. They are the difference between "completed 12 quests" and
an entry that knows what the quests were *about*, and they cost nothing to
capture because the player already read them.

`C_CampaignInfo` is the other half. It answers which storyline a quest belongs
to and hands over Blizzard's own paragraph describing it, which turns a dozen
scattered turn-ins into chapters of one arc. Between the two, the chronicle has
better lore for an evening than any third-party summary of it — first-party
text, about the exact quests the player did, at no request cost.

There is a third source of written content, and it is the one nobody thinks of:
**what the world says while you are standing in it.** An NPC talking to another
NPC, a boss yelling mid-pull, an escort narrating itself, the emote a rare does
before it charges. All of it is scripted, all of it was written by somebody at
Blizzard, and none of it exists in any endpoint. `CHAT_MSG_MONSTER_*` and
`CHAT_MSG_RAID_BOSS_*` hand it over for free, and a model given the evening's
actual dialogue stops having to invent atmosphere for it.

Three channels, not one, and the obvious one is the least of them. NPC chat
(`CHAT_MSG_MONSTER_SAY`/`YELL`/`EMOTE`/`WHISPER`) is what most people think of.
`CHAT_MSG_MONSTER_PARTY` is escort followers and bodyguards, on their own event.
And the **talking-head bar** — the cinematic strip since Legion — raises
`TALKINGHEAD_REQUESTED` and no chat event at all, which makes it the easiest
thing in the game to miss and the best-written: it is the dialogue Blizzard paid
to have voiced.

Cutscenes are recorded as events rather than as content. A pre-rendered one
raises `PLAY_MOVIE` with a `MovieID` that names it exactly; an in-engine one
raises `CINEMATIC_START` with no identifier at all, so what is kept is that it
happened and where, and the quest turned in a moment later is what names it in
practice. Subtitles are not available to an addon — but an in-engine cutscene
usually speaks through the ordinary monster chat events, so the words are
captured anyway.

There is a fourth channel, and it needed a different rule: **gossip**, the text
an NPC shows when you click them. It is captured too, but kept as its own kind,
because it is something the player *chose* to read rather than something they
happened to be standing next to — and a great deal of it is a shopkeeper's
greeting. No heuristic filters it. A length threshold would drop a short line
that mattered and keep a long one that did not; instead the log labels the
section and the journal's instructions tell the model to take what carries the
evening and ignore the rest without remarking on it. Capture is cheap and
judgement is what the model is for.

Only NPCs. The events that carry player chat are deliberately not registered:
what somebody said in party is their business and none of it belongs in a file
another program reads. It has its own budget rather than a share of the event
cap, because ambient chatter arrives faster than anything else in the game and
would otherwise crowd out the quests, the deaths and the loot.

So the chronicle fetches nothing. Where somebody wants to read *further* they
get links, the way collections already do: quests and achievements to Wowhead,
zones to `warcraft.wiki.gg`, and per-zone searches on Nobbel87 and The Karazhan
Library for anybody who would rather watch than read.

### Where the gold went

A purse is a net figure and a net figure hides the evening. Down forty gold is
the same number whether nothing happened or three hundred was earned questing
and three hundred and forty spent at the auction house, and those are not the
same evening.

The game raises `PLAYER_MONEY` with no reason attached, so the reason has to be
inferred — and everything in this game that takes or gives gold does it through
a frame. The frame that is open when the purse moves *is* the attribution:
merchant, auction house, mailbox, trade, flight master, trainer, transmogrifier,
barber, guild bank. Two more are not frames and need no inference: a quest
reward, which arrives with its own event, and coin off the ground, which is a
gain with nothing open. Three refinements sit on top — a vendor charge matching
the drop in `GetRepairAllCost` is a repair rather than a purchase, an auction
charge within five seconds of `AUCTION_HOUSE_AUCTION_CREATED` is a deposit
rather than a bid, and anything else is `unknown` rather than a guess.

The ledger is then the **only** set of books. Quest rewards and auction mail are
still itemised as their own moments, because "Mycobloom sold for 374g" is worth
saying, but the totals come off the ledger and only off the ledger — counting a
reward once as a moment and again as a delta is how a journal ends up claiming
an evening earned twice what it did.

### The counters nothing else keeps

Some things are only interesting as a total. "Made two flasks tonight" is barely
a fact; "has made four hundred and twelve" is a character. The same holds for
who keeps turning up in the party, which boss has been pulled eleven times,
where the hours actually went, and what keeps killing somebody — and Blizzard
records none of it at any granularity worth having. The statistics pane counts
a few professions in the aggregate and no individual recipe, counts no party
members at all, and forgets a boss attempt the moment the pull ends.

So the addon keeps its own, and `model/tally.rs` is one table for all of them:
a `(kind, key)` and a number. That generalisation happened at the *second*
counter rather than the fifth, which is the whole reason there are nine of them
now and one reader.

| Counter | Keyed by | Where it comes from |
| --- | --- | --- |
| Recipes made | Spell id | `UNIT_SPELLCAST_SUCCEEDED` filtered through `GetRecipeInfo` |
| Evenings alongside | Name | `GROUP_ROSTER_UPDATE`, once per person per session |
| Boss attempts, and defeats | Encounter | `ENCOUNTER_END`, both outcomes separately |
| Hours per zone | Zone | The interval between zone changes, closed at logout |
| Deaths | What did it | The last thing to damage the player before `PLAYER_DEAD` |
| Distance | On foot, by flight | `GetWorldPosFromMapPos`, sampled once a second |
| Flights | Where from | `UnitOnTaxi` a second after the map closes |
| Delves | Tier | `C_DelvesUI.GetActiveDelveTier`, at completion |

Every one is written twice on purpose: into the session, where it is one
evening's work, and into a running total in the account file, where it is the
lifetime. The store merges those totals by taking the larger of the two, for the
same reason `earned_reputation` does — a reinstalled addon starts at one, and a
year of somebody's work must not be erased by a cleared folder.

They are drawn in one group of shut expanders above the cards, and only when a
single character is being looked at. "Everyone has made four hundred flasks" is
nobody's achievement, and eight open lists would push tonight's evening off a
page that is supposed to be a journal.

### Three numbers and a join

Three things an evening has that a list of events does not. The **longest single
fight**, from `PLAYER_REGEN_DISABLED`/`ENABLED`, which is the difference between
a boss that took eleven minutes and an evening of six-second pulls. The
**hardest single hit taken** and the **lowest the health bar got** — the near
death somebody would actually be thinking about afterwards. All three are
session fields rather than moments, because they only mean anything once the
evening is over.

And one join. Nothing in the game connects a piece of gear to the thing that
dropped it: the loot event names an item, the encounter event names a boss, and
they sit minutes apart in several hundred rows. `Digest::where_it_came_from`
puts them back together — the item has to have been *looted* tonight, and
something with a name has to have died in the thirty seconds before. Outside
that window the answer is nothing at all, because gear is also bought, crafted
and handed over by quest givers, and none of those has a boss to name.

### Specialisations, which no endpoint has

Two characters with Alchemy at 100 can have spent a year of weekly knowledge in
completely different places, and the profile API cannot tell you which — it has
the expansion tier and stops. `C_ProfSpecs` has the trees, which tabs are open,
and the knowledge currency behind them.

That makes professions the one field on `Detail` where both halves matter, so
the API's answer is **merged** into what the addon read rather than assigned
over it. A straight write is the same bug as flattening a collectible: one
profession sync after a logout and the trees are gone from every character.

They live in the roster row's tooltip rather than in its subtitle. Two
professions carrying a tree name each wraps the row onto a second line, and the
scannability of that column is worth more than a fact somebody looks up once.

Brann's level needs nothing at all, incidentally. The delve companion is a
Warband reputation, so it already arrives through the reputation path with
every other faction.

### A model on this machine

Entries are written by a `llama-server` over llama.cpp's OpenAI-compatible
`/v1/chat/completions`, at `http://127.0.0.1:8080` by default — the same server
the sibling application Familiar drives. That decision carries most of this
section:

- **No credential.** Nothing in the keyring, nothing to register for, nothing to
  leak. The prerequisite is a server that is already running, and `/props`
  answers both "is it there" and "what is it called".
- **No bill.** Which is why each new evening is written automatically. The
  reason to hold back was somebody's money; there is none at stake, and a
  journal you have to remember to write does not get written.
- **Nothing leaves.** A journal is a record of somebody's evenings, and the
  sentences in it stay on the machine that recorded them.

The response shape is constrained by a JSON schema, which llama.cpp compiles to
a grammar the sampler cannot leave — a stronger guarantee than a hosted API
offers. The one thing a grammar does not constrain is what a chat template
prepends, so a `<think>` block is cut off the front before parsing.

The model is given the evening's log and told, in as many words, not to invent
events, people, places or outcomes — and that a boss the log says was lost to
was lost to. What it may add is framing: it knows who the Mag'har are. Every
entry records which model wrote it and when, and says so on the card, because
that paragraph is the one thing on the page that is not a measurement from the
person's own machine.

### Kept, not purged

`session` and `entry` are the two tables `Store::purge` deliberately does not
touch. The thirty-day term is a condition on data obtained through Blizzard's
API; this came from the addon, which is to say from the user's own client
recording the user's own play. A journal you are not allowed to keep is not a
journal. Individual evenings can be forgotten by hand, which matters more here
than elsewhere — this is a record of somebody's hours, and a paragraph of it
goes to a third party whenever they ask for an entry.

## The auction house

Opt-in twice: per connected realm, and per watchlist item. Five realms of
non-commodity data is roughly 3 GB a day of raw JSON, and ingesting all of it by
default to answer questions nobody asked is how a desktop application becomes a
service.

Snapshots refresh hourly. `If-Modified-Since` gives a free 304 everywhere except
commodities, which costs 25x quota per call and is charged that even for a 304 —
so commodities is polled on a schedule, not on speculation.

What a snapshot keeps per item is the *shape* of the book, not just its first
row: the cheapest unit price, the total quantity, how many separate auctions
those units are spread across, and the unit price a tenth and half of the way in
by quantity. Six numbers rather than two, still one row per item per hour. The
cheapest price alone cannot tell one lowball at a hundred gold from four hundred
units at a hundred gold, and every question worth asking — how deep is this, what
would twenty cost me, is that floor one goblin — is about the shape.

The clock is kept with them. A count of what sold is meaningless without the
span it covers, and the span is what was *observed*, never the thirty days the
store may keep: a realm watched since Tuesday has four days of evidence.

Thirty days is a ceiling, not a default. It is a term of the licence rather than
a cache policy, so the answer to "we need more history" is richer rows inside the
window and never older ones.

### Browsing versus watching

These are two questions and confusing them is what makes an auction feature
either useless or unaffordable.

**Browsing is about now**, and it is free. The full realm dump and the
region-wide commodity file are downloaded in their entirety every hour anyway —
what used to happen is that everything outside the watch list was parsed and
then thrown away. One table, replaced whole each sync, turns that into a market
somebody can search. It is replaced rather than merged so that an item which has
left the auction house disappears instead of sitting there looking current.

**Watching is about history**, and it is the expensive half: it accumulates, it
carries the thirty-day obligation, and it is the only thing that can answer
"what has this been doing". So it stays opt-in, and the browser is where you opt
in — you find something, you open it, and the one action on that page is to
start recording it. Nothing can be recovered from before you ask, which the page
says outright rather than showing an empty chart.

The one thing the auction house will not tell you is what anything is *called*.
A listing is an item id. The search endpoint goes the other way, from a name
somebody typed, and there is no bulk lookup — so names are fetched one at a time
and the browser shows an id until one arrives, exactly as the collection pages
show a placeholder until artwork does. The budget is spent on the rows currently
filtered and sorted onto the screen, because spending it in database order
scatters names through a page with no pattern and leaves the top of it blank for
weeks.

Storage is per-item deltas in SQLite, never stored snapshots. Undermine Exchange
holds 186 realms in ~56 GB by doing this; storing raw hourly JSON is not a thing
that works. There is no sale signal in the data — quantity simply disappears
between snapshots — so sales are inferred by diffing and labelled as inferred.

History has a 30-day horizon, per the terms above. Longer-range questions get
answered by daily aggregates, and where they cannot be, they are not offered.

Blizzard publishes no dictionary for bonus IDs, modifier types or item context
values, so gear pricing is approximate and says so.

### Crafting flips

The question is "which of my characters should make what". Three things have to
meet for it to be answerable, and only one of them was already here.

**The market.** Region-wide commodities are fetched already, and reagents are
commodities, so nearly every input is priced by a call Armory makes anyway. What
was missing was retention: `Application::record` kept watched items and pets and
threw the rest away. It now also keeps whatever some character's recipe names —
bounded by the account's own books, so an account with no books records nothing
extra.

**The recipe book.** The crafting tally says what somebody has *made*; this
needs what they *can* make, and nothing in the API knows recipes exist. The
addon reads it, with one hard constraint: `C_TradeSkillUI.GetAllRecipeIDs`
answers an empty table until the profession window has been opened, and there is
no call that substitutes. So it hangs off `TRADE_SKILL_LIST_UPDATE`, and the
user opens each profession once. An empty answer means "not open yet" and leaves
what is stored alone, which is also why the `recipe` tables merge rather than
replace — a character who has opened Alchemy and not Herbalism must keep their
Herbalism recipes.

**The join**, `market::worth_making`, pure like the two beside it. Reagents at
the cheapest tier that has a price, revenue from `quantityMin` after the auction
house's five percent, and the ranking is the part that matters:

> **margin × what has actually been selling**, not margin.

A four-hundred-gold profit on a thing nobody buys is forty unsold flasks. The
liquidity signal comes from `record_prices`'s quantity deltas — Blizzard records
no sale anywhere, so a falling quantity is the only evidence there is — and it
is the one thing Armory has that an ordinary margin calculator does not.

Three refusals hold the rest of it honest:

- **A recipe with one unpriced reagent is unmeasured, not cheap.** The same rule
  as `Evaluation::observable`. It is counted in `Crafting::unmeasured` and said
  out loud, because a page that silently drops what it cannot price presents a
  subset as the whole.
- **Every figure is a one-star craft.** Output quality depends on skill,
  specialisation and inspiration, and Armory reads none of them. Quoting the
  three-star price would be inventing a number about somebody's own character —
  exactly what `Resale::floor` exists to avoid.
- **Warband stock is shown and never subtracted.** The bag indices have never
  been confirmed against a stocked bank. As its own line a wrong index looks
  wrong; taken off the cost it would be an inflated margin nobody could see.

## The interface

GTK 4.22 and libadwaita 1.9, `AdwNavigationSplitView` over the cohort. The run is
the home page, because it is the reason the application exists.

Widget choice, layout and every HIG question go through the `designing-gnome-ui`
skill rather than being derived again here.

### Threading

`soup3`'s async calls complete on the GLib main loop, so syncing twenty-three
characters is `spawn_future_local` and no threads. The addon file watch is a
`gio::FileMonitor`. The one genuinely long job — parsing AllTheThings, and any
bulk auction ingest — runs on a worker thread with a channel, because it is
CPU-bound and would otherwise stall the frame.

Widgets report what a person did. `ui/application.rs` is the only object that
mutates state or asks a source anything.

## What this deliberately does not do

- **Automate anything in game.** No posting, no cancelling, no scanning on a
  timer, nothing that touches gameplay. The addon reads and writes files.
- **Scrape Wowhead.** Links out, always.
- **Send a journal anywhere.** Entries are written on the machine that recorded
  them.
- **Pretend to be live.** Profile data is a snapshot written when a character logs
  out. Every view carries the time it was taken, and none of them imply otherwise.
- **Support Classic.** The endpoints are broken and Blizzard is not fixing them.
- **Rank things it cannot explain.** A score with no visible working is a score
  that will be wrong once and distrusted forever.
- **Keep a backlog of the impossible.** Spent goals are reported once and then
  filtered out.
