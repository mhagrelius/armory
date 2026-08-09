# armory

A World of Warcraft companion. Tracks an account, and measures a *run* — a
replay of content the account already remembers — rather than reading Blizzard's
completion flags.

## Stack

GTK 4.22 + libadwaita 1.9 via gtk4-rs 0.11 / libadwaita-rs 0.9, Rust edition
2021 (MSRV 1.80). `gio` is a direct dependency purely to raise the API level to
v2_80 — leave it.

Beyond the sibling baseline: `rusqlite` (the store), `soup3` (all HTTP *and* the
OAuth loopback listener). Each is justified where it is declared in
`Cargo.toml`; read that before adding a third. There is no LLM client library
either — the journal's request is one JSON POST at llama.cpp's
OpenAI-compatible endpoint, `model/source/journal.rs` builds it and
`ui/http.rs` performs it like every other request.

There is no libsecret dependency. `ui/keyring.rs` talks to
`org.freedesktop.secrets` over D-Bus through `gio::DBusConnection` — same
service, no `libsecret-1-dev` to build against, nothing extra in the Flatpak.

Three crates in a workspace. The root package is the GTK shell, a lib + bin so
integration tests and `examples/` drive the real application rather than a copy
of it. `core/` is `armory-core` — everything that was `src/model/`, re-exported
from `src/lib.rs` as `armory::model`, so every `model::…` path still means what
it did. `server/` is `armory-server`.

The core came out for one reason: **`armory-server` is that same store with a
socket in front of it, and it cannot link libadwaita to get at the schema and
the merge rules.**

## Commands

- `./test.sh` — fmt check, clippy with `-D warnings`, luacheck if installed,
  then `cargo test --workspace --all-targets`. Add `--headless` to run under
  Xvfb + a private D-Bus session. This is the gate; run it, not bare
  `cargo test`. **`--workspace` is not optional** — without it cargo checks
  only the root package and `armory-core`, which is most of the suite, silently
  stops being tested.
- `./sync-check.sh` — two stores, one throwaway `armory-server` on loopback,
  the real client transport. Not part of `test.sh` because it starts a process
  and binds a port; run it after anything that touches sharing. It needs no
  network and no NAS.
- `packaging/deploy-server.sh` — test, build, smoke-test the image, push to the
  NAS registry. `server/README.md` has the rest of the path, and `SETUP.md` is
  the client half: what a second gaming PC needs, start to finish.
- **Never run `dbus-run-session` or `xvfb-run -a dbus-run-session` directly** —
  use `isolated-bus [--headless] -- CMD`. A private bus activates its own
  `xdg-document-portal`, which mounts over `/run/user/$UID/doc` and takes the
  login session's portal down with it when the bus exits; every flatpak on the
  machine then fails to launch until it is restarted. `test.sh --headless`
  guards against this internally, but one-off runs of a single test, or of the
  built binary, bypass it.
- `./install.sh` — release build, installs under `~/.local`. `./uninstall.sh`
  reverses it. `./install-addon.sh` copies the addon into a WoW install.
- `cargo run --example preview -- /tmp/preview [dark]` — paints the real pages
  offscreen to PNGs. This is how a UI change gets looked at; GNOME will not give
  a screenshot to a non-interactive caller.

No test touches the network. `test.sh` sets `GTK_A11Y=none` and
`GSETTINGS_BACKEND=memory` so tests never touch real user state — keep that
true for anything new.

## Layout

`core/src/` is pure logic with no GTK types, reached as `model::` from the
shell. `src/ui/` is widgets and the application. `server/src/` is the shared
account. `addon/Armory_Collector/` is the in-game Lua. Read `DESIGN.md` and
`RESEARCH.md` before proposing structural changes; both are current, and
`RESEARCH.md` carries the evidence for every claim the design rests on.

The seam that makes the tests possible: **`model/source/*` builds request URLs
and parses response bodies; `ui/http.rs` performs the requests.** Nothing in
`armory-core` opens a socket. `ui/redirect.rs` is the only file that listens on
one. Three files open one — `ui/http.rs` for Blizzard, `ui/images.rs` for the
render service, `ui/sync.rs` for the account's own server — each with its own
client, because a rate gate built for Blizzard's quota has no business slowing
a scrolling grid or a push to the NAS.
Widgets report what a person did; `ui/application.rs` is the only object that
mutates state or asks a source anything.

`model/chronicle.rs` is the same seam applied to a language model: a `Session`
is what the addon recorded, a `Digest` is that rolled up, and
`journal::brief` turns a digest into the words a model is given. All three are
pure functions, so the prompt has tests and nothing has to be mocked to check
what an entry was written from.

`ui/images.rs` is the one thing that fetches bytes rather than JSON, and it goes
through `ui/http.rs` like everything else. It holds its own `Http` because the
render service is not an API host: no token, no namespace, no quota, so putting
image traffic in the same rate bucket as a sync would make a scrolling grid slow
the sync down.

`model/sync.rs` and `model/replica.rs` are the same seam applied to a second
machine: `sync` says what a row is on the wire and `replica` reads one out of
SQLite and writes one back. Both ends run them — `armory-server` is
`armory-core`'s store with a socket and a mutex, and no second opinion about
anything.

`model/store.rs` touches a local SQLite file and `model/addon/*` reads Lua off
disk. Both are deterministic, neither goes near the network, and both are tested
against real files — the seam worth defending is the network one, not the disk.

`ui/almanac.rs` is the vocabulary every page is drawn in: the palette, the type
helpers, the drawn widgets (ring, momentum strip, bars, sparkline, spine,
ledger, depth) and the two-pane `split`. Three rules carry the whole interface —
**every page is a main column and a right rail**; **gold means "you earned
this"** and is spent on nothing else; **numbers are monospaced, narrative is
serif**, everything else the platform font. A page that reaches past `almanac`
for a colour or a size is a page that will drift from the other six.

## Where the build is

| Area | State |
| --- | --- |
| Roster | Done bar the Great Vault, which has no endpoint — see the note on `Detail`. A card opens a character page. |
| Character | A page a character, reached from a Roster card: header, a stat strip whose gold/not-gold split is the accent's rule in miniature, a dated spine of firsts, gear sorted **weakest slot first** with empty slots drawn as empty, the lifetime record, hours by zone, this season's keystones and raid progress. Rail: professions, share of the run, when they play, who they play with. **No character age** — neither the profile API nor any in-game API reports a creation date, so the header says how much Armory has watched instead. |
| Collector addon | A complete data source, not a supplement: roster, achievements with criteria trees, collections with real source text, attribution, currencies, Warband bank. Two files — account-wide and per character — because a quest list is thousands of ids and twenty-three in one file would hit the Lua constant-table ceiling. Channel-*out* unimplemented. Has now been run in game: the mount, pet and toy journals, the criteria table and the currencies all came back. The Warband bag indices are still unverified — the bank read empty, which is either an empty bank or the wrong index. Pets carry two extra columns — `isTradeable` and the journal's collected count — which is what `market::worth_selling` needs and what an older collector file does not have; the reader treats their absence as silence rather than as no. |
| Run | Baseline, planning, chain resolution, attestation, hand exclusion and persistence all working. One run at a time; no run picker. |
| Achievements | Criteria evaluation, catalogue join, closest-to-done ranking, dependency chains. Names fill in 200 per sync. No category browsing. |
| Collections | A page each for mounts, pets, toys and housing decor: a `GtkGridView` over the whole catalogue, illustrated, searched as you type, grouped by source, filtered from the rail. An owned entry is dashed and dimmed with "already owned" rather than ticked — in a run, owning something is the bad news. The three closest to earning carry **the drop chance and how many times this account has pulled the thing that drops it** — `1 IN 20` over `58 TRIES`. The tries come from the addon's own counters (`tally::attempts_at`); the odds are read out of an installed Rarity (`model/rarity.rs`) and are absent without it. No transmog. |
| Bags, banks, currencies | Warband bank and currency counts on the Roster page. No per-item bag browsing. |
| Auction house | Opt-in realms and items, both chosen from the UI now: a realm picker built from `/data/wow/realm/index` with the account's own realms first, and an item search against `/data/wow/search/item`. Delta-stored history, a sparkline per row, cross-realm comparison, token price. No arbitrage suggestions. |
| Housing | Decor is a fourth collection, from `/data/wow/decor/*` and `/profile/user/wow/collections/decor`. The one system where the API is ahead of the addon — the collector does not read the housing catalogue at all. No house editor, and quantities are dropped: decor is owned in counts and Armory records owned or not. See `RESEARCH.md`. |
| Market | Realm and item watches are both chosen from the UI. **Crafting flips**: `market::worth_making` joins the account's recipe books against the market, costs each craft at the cheapest priced reagent tier, and ranks by margin *against what has actually been moving* rather than by paper margin. The books come off the addon one profession window at a time — `GetAllRecipeIDs` answers nothing until one is open. `model::market::on_sale` joins the missing collection against a realm snapshot, which is the one feature worth having from WoWthing: caged pets carry a species id on the listing, so a missing pet that is for sale can be named exactly. `market::worth_selling` runs the join the other way — spare pets that can be caged, against thirty days of per-species prices, ranked by realm. It quotes the cheapest quality's price because the journal's per-pet quality is not read yet; see the note on `Resale::floor`. |
| Zones | A page a place, joined on `UiMapID`: what it is (the corpus in `data/zones.json`, 143 entries in Armory's own prose from the wiki and all four Chronicle volumes), what happened in its dungeons (Blizzard's Adventure Guide, with `data/instances.json` standing in for the 21 raids older than Mists that the guide never wrote up), and what *you* did there (evenings, hours, quests, deaths, rares). The corpus is compiled into the binary, so a zone page costs no request and works offline — which is the whole reason it is our prose rather than pasted wiki text. |
| Chronicle | A journal, one card an evening. The addon records sessions as they happen — route and instances, quest turn-ins **with the game's own quest text and the campaign they belong to**, levels, deaths *and what killed you*, bosses won and lost, named rares, keystones with their timer, rare-and-above loot, auction mail, achievements, new collectibles, appearances, gear upgrades, skill-ups, reputation ranks crossed, a kill tally, party, **a gold ledger by source and destination**, **a gold ledger by source and destination** that tells an auction sale from an alt's transfer, **the distance covered, the longest fight and the hardest hit taken**, and **a set of lifetime counters** (`model/tally.rs`) — recipes made, people played with, bosses pulled and beaten, hours per zone, what keeps killing you, delves by tier, flights taken. A gear upgrade is joined to what dropped it. Addon-only, so it works with no Battle.net client. The card stands alone; prose on top is written by a **local `llama-server`** (`model/source/journal.rs`, OpenAI-compatible, `127.0.0.1:8080` by default) — no key, no bill, nothing leaves the machine, automatic by default. No lore is fetched from anywhere. |
| Reputations | Per character or per faction, with inherited standings marked rather than counted — **and, where the addon has been watching, what this character personally earned toward one anyway.** See `model/provenance.rs`. |
| Sharing | A change log and a cursor. Each machine keeps the whole account and works with the server off; `armory-server` is where they meet. Recording is done by SQLite triggers rather than by a call at each write site, so a writer added later is shared without anybody remembering. Main Menu → Sharing… is the triage: what is queued to go up by table, what the last pass moved, and what the other end could not read. `./sync-check.sh` drives two machines against a real server. |
| Provenance | Who actually earned the account's account-wide progress. The addon snapshots every faction and currency at login and diffs at logout, accumulating per-character totals; one client means one character at a time, so a rise between a login and a logout is that character's work. Feeds `PrimaryData::earned_reputations`, which is what lets a poisoned reputation criterion be measured at all. Currency additionally answers earned / transferred / already-held / **unclear**. |
| Artwork | Mount and pet renders and class crests are addressed directly on `render.worldofwarcraft.com` and cost no request. Toy icons, achievement icons and character portraits need one call each and fill in over successive syncs, from the top of the page down, or all at once from the menu. `ui/images.rs` caches under XDG cache with the same thirty-day sweep as the store, and `Application::restore_art` re-derives the URLs from the response cache at startup so a launch keeps what the last one earned. |

**Armory works with no Battle.net client at all.** `Settings::addon_only` records
that choice, and the addon supplies everything except the Market tab and alts
you have never logged in on. This is not a fallback bolted on — Blizzard's
developer portal has been answering 500 to client creation since late 2025, so
the API is the optional half.

## Things that will bite

- **The change log is written by triggers, not by anything you can call.**
  `Store::triggers()` generates three per table from `sync::TABLES` and they do
  the recording. There are two dozen ways to write to this store; asking each
  of them to remember means the twenty-fifth, added in a year, will not, and
  what that looks like is one table that silently stops travelling between
  machines. Two things fall out of it: `WHEN old.c IS NOT new.c` means an
  upsert that writes what is already there logs nothing, and `json_array` in
  the trigger builds the key in exactly the encoding `serde_json` produces for
  the same values, so `replica` reads it back with no agreement to maintain.

- **A wholesale table rewrite is now a bug, and `reconcile` is the fix.**
  `DELETE FROM criterion` followed by fifty thousand inserts leaves the table
  as it was and tells the log that fifty thousand rows moved — one addon read
  would then queue the whole account to be sent to every other machine to say
  nothing. `save_roster`, `save_cohort`, `save_collected` (five tables),
  `record_snapshot`, `save_owned`, `save_run` and the recipe reagents all go
  through `reconcile` now: upsert, then delete what is no longer among them,
  scoped by realm or recipe or run where that matters.
  `a_second_identical_write_enqueues_nothing` is the test, and a new writer
  belongs in it.

- **The column a row is guarded on is never itself the reason it travels.**
  That is `Rule::Stamp`, and there is a test asserting the correspondence.
  `store_response` rewrites `fetched_at` on every conditional request, so
  without it a body that came back unchanged would travel again in full; a
  realm's snapshot rewrites `seen_at` on tens of thousands of rows every hour.
  What it costs is small and worth knowing: another machine's copy is stamped
  when it last *changed* rather than when it was last confirmed.

- **Recording is a flag in the database, so only one thread may ever write.**
  `Store::apply` turns it off so a pulled row is not enqueued to go straight
  back up — and that flag belongs to the file, not to a connection. A second
  writer applying a pull would silence the first writer's changes for as long
  as it took and neither would know. So the shell runs the network on a worker
  through `gio::spawn_blocking` and does every read and write on the thread
  that owns the database. A per-connection version of this was tried:
  SQLite refuses a trigger that references a `temp` object.

- **A sweep is not a deletion anybody else hears about.** `Store::purge` turns
  recording off for its whole length. With it on, one machine's expiry would
  travel to the server, the server would delete what it holds, and every other
  machine would delete its copy on the next pass — so a machine switched off
  for a month would come back and take the last month off everything else.
  Nothing about that would look like a bug; it would look like the sweep
  working.

- **`run.key` exists because `run.id` is an `AUTOINCREMENT`.** Two machines
  pick different ids for the same run, so a goal could not name its run on the
  wire. Every other table here is keyed by something the game already agreed
  on. Two things about it: the key is derived from the baseline's moment, so a
  replan does not rename the run; and the unique index over it **cannot be
  partial**, because SQLite matches an upsert's conflict target against a
  unique index *including* its `WHERE`, and `ON CONFLICT (key)` would then find
  no index and fail — silently, since `apply` counts a row it cannot write
  rather than losing the batch. `Store::name_runs` backfills the runs that
  predate the column, in Rust rather than SQL, because the stamp comes out of a
  JSON column and `json_extract` spells an instant differently from
  `run_key`.

- **A pull excludes the caller's own rows, and that is what the machine id is
  for.** Without it a client's first push comes straight back down as a pull of
  fifty thousand rows it already has. The id is made once and kept in the
  database beside the cursor, not in `settings.json` — copying settings between
  two machines to set them both up is a reasonable thing to do, and two
  installations sharing an id means each is handed the other's rows as its own
  and neither ever pulls anything. Copying the *database* between machines is
  the thing that would collide.

- **A cached body over `sync::MAX_BODY` stays where it is.** Every profile,
  catalogue, media and game-data body is a few hundred kilobytes and passes it.
  One class does not: a connected realm's auction dump is tens of megabytes,
  replaced hourly, which is gigabytes a day between three machines to re-send
  something either end can fetch in seconds and which is already reduced into
  `snapshot` and `price` rows that do travel. Nothing depends on its presence —
  `Store::response` answering `None` is the ordinary cache miss. The size is
  asked of SQLite before the body is read, so a dump is never pulled into
  memory to be dropped.

- **A batch is bounded by bytes as well as by rows, and the byte bound is the
  one that matters.** Two thousand rows is nothing for most tables and hundreds
  of megabytes for `response`. A client that built a body past the server's
  ceiling would rebuild the same one on every pass — not a slow sync, a
  permanently stuck one. `sync::MAX_PARCEL` is the bound and a batch always
  carries at least one row, so a single large row cannot wedge the queue
  either.

- **The pass loop has one definition and two callers.** `replica::next_step`,
  `absorb_push` and `absorb_pull` hold every decision; `replica::pass` runs
  them straight through for `sync-check`, and the shell runs the same three
  with an `await` between them so the window keeps drawing through a first
  sync. Anything that has to be *decided* goes in the core, or the two come to
  mean different things.

- **The `--al-*` colour tokens are generated from Rust, not written in CSS.**
  `almanac::Palette::css` emits the `:root` block and `ui::load_stylesheet`
  reloads it whenever `AdwStyleManager::dark` changes, because libadwaita gives
  an application no CSS selector for the colour scheme. The palette is in Rust
  rather than in `style.css` so the Cairo draw functions and the stylesheet are
  the same literals — writing a colour into `style.css` by hand puts it beyond
  reach of every drawn widget, and hardcoding one in a draw function puts it
  beyond reach of the theme swap.

- **A `GtkStack` sizes to the widest child, not the visible one.** The window's
  stack of ten places and the market's stack of three tabs are both
  `hhomogeneous(false)` for that reason: with it on, the market's table set the
  minimum width of the Run page, and through it of the window, and the rail was
  pushed off the right edge of the screen. Any new stack of pages needs the
  same, and any widget with a hard `width_chars` or `size_request` in a main
  column is a floor under the whole window — `width_chars(14)` on a grid tile's
  name, seven columns of it, was exactly that.

- **The rail folds before the places sidebar does.** Two breakpoints, at 1160sp
  and 800sp, and the order is the argument: the main column is the thing the
  page is about and the rail is its asides, so squeezing the goals to keep a
  legend on screen has it backwards. `almanac::split` registers every rail in a
  weak list so the window can drive all of them without ten accessors, and a
  rail built *after* the breakpoint fired is born folded.

- **The first breakpoint has to clear the widest page, and `tests/width.rs` is
  what says so.** It was 1080sp while the market page needed 1159sp with its
  rail out, so between 1081 and 1158 the rail was shown on a window with no room
  for it: the content pane overflowed to the right and carried the header bar,
  and the window's own close button, off the edge. The default size of 1180 is
  inside that band, so it was the ordinary case. Nothing fails — one libadwaita
  line on stderr is the entire warning — which is why the budget
  (1180 default − 200 sidebar) is asserted in a test. The test loads the
  stylesheet, because `.al-segment.al-fixed` is a `min-width` and a page
  measured without CSS measures narrower than the one on screen.

- **A paragraph break costs a whole line of prose, at whatever leading it is
  set in.** The journal's body arrives as the model wrote it, paragraphs
  separated by an empty line, so at 1.7 leading a three-paragraph entry spent
  more height on its two gaps than on two of its lines. `almanac::prose` sets
  `LEADING` on the writing and `BREATH` on the blank lines, as two Pango
  attributes with byte ranges — which is the only way to have generous prose and
  a card somebody will scroll to the end of. `blank_lines` marks the newline
  that *ends* an empty line, never the one before it: taking the wrong one
  shrinks the line the prose is on.

- **A drop cap, a floated anything, and `line-height` are not GTK CSS.** Line
  height is a Pango attribute (`almanac::prose` sets it); a drop cap has no
  equivalent at all and the design says to drop it rather than fight it. What
  GTK 4.22 does support and this stylesheet leans on: `letter-spacing`,
  `text-transform`, `opacity`, `font-feature-settings`, `:root` and `var()`.

- **A second character completing an account-wide achievement produces no signal
  at all.** No per-character copy, no second timestamp, no event. `earnedBy`
  names whoever earned it first and goes on naming them forever. This single
  fact is why the run cannot be tracked from completion flags, and it is the
  thing to re-read before "simplifying" `model/run.rs`.
- **Only *poisoned* goals get recomputed.** Unearned and cohort-earned goals are
  a flag read. Recomputing everything was an earlier design and it is the
  expensive, wrong one — on a fresh Battle.net account nothing is poisoned and
  the machinery must cost nothing.
- **Without the addon, every already-earned achievement is assumed poisoned.**
  That is the sound pessimistic reading, not a bug. The addon's `earnedBy` is
  what shrinks the set; it makes the run *cheap*, not possible.
- **An inherited reputation is never progress.** The War Within syncs most reps
  account-wide to the furthest-progressed character, so a standing on a fresh
  alt was very likely earned by somebody else in 2023. `Evaluation::inherited`
  suppresses both completion and the progress bar. Counting it would inflate
  every run and make the whole application worthless.
- **One unknown criterion makes a whole tree unobservable.** A floor is not a
  measurement. `Evaluation::observable` is false for the entire tree if any leaf
  cannot be answered, which sends the goal to attestation instead of to a
  confident, wrong progress bar.
- **`CriterionKind::from_catalogue` claims four criteria types and no more.** A
  wrong mapping is worse than a missing one: missing sends a goal to
  attestation, wrong draws a bar over a number that means something else. Grow
  the table from confirmed data, never by inference.
- **Blizzard's profile response never says what a criterion measures.** It gives
  structure and progress only. Meaning is joined on from a catalogue
  (`Criteria.Type`/`Asset` in the client DB, or the addon). Do not invent kinds
  in `profile::read_criterion`.
- **The keyring attribute is `us.hagreli.Armory`, not `armory`.** It is the
  application id, and a `SearchItems` call with the wrong one answers zero
  unlocked and zero locked — which reads exactly like a keyring with nothing in
  it rather than like a bad query.

- **A blank secret field means "use the stored one".** The field cannot be
  pre-filled without reading the secret out of the keyring to display it, so it
  is empty on every launch; the row's title says whether one is held. Refusing
  to sign in on an empty field sends somebody hunting for a value Armory is
  already holding, which is what happened the first time a session lapsed.

- **Battle.net has no PKCE and no public-client mode.** Confirmed by Blizzard
  dev relations, October 2024. The secret is mandatory, so the user registers
  their own client. Do not "improve" onboarding by shipping one.
- **The redirect port is fixed and registered.** Blizzard matches the redirect
  URI exactly and rejects custom schemes; there is no loopback port wildcard.
  `Redirect` unbinds on drop, and leaking it makes the next sign-in fail with
  something that reads like a Blizzard problem.
- **`CREATE TABLE IF NOT EXISTS` never adds a column.** Adding one to a
  definition in `Store::migrate` reaches new databases only; an existing one
  keeps the old shape, every statement naming the new column fails, and the
  writes are mostly `let _ =` because a name that has not arrived yet is not an
  error — so it fails silently and completely. `item`'s `sellable`/`quality`
  and `price`'s `listings`/`tenth`/`median` were all added this way and a
  running install spent months naming no items and recording *no* price
  history. A new column goes in `Store::ADDED` as well as in the `CREATE`, with
  a default.
- **The 30-day TTL in `model/store.rs` is a term of the API licence**, not a
  cache policy. `Store::purge` runs at shutdown. Do not raise it.
- **Profile data is a logout snapshot.** Blizzard staff, plainly: it changes only
  when the character has logged out. `If-Modified-Since` is what makes syncing
  twenty-three characters affordable, and `Outcome::Unchanged` is a distinct
  variant because a 304 is neither an answer nor a failure.
- **`Outcome::Empty` is not `Outcome::Stale`.** A character with no mounts and a
  parser that has stopped understanding the response both produce an empty list.
  Collapsing them makes a broken parser silently empty a collection and a run
  look finished.
- **Wowhead is off limits.** Its terms forbid automated access and its
  robots.txt names `anthropic-ai` and `ClaudeBot`. Linking to it is fine and is
  what "point me at a guide" means. Fetching it is not.

- **Rarity is read, never shipped, and never fetched.** `model/rarity.rs` scans
  an *installed* `Interface/AddOns/Rarity/DB` for drop chances. It cannot be
  vendored: Rarity is GPL-2.0 with no "or later" grant and Armory is
  GPL-3.0-or-later, which are incompatible. Reading a file on the machine
  already running both is a different act, and it is the same one the collector
  addon already involves. No Rarity, no odds, and the cards fall back to the
  tries count.

  Three things about the data. **`chance = 100` means one in a hundred**, not a
  hundred per cent. **The figures are estimates read off Wowhead by Rarity's
  authors**, overridable in its options — the tooltip says so rather than
  letting them read as measurements. And **the parser is not a Lua
  interpreter**: these are hand-written files with `LibStub` calls, `CONSTANTS`
  references and an early `return {}`, so it scans for a shape it knows, takes
  four scalar fields, and drops any entry it cannot read whole. `chance = 100,
  -- Blind guess` is a real line and eighty-odd entries carry a trailing
  comment; a reader that stops at the comma loses every one of them silently.

  The join is on `link_id`, which is a different id space per kind and is
  exactly what Rarity keys by: a mount's summoning spell, a pet's creature, a
  toy's item. A toy whose `link_id` is still a guessed stand-in is refused, for
  the same reason its Wowhead link is.

- **The chronicle records what NPCs say and never what players say.** Only
  `CHAT_MSG_MONSTER_*` and `CHAT_MSG_RAID_BOSS_*` are registered. Those are
  Blizzard's own writing, read by the player, available from no endpoint — the
  quest-text argument applied to everything between the quests. Player chat is
  somebody's own words and does not belong in a file another program reads,
  which is the same rule the mailbox scan already follows. `MAX_SAID` is a
  budget of its own rather than a share of `MAX_EVENTS`, because ambient
  chatter arrives faster than anything else and would otherwise crowd out the
  events an evening is actually about.

- **Three kinds of dialogue, and they are not worth the same.** `Said` is what
  the world said unbidden and is the evening's atmosphere. `Told` is what an
  NPC said when the character walked up and asked, and much of it is a
  shopkeeper's greeting. They are separate variants all the way to the prompt,
  because "much of this is functional, use only what carries the evening" is
  true of gossip and false of a boss mid-fight. Gossip carries its own budget
  for the same reason — a shared one would let vendor patter crowd out the
  dialogue an evening is about.

- **Gossip is captured unfiltered beyond deduplication, deliberately.** A
  length threshold would drop a short line that mattered and keep a long one
  that did not. The reader best placed to judge is the one writing the entry,
  so the addon captures and the prompt decides.

- **A lot of modern quest dialogue is not a chat event.** Since Legion much of
  it goes through the talking-head bar, which raises `TALKINGHEAD_REQUESTED` and
  nothing else — an addon reading only `CHAT_MSG_MONSTER_*` silently misses the
  most deliberately written lines in the game, because they are the ones
  Blizzard paid to have voiced. `CHAT_MSG_MONSTER_PARTY` is the second miss:
  escort followers and bodyguards speak on their own channel.

- **A cutscene can be identified only if it was pre-rendered.** `PLAY_MOVIE`
  carries a `MovieID`, which names the cinematic exactly. `CINEMATIC_START`
  carries no identifier of any kind, so an in-engine cutscene is recorded as
  having happened and the quest turned in a moment later is what names it.
  `IsInCinematicScene` is what keeps every flight-path camera pan out of the
  journal. **Subtitles are not readable** — they are in the client's own data
  files and no API exposes them; the dialogue usually arrives through the
  ordinary monster chat events instead, which is as close to a transcript as
  this gets.

- **An expired auction is the only evidence anything did not sell.** Blizzard
  records a failure no more than it records a sale. It arrives as auction-house
  mail with *no money attached*, which is exactly what tells it from a sale, and
  it is the feedback loop on `worth_making` — forty listed and twenty-eight
  back is the answer to whether a flip was real.

- **An in-game auction scan is not wanted.** The API's hourly dump is the same
  data, complete, with no in-game action; `ReplicateItems` is throttled to
  roughly a quarter-hour, needs somebody standing at an auctioneer, and buys a
  very large in-memory database. What the API genuinely cannot see is the
  account's *own* auctions, and mail already carries the half that matters.

- **The chronicle fetches no lore, and the reason is not robots.txt.** It is
  that the game is a better source. `GetQuestText` and `GetRewardText` are the
  sentences the player just read, and `C_CampaignInfo.GetCampaignInfo` hands
  over Blizzard's own paragraph about the storyline they belong to. Both cost
  nothing and neither can be improved on by a wiki summary. Anybody reaching
  for a scrape or a retrieval step to make entries "know the lore" is solving a
  problem the screen already solved.

- **The addon cannot take a screenshot, and no longer tries.** `Screenshot()`
  needs a hardware event behind it; called from an event handler or a timer
  there is none, and the client answers with the "blocked from an action only
  available to the Blizzard UI" dialog — so every notable moment put a popup on
  screen instead of a picture in the journal. The moment is still noted, and
  `Digest::pictures` still matches it against the Screenshots folder's
  modification times within `SHUTTER`; the picture is now one the *player*
  took. A filename was never knowable either way, which is why the correlation
  was ever by time.

- **The addon says what the client refused it.** The blocked-action dialog
  names the addon and not the function, so diagnosing one otherwise means
  reading every call and guessing. `ADDON_ACTION_BLOCKED` is recorded into
  `ArmoryCollectorDB.blocked` as a function name and a count — a fault report,
  which is why it is in the account file rather than in an evening.

- **The quest text is only readable while the quest frame is open.** So the
  turn-in text is captured at `QUEST_COMPLETE` and attached at
  `QUEST_TURNED_IN` — the frame is gone by the time the id arrives, and the id
  is what everything else keys on.

- **The journal writes through a local `llama-server`, not a hosted API.**
  llama.cpp's OpenAI-compatible `/v1/chat/completions`, the same server Familiar
  drives, `http://127.0.0.1:8080` unless `Settings::journal_server` says
  otherwise. No key, nothing in the keyring, and nothing about an evening leaves
  the machine. `/props` is asked once for the model's name, because
  llama-server serves whatever it was launched with and ignores the request's
  `model` field entirely — that call is also the readiness check.

- **`journal_automatic` is on by default, and that is a change.** It was off
  while entries cost money at a hosted API. They do not now, so the only reason
  left to hold back was gone. "Write Every Entry" still runs strictly one at a
  time and abandons the queue on the first failure — a server that is not
  running fails identically thirty times.

- **A local model may put `<think>` in the visible content.** llama-server puts
  reasoning in `reasoning_content` only when it was launched with a template
  that knows how; plenty of GGUFs emit the tags inline instead, and the JSON
  the grammar produced is then unparseable. `strip_thinking` cuts to the
  closing tag. Do not remove it because "the schema guarantees the shape" — the
  grammar constrains the sampler, not what a template prepends.

- **`session` and `entry` are not purged, and that is not an oversight.**
  `Store::purge` sweeps `response` and `price` because the 30-day term is a
  condition on data obtained through Blizzard's API. The chronicle came off the
  addon — the user's own client recording the user's own play — and a journal
  you are not allowed to keep is not a journal. Adding these tables to the sweep
  would silently delete the one thing in this application somebody might still
  want in ten years.

- **WoW's serializer writes a table with an interior `nil` as keyed entries, not
  as a padded array.** So `{ at, "quest", 123, nil, "text" }` comes back in a
  different shape from `{ at, "quest", 123, "title", "text" }`. Every chronicle
  row is written with `""` for absent fields to keep it dense, and the reader
  turns `""` back into `None`. Do not "tidy" the empty strings out of
  `Chronicle.lua`.

- **The money ledger is the only set of books, and the itemised moments are not
  a second one.** `Paid` and `Sold` say *what* — "Mycobloom sold for 374g" —
  and `Coin` says how much moved and why. `Digest::quest_income` and
  `sale_income` read the ledger. Deriving either from the moments instead
  double-counts every quest reward and every auction sale, which is exactly what
  the `questPaid` flag in `Chronicle.lua` exists to prevent.

- **A gold delta with no frame open is not "unknown".** It is loot if it went up
  and unknown if it went down, because coin off the ground has no event and
  every way of *spending* money has a frame. Collapsing the two would file every
  copper picked up in a cave under "we do not know".

- **A lifetime counter is written twice and merged by `MAX`.** The session copy
  is one evening; the copy in `ArmoryCollectorDB.tally` is the lifetime, and
  `store::save_collected` takes the larger count on conflict. A reinstalled
  addon starts at one, and nothing in the game or the API can give any of these
  back — Blizzard's statistics count some professions in bulk and no particular
  recipe, count no party members at all, and forget a boss attempt the moment
  the pull ends.

- **One `tally` table, not one per counter.** `model/tally.rs` is a
  `(kind, key)` and a number, deliberately generalised at the *second* counter
  rather than the fifth. A kind the reader does not know is skipped rather than
  filed under a plausible one, in both the addon reader and the store's.

- **`travelled` is accumulated separately from `walked`/`flown`.** The addon
  flushes distance to the account file every few minutes and zeroes the two it
  flushed; reading those at logout reports the last four minutes of a four-hour
  evening. There is a third accumulator that nothing but the next login resets,
  and it is the one the session total comes from.

- **The lifetime counters are drawn shut, in one group.** Eight open lists is
  most of a screen and this is a journal — pushing tonight's evening below how
  many flasks somebody has made is the wrong page. Each row carries its
  headline in the subtitle so the shut state still answers.

- **Gear provenance is a join over the evening, bounded to `SPOILS`.** Nothing
  in the game connects an item to what dropped it: the loot event names an item
  and the encounter event names a boss, minutes apart in several hundred rows.
  `where_it_came_from` requires the item to have been *looted* and something
  named to have died within thirty seconds. A wider window puts the previous
  pull's boss on somebody's belt.

- **Gold from an alt is not income.** `MAIL_INBOX_UPDATE` is the only place a
  sender is readable, and it is gone by the time `PLAYER_MONEY` fires — so the
  addon classifies during the scan and looks the classification up *by amount*
  when the money lands. `Purpose::Sale` is what `sale_income` counts;
  `Transfer` is the account's own money moving and counting it would let
  somebody earn the same gold on every character they own. `Purpose::Mail` is
  the honest "we could not tell".

- **The combat log is closed to addons as of patch 12.0, and this is why three
  numbers are always empty.** `COMBAT_LOG_EVENT` and
  `COMBAT_LOG_EVENT_UNFILTERED` refuse registration — Blizzard's "addon
  apocalypse", aimed squarely at addons making decisions from combat
  information. So `kills`, `worstHit` and `lowestHealth` sit at their starting
  values on 12.0.7 while every other handler records normally, and the pages
  that draw them are already guarded on `> 0` so they simply do not appear.
  There is no replacement API and Advanced Combat Logging does not help: it
  changes the payload of an event that never arrives. The handler is kept for
  clients that still allow it, and the registration loop now asks
  `IsEventRegistered` rather than trusting that no error meant success — a
  refused registration throws nothing, which is exactly how a dead handler sat
  there looking wired up.

- **The CLEU damage amount is read by position from the front, never the back.**
  Eleven arguments are common to every subevent; a swing puts the amount
  twelfth, a spell fifteenth, the environment thirteenth. Counting from the end
  is shorter and wrong — a trailing nil shortens the list.

- **Professions are merged, not assigned.** The API has the expansion tier and
  the addon has the specialisation trees and the knowledge spent, and neither
  knows the other's half. `parse_professions` landing on `detail.professions`
  as a straight write takes the trees off every character one sync after a
  logout — the same rule as `save_collectibles`.

- **Brann's level needs no code.** The delve companion's level is a Warband
  reputation, so it already arrives through the reputation path. The delve
  *tier* does not: `C_DelvesUI.GetActiveDelveTier` answers only while a delve
  is active, which is why it is read at `SCENARIO_COMPLETED` and not at logout.

- **A recipe with one unpriced reagent is unmeasured, not cheap.** The same
  rule as `Evaluation::observable`, in the market: costing a craft against the
  reagents that happen to be listed and ignoring the one that is not makes the
  dearest recipes look like the best ones. `Crafting::unmeasured` counts them
  and the page says so, rather than quietly showing a subset as the whole.

- **`worth_making` is ranked by margin × what sold, never by margin.** A
  four-hundred-gold profit on a thing nobody buys is forty unsold flasks, and a
  calculator that cannot tell that from a real flip is worse than no page. The
  liquidity comes from `record_prices`'s quantity deltas, which is the one thing
  Armory has that a margin calculator does not.

- **Every crafting figure is quality one, and that is a floor.** What quality a
  craft lands at depends on skill, specialisation and inspiration, and Armory
  reads none of them. Quoting the three-star price would be inventing a number
  about somebody's own character — the same mistake `Resale::floor` exists to
  avoid. Reading crafting stats properly means a per-recipe pass in the addon;
  grow it from confirmed shapes.

- **Warband stock is shown and never subtracted.** The bag indices have never
  been confirmed against a stocked bank, so `Making::held` is a line of its own.
  Taking it off the cost would turn an unverified read into a silently inflated
  margin; as its own line a wrong index is visibly wrong.

- **`snapshot` and `price` are two different questions and only one is
  expensive.** `snapshot` is *now*: every item on a realm, replaced whole every
  hour, and it costs nothing because the response it comes from was already
  being downloaded in full and thrown away. `price` is *history*: opt-in,
  thirty-day, and the reason the watch list exists at all. Browsing reads the
  first; watching an item is what starts the second. Merging them would make
  either browsing impossible or history unaffordable.

- **`record_snapshot` replaces a realm's rows, never merges them.** An item that
  has left the auction house entirely has to disappear from the browser, and a
  merge leaves last hour's price sitting there looking current.

- **The auction house never says what an item is called.** A listing is an id.
  `item_search` goes the other way — a name somebody typed to an id — and there
  is no bulk endpoint, so names arrive one call at a time and the browser shows
  an id until one does. `MarketPage::wants_names` reads the *filtered, ordered*
  rows for the same reason `art_wanted` reads the grid's model: a hundred and
  fifty names a sync against a market of tens of thousands, spent on whatever
  the database returned first, leaves the top of the page blank for weeks.

- **The browser puts `BROWSE_SHOWN` rows in the model, not the market.**
  `GtkColumnView` recycles widgets and would hold the lot, but the model is
  rebuilt on every keystroke and thirty thousand `GObject`s per character is
  what stops a search keeping up.

- **Ordering lives in `market::browse`, not in column sorters.** A
  `GtkColumnViewColumn` sorter would be a second implementation of a comparison
  that is already written and tested, and two orderings of one list is how they
  come to disagree.

- **A price row is the shape of the book, not just its floor.** `Depth` keeps
  the cheapest price, the total quantity, *how many listings*, and the tenth and
  median unit price weighted by quantity. The floor alone cannot tell one
  lowball at a hundred gold from four hundred units at a hundred gold, and
  every interesting market question is about the shape. `record_prices` counts
  all six as movement — a floor that holds while forty listings become one has
  changed in the way that matters most.

- **Browsing a realm shows the region-wide commodities too, because that is
  what the auction house is.** Commodities have no realm — Blizzard records them
  region-wide, which is realm 0 here — and gear is listed on its realm alone, so
  the two sets are disjoint and `Store::snapshot` queries `realm IN (?1, 0)`.
  Showing one half answers "nothing" for Copper Ore, Copper Bar and every other
  stackable trade good in the game, which is most of what anybody types. This
  also decides what the name backfill spends its budget on: `wants_names` reads
  the rows on the page, and a page of realm-only listings spends every request
  on gear nobody searches for while the commodity market stays a wall of ids.

- **`Series` carries the clock, and that is a change.** It used to drop
  `seen_at` on the grounds that the series *is* the window. True, and enough for
  "what is this worth" — but it makes every question with a rate in it
  unanswerable. `span_hours` is the span actually observed, never the thirty
  days the store may keep: a realm watched since Tuesday has four days of
  evidence, and dividing by thirty quotes a number nobody measured.

- **Thirty days is a ceiling and cannot be raised for analysis.** It is a term
  of the API licence, not a cache policy. Longer history is not available at any
  scope — the answer to "we need more history" is richer rows inside the window,
  which is what `Depth` is.

- **The recipe book cannot be read at login.** `C_TradeSkillUI.GetAllRecipeIDs`
  answers an empty table until the profession window has been opened, and
  includes unlearnt recipes when it does answer. So the scan hangs off
  `TRADE_SKILL_LIST_UPDATE`, filters on `info.learned`, and treats an empty
  answer as "not open yet" — a character who has opened Alchemy and not
  Herbalism must keep their Herbalism recipes. That is also why the `recipe`
  tables merge rather than replace.

- **A reagent's quality tiers are separate item ids.** `slot.reagents` is the
  list of them, and the auction house proves it: reagents are commodities and a
  commodity carries no bonus ids to vary by. All tiers are recorded so a craft
  can be costed at the cheapest one that has a price, and `commodity_series`
  reads only `variant = ''` rows for the same reason.

- **The price net is bounded by the recipe books, not by the market.**
  `Application::record` keeps watched items, pets, and items some character's
  recipe names. An account whose professions have never been opened records
  nothing extra at all.

- **`ui/http.rs` reads 401, 403 and 404 the way Blizzard means them**, which is
  wrong for anything else — a 403 is a privacy checkbox *there*. `SourceId`
  answers `is_blizzard`, and the Anthropic path reads its own statuses and its
  own error body so that a mistyped key says "invalid x-api-key" rather than
  "your Battle.net sign-in has expired". Adding a third non-Blizzard source
  means deciding which of those readings it wants.
- **`model/addon/lua.rs` is not a Lua interpreter and must not become one.** It
  reads generated table literals out of a directory addon managers also write
  to, and refuses anything that looks like a call.
- **Chain resolution is a bounded fixpoint, not a graph walk.** `plan::plan`
  re-plans up to `CHAIN_PASSES` times, feeding each pass what the previous one
  settled. The bound is what stops a cycle in Blizzard's data spinning forever;
  there is a test for exactly that.
- **`record_prices` writes only what moved, and quantity counts as movement.**
  Blizzard records no sale at all — quantity just disappears between snapshots —
  so the whole inference of "what sold" is those deltas. Do not "optimise" them
  away by comparing price alone.
- **`purge` covers `price` as well as `response`.** Price history is data
  obtained through the API and carries the same 30-day obligation.
- **The addon's criteria trees are one level deep; the API's are nested.** So
  `collected.progress()` only fills `inputs.progress` when the API has not
  already supplied a richer list. A nested tree measures a meta-achievement
  better, and overwriting one with a flat one would silently lose that.
- **The addon's roster is merged, not assigned.** A character the API found but
  which has never been logged in on must survive an addon read; the addon
  knowing nothing about them is not evidence they are gone.
- **`Source::from_text` matches the leading clause, not a substring.** "Vendor:
  sold near the Drop Zone" is a vendor. There is a test.
- **Most of what a raid drops has no market at any price.** Bind-on-Pickup is
  the filter that makes "worth looking for" a short useful list rather than a
  wall of things you cannot sell, and the binding arrives free on the same
  `/data/wow/item/{id}` call the name backfill already makes
  (`preview_item.binding.type`). An item *absent* from the item table is
  unknown rather than sellable and is left out — offering somebody a BoP drop
  as a thing to sell is the failure worth avoiding. An absent binding on an
  item that *is* known means freely tradeable, so silence is a yes there, which
  is the opposite of the usual rule and worth the second look.

- **`AdwPreferencesGroup::add` puts a non-row widget below the boxed list**,
  however early it is added. A history paragraph added before the landmark rows
  still renders underneath them and reads as a footnote. The chronicle card hit
  this and it was written down; the zone page hit it again anyway. Prose first
  means prose in its own container above the group, not first in the group.

- **The zone corpus ships and the Adventure Guide does not.** `data/zones.json`
  and `data/instances.json` are Armory's own writing from CC BY-SA sources, so
  they compile into the binary and cost nothing. The guide's descriptions are
  Blizzard's text arriving through the licensed API, so they live in the
  database like the achievement catalogue — fetched, cached, displayed. When
  both exist the guide wins, and the page says which it is showing.

- **A link that lands on a plausible wrong page is worse than no link.** The
  collection *index* gives a toy or a piece of decor the collection's own id as
  a stand-in for the item they wrap, and both are addressed on Wowhead — and
  for icons — *by item*. Following that stand-in sends somebody to a real,
  unrelated item: clicking a chair opened a belt, and the belt icon was drawn
  on the cell for the same reason. `item_id_is_guessed` is the test,
  `known_item_id` and `wowhead_url` both answer `None` for it, and the dialog
  simply omits the row.

  The comment that used to sit on `link_id` argued the opposite — "a link that
  lands on the wrong page is recoverable, and no link at all is not". That
  holds for a mount, where a wrong id lands on nothing and fails visibly. It is
  false for anything addressed by item, because the wrong page reads as the
  right one until somebody looks at it.

- **A toy lives in two id spaces and nothing joins them but us.** The toy box
  knows an item — `Kang's Bindstone` is 86571 — and the web API knows a *toy*,
  a separate much smaller id space the client never exposes. An account with
  both sources holds most of its toys twice. `store::collapse_toys` folds them
  on the item id where a detail call has supplied one and on the name where it
  has not, and the name half is safe only for toys: Blizzard ships several
  distinct mount ids called `White Stallion`.

- **A toy has no name of its own.** `/data/wow/toy/{id}` puts it on the item it
  wraps. Reading it the way a mount's name is read yields an empty string, and
  a hundred and fifty nameless toys is what that looked like.

- **The art maps are memory, and `restore_art` is what makes them survive.**
  `portraits`, `toy_art` and `achievement_art` are `HashMap`s on the
  application and nothing writes them to a schema of their own — the bodies
  they were parsed out of are already in the response cache under the ordinary
  term, so a launch re-reads those instead. Without that step every launch
  starts blank and spends its whole `ART_PER_SYNC` re-earning URLs it has the
  bodies for, which at two thousand toys and a hundred and twenty a sync never
  converges. A fourth thing that needs a looked-up picture needs a fourth read
  in `restore_art`.

- **`art_wanted` reads the grid's model, not the backing store.** The budget is
  one request per entry and it has to be spent on what somebody is looking at.
  The store's order is `collapse_toys`'s — descending item id, which is the
  newest toys in the game — and the grid sorts by name, so spending it there
  scatters icons through the page with no pattern and leaves the top of it
  blank however many syncs run.

- **`save_collectibles` merges, it does not replace.** The journal has the
  artwork, the sentence and the faction lock; the web API has a name. Whichever
  lands second must not flatten the other, or one index sync after a logout
  takes the pictures off the whole collection.

- **The render service is not an API host.** `render.worldofwarcraft.com` takes
  no token, answers no namespace and spends no quota, which is the only reason
  sixteen hundred mounts can be illustrated. A creature display id is a URL, not
  a request. Its 403 means "no such art", and `ui/http.rs` reads a 403 as the
  privacy refusal it is on the API — true there, wrong here, and harmless only
  because `Images` treats every failure the same way.

- **A caged pet is item 82800, every single one of them.** The species is a
  field beside it, which is why `auctions::Listing` keeps `pet_species` and why
  `market::on_sale` joins pets on the species and never on the item. Joining on
  the item would report every missing pet in the game as available the moment
  anybody listed any pet at all.

- **A price series is keyed by `Listing::series`, not by item id.** The same
  fact one step further on: `cheapest` used to key on `(item_id, variant)` with
  `variant` covering only bonus ids and modifiers, so every caged pet on a
  realm — no bonuses, no modifiers — collapsed into one row. What got recorded
  was the price of the cheapest pet on the whole realm against the summed
  quantity of every pet listing on it, which is a price for nothing. `series`
  puts the species and quality in the `variant` column, which is exactly the
  question that column answers and needs no migration. Level is deliberately
  left out: it would multiply the series twenty-five-fold, and since a history
  keeps the cheapest listing, folding it in makes the figure a floor rather
  than a guess.

- **Armory knows the quality of every pet listed and of none that you own.**
  The auction house says `pet_quality_id`; the pet journal reports quality per
  pet rather than per species and the collector does not read it. So
  `worth_selling` quotes the *cheapest* quality's price and shows the spread.
  Quoting the rare price at somebody whose spare is a common would be inventing
  a number about their own collection. Reading it properly means a per-pet pass
  in the addon — grow that from confirmed API shapes, not from inference.

- **A mount cannot be joined to the auction house.** Its record names the spell
  that summons it; the item that teaches it is a different number and appears
  nowhere in the profile API. So the auction join answers pets, toys and decor,
  and saying "no mounts are for sale" would be a lie about our data rather than
  about the market.

- **Decor's response shapes are the one thing here parsed from documentation
  rather than from traffic.** The endpoints were announced in December 2025 and
  no token was available to record a fixture against. `parse_collected` reads
  both the nested and the flat form for that reason, and `Outcome::Stale` is
  what a third shape would produce — visibly, rather than as an empty
  collection.

- **`run::Standing` is the poisoning distinction and keeps that name.** The
  collections page's own "which entries am I looking at" enum is
  `collection_page::Showing`, deliberately not a second `Standing`.

## Serena is the primary toolset for Rust and Lua code

This project runs the **Serena MCP server** under the `claude-code` context. Serena's symbol-aware
tools are the primary tools for anything in a `.rs` or `.lua` file — `src/` and
`addon/Armory_Collector/` are both indexed; `Read` and `Edit` are the fallback. Where a built-in
tool description tells you to prefer `Read`/`Edit`, that description is written for projects without
Serena and is superseded here.

| Task | Tool |
|------|------|
| See a file's structure | `get_symbols_overview` |
| Read one symbol's body | `find_symbol` with `include_body=true` |
| Find a symbol, or its callers | `find_symbol` / `find_referencing_symbols` |
| Find declarations, impls of a trait | `find_declaration` / `find_implementations` |
| Check errors without a build | `get_diagnostics_for_file` |
| Replace a fn, impl block, or struct | `replace_symbol_body` |
| Add an item, or an import at the top | `insert_after_symbol` / `insert_before_symbol` |
| Change a few lines inside a fn | `replace_content` |
| Make the same change across files | `replace_in_files` (`dry_run` first) |
| Rename or remove a symbol | `rename_symbol` / `safe_delete_symbol` |

Serena's `read_file`, `list_dir`, `find_file`, `search_for_pattern` and `execute_shell_command` are
switched off in this context — `Read`, `Glob`, `Grep` and `Bash` cover those. Use `Grep` and `Glob`
freely for **discovery**, then follow every hit through Serena rather than reading the file around it.

Reach for `Read`/`Edit` on a `.rs` or `.lua` file only when: Serena was tried on that target and
failed; the file will not parse; or you need a handful of lines whose enclosing symbol is very
large. `Read`, `Write` and `Edit` are the right tools for non-code files — Markdown, TOML, the JSON
under `data/`, shell scripts. A brand-new file is `Write`; there are no symbols to navigate yet.

Before editing code: `get_symbols_overview` on the target → `find_symbol` with `include_body=true`
for only the symbols you will touch → edit through the symbolic tools. When you already know the
symbol's name, call `find_symbol` first — no `Grep` or `Read` warm-up.

None of the following is a reason to fall back to `Read`/`Edit`, and catching yourself forming one
is the signal to use Serena instead: "I already know the path", "one `Read` is cheaper than three
Serena calls", "the file is short", "I need to see it in context first".

`data/**` is outside the index — 8M of zone and instance JSON compiled into the binary. Serena will
not find a symbol there and is not meant to; those files are `Read` and `Grep`.

Subagents are bound by this too, and you only ever see their diff — so put it in the dispatch
whenever you delegate an edit to an existing `.rs` or `.lua` file.

## Conventions

- Use the `developing-gtk-apps` and `designing-gnome-ui` skills for widget,
  threading, and HIG decisions rather than deriving them again.
- Edit Rust and Lua through Serena's symbolic tools; the Edit tool is the
  fallback and non-code default. Never rewrite sources through
  `python3 - <<PY` heredocs or `sed -i`.
- The sibling apps (brain, familiar, magpie, planner, scribe, sleeve, stickies)
  share this layout and these scripts; a pattern established in one is the
  pattern here.
