# Zone lore: the checklist

A zone page shows what Armory already knows about a place — the evenings spent
there, the hours, the quests turned in with their text, what killed you, the
rares, the screenshots — and one thing it does not: **what the place is**.

That last part is gathered here, one zone at a time, and checked off below.

## What a zone entry holds

One JSON file per zone under `data/zones/`, named by the game's own zone name
slugified. Nothing is fetched at runtime: this is a build-time corpus that ships
with the application, so a zone page costs no request and works with no network
at all — the same reasoning that keeps the chronicle from fetching lore.

```json
{
  "zone": "Nagrand",
  "map": 107,
  "expansion": "The Burning Crusade",
  "summary": "Two or three sentences. What the place is, who lives there, and why it matters.",
  "history": "A paragraph, 80-140 words. What happened here, in the order it happened.",
  "factions": ["Mag'har", "Kurenai", "The Consortium", "Warmaul ogres"],
  "notable": [
    { "name": "Oshu'gun", "what": "The white mountain the orcs held sacred" }
  ],
  "sources": [
    { "title": "Nagrand", "url": "https://warcraft.wiki.gg/wiki/Nagrand" },
    { "title": "Oshu'gun", "url": "https://warcraft.wiki.gg/wiki/Oshu%27gun" }
  ],
  "licence": "CC BY-SA 4.0"
}
```

### `map` is the key, not the name

`map` is Blizzard's `UiMapID`, and it is the field everything joins on. **Zone
names are not unique**: there are two Nagrands and two Shadowmoon Valleys, on
different continents in different expansions, and a session recorded in one
would otherwise show the other's lore. The addon records the id alongside the
name for exactly this reason (`whereAmI` in `Chronicle.lua`), so the file name is
convenience and the `map` field is the join.

`scripts/pull.py maps` reads them from the wiki's own `UiMapID` page, which is
a table of id, name, type and **parent map**. The parent is not optional: the
table lists `Nagrand` twice — 107 under Outland and 550 under Draenor — and a
reader that takes the first gives the alternate-universe zone the wrong
continent's history, which is the exact failure the id exists to prevent.

**That page is stale, and the gap is self-healing.** It is current to PTR patch
10.1.7 (July 2023), so nothing from The War Within or Midnight is in it —
Hallowfall, Isle of Dorn, the Ringing Deeps, K'aresh, Harandar, Quel'Thalas and
the Emerald Dream all come back without an id. They are also the zones somebody
is actually playing right now, and the addon records
`C_Map.GetBestMapForUnit` for every zone it sees, so those ids arrive from play
rather than from a wiki. Four more (`Oribos`, `Isle of Quel'Danas`, `Zul'Aman`,
`The Wandering Isle`) are typed as dungeons or simply absent, and are the same
fix.

If an id cannot be established, leave it null rather than guessing — a wrong id
silently attaches the wrong history to a real evening.

### `sources` is attribution, and covers every page used

One list, not a `source` string beside a `reading` list. CC BY-SA 4.0 requires
crediting everything an entry drew on, and an entry that took its history
paragraph from three pages must name three. The zone page renders the whole list
as the credit *and* as the further reading, because they are the same thing.

### Rules for gathering

- **`warcraft.wiki.gg` is the source.** Not Wowhead — its terms forbid automated
  access and its robots.txt names `ClaudeBot` by name. Linking to Wowhead is
  fine and is what "point me at a guide" means; fetching it is not.
- **CC BY-SA 4.0 means attribution and share-alike.** Every entry carries the
  pages it came from, and the zone page displays them. This is not optional and
  it is why `sources` and `licence` are fields rather than comments.
- **Summarise, do not copy.** A paragraph in Armory's own words with a link to
  the original is both better reading and a cleaner licence position than a
  block of pasted wiki text.
- **Polite volume.** One pass, cached to a file, never repeated at runtime.
- **Leave out what the game already says better.** A zone's quests and campaign
  text are captured by the addon at the moment the player reads them, and are
  first-party. This corpus is for the *place*, not the storylines in it.

### Settled, so nobody has to decide twice

These came out of the pilot and are not open questions.

- **`summary` describes the zone as it is now.** It is what the player is
  standing in. `history` *may* cover a revamp — for a Cataclysm-changed zone the
  cataclysm is usually the most interesting thing that ever happened there.
- **Novel and other-media lore is allowed**, and is often the only way a history
  paragraph says anything specific. Most of Nagrand's past is from *Rise of the
  Horde* rather than from anything in game.
- **`factions` means "who is here"**, hostile included. Warmaul ogres belong in
  it; it is not a list of reputations to grind.
- **One file per checklist entry. Sub-zones become `notable` rows.** Halaa and
  the Throne of the Elements have substantial pages of their own and are still
  one line each inside Nagrand, with their pages in `sources`.
- **Which of a duplicated zone is named in the prompt**, not chosen by the
  gatherer. The wiki's hatnote is a reliable tell but the expansion should be
  given.
- **A name is verified against the whole corpus, not against the entries.**
  `data/zones/_raw/*.json` holds the full wikitext of 144 zones and
  `_raw/_linked.json` holds 1,330 more pages — about four million characters.
  An earlier pass dropped `Algalon` because it appeared in no *entry*, when it
  is sitting in Uldum's raw wikitext. Search the raw corpus before giving a
  name up, and only drop one that is genuinely absent from all of it.
- **Two names in the corpus come from the transcripts and are unverified.**
  `Cyrukh the Firelord` in `shadowmoon-valley.json` and `Whiteclaw` in
  `frostfire-ridge.json` appear nowhere in the raw corpus — not in the zone
  wikitext, not in `_linked.json` — so they stand on the transcript alone.
  Anybody with the hardcopy Chronicle can settle both in a minute.

- **These names were hunted through the whole raw corpus and are not in it.**
  Recording them so nobody spends the search again: the Halls of Stone boss the
  transcript calls "Xionia"; the Gurubashi warlord Medivh killed and the son
  who took the empire to war over it ("Jaaknon"/"Jagnan" and "Zanon"); the mogu
  warlord "Shao Jin the blood-letter"; the pandaren commander "Ban
  Bearheart"/"San Behrhardt"; the worm "Norvakesh". The sporemound *was* found
  — Botaan, in `netherstorm.json` and in `_linked.json`'s `Primals` and
  `Breakers` — and `gorgrond.json` now names it, along with the correction that
  the colossals killed it after Grond fell, not Grond himself.

- **The audiobook transcripts are the books' own prose, and that changes the
  rule.** The abridgement videos were somebody else's paraphrase — already a
  step removed from Blizzard's text. The audiobooks are not: a transcript
  sentence is a book sentence with transcription errors in it. So the
  paraphrase bar is *higher* here, not lower, and `check_zones.py` now compares
  every `summary` and `history` against all 232,000 shingles of them. A match
  is a passage of a copyrighted book sitting in a data file, and the brief
  check would never have seen it.

- **The Chronicle transcripts mangle every proper noun.** They are automatic
  transcripts of spoken video: Ulduar comes out as "Uldurar", Y'Shaarj as
  "Yasharj", Azshara as "Ashara", saronite as "serenite". Use them for facts and
  normalise every name against the wiki before writing it down — and grep
  loosely, because searching for the correct spelling will miss the passage.
  They also carry no open licence, so facts only, never phrasing, and the
  citation goes to the book rather than to the video.

- **Wiki navboxes come back empty.** `Travel hubs`, `Major factions` and
  `Maps and subregions` are template-driven and render as bare headings through
  a fetch. Reconstruct factions from the prose; do not conclude a zone has none
  because the section looked blank.

---

## The checklist

Tick a zone when `data/zones/<slug>.json` exists and validates.

**Every entry here is a real `warcraft.wiki.gg` page title**, so `scripts/pull.py`
can use it directly. Where a title needs disambiguating the wiki's own form is
used, and it is not always the obvious one: the Outland zone keeps the bare
title and the Warlords one is `Nagrand (alternate universe)`. Where the
checklist would otherwise carry an editorial label the zones are listed
separately instead.

### Classic — Eastern Kingdoms

- [x] Elwynn Forest
- [x] Westfall
- [x] Redridge Mountains
- [x] Duskwood
- [x] Northern Stranglethorn
- [x] The Cape of Stranglethorn
- [x] Swamp of Sorrows
- [x] Blasted Lands
- [x] Burning Steppes
- [x] Searing Gorge
- [x] Badlands
- [x] Loch Modan
- [x] Dun Morogh
- [x] Wetlands
- [x] Arathi Highlands
- [x] Hillsbrad Foothills
- [x] The Hinterlands
- [x] Western Plaguelands
- [x] Eastern Plaguelands
- [x] Tirisfal Glades
- [x] Silverpine Forest
- [x] Deadwind Pass
- [x] Isle of Quel'Danas
- [x] Ghostlands
- [x] Eversong Woods

### Classic — Kalimdor

- [x] Durotar
- [x] Northern Barrens
- [x] Southern Barrens
- [x] Mulgore
- [x] Stonetalon Mountains
- [x] Ashenvale
- [x] Darkshore
- [x] Teldrassil
- [x] Desolace
- [x] Feralas
- [x] Dustwallow Marsh
- [x] Thousand Needles
- [x] Tanaris
- [x] Un'Goro Crater
- [x] Silithus
- [x] Winterspring
- [x] Felwood
- [x] Moonglade
- [x] Azuremyst Isle
- [x] Bloodmyst Isle

### Cities

- [x] Stormwind City
- [x] Ironforge
- [x] Darnassus
- [x] The Exodar
- [x] Orgrimmar
- [x] Thunder Bluff
- [x] Undercity
- [x] Silvermoon City
- [x] Shattrath City
- [x] Dalaran
- [x] Valdrakken
- [x] Dornogal

### The Burning Crusade — Outland

- [x] Hellfire Peninsula — `data/zones/hellfire-peninsula.json`
- [x] Zangarmarsh — `data/zones/zangarmarsh.json`
- [x] Terokkar Forest — `data/zones/terokkar-forest.json`
- [x] Nagrand — `data/zones/nagrand.json`
- [x] Blade's Edge Mountains — `data/zones/blades-edge-mountains.json`
- [x] Netherstorm — `data/zones/netherstorm.json`
- [x] Shadowmoon Valley — `data/zones/shadowmoon-valley.json`

### Wrath of the Lich King — Northrend

- [x] Borean Tundra — `data/zones/borean-tundra.json`
- [x] Howling Fjord — `data/zones/howling-fjord.json`
- [x] Dragonblight — `data/zones/dragonblight.json`
- [x] Grizzly Hills — `data/zones/grizzly-hills.json`
- [x] Zul'Drak — `data/zones/zuldrak.json`
- [x] Sholazar Basin — `data/zones/sholazar-basin.json`
- [x] The Storm Peaks — `data/zones/the-storm-peaks.json`
- [x] Icecrown — `data/zones/icecrown.json`
- [x] Crystalsong Forest — `data/zones/crystalsong-forest.json`
- [x] Hrothgar's Landing — `data/zones/hrothgars-landing.json`

### Cataclysm

- [x] Mount Hyjal — `data/zones/mount-hyjal.json`
- [x] Vashj'ir — `data/zones/vashjir.json`
- [x] Deepholm — `data/zones/deepholm.json`
- [x] Uldum — `data/zones/uldum.json`
- [x] Twilight Highlands — `data/zones/twilight-highlands.json`
- [x] Gilneas — `data/zones/gilneas.json`
- [x] Kezan — `data/zones/kezan.json`
- [x] The Lost Isles — `data/zones/the-lost-isles.json`
- [x] Tol Barad — `data/zones/tol-barad.json`

### Mists of Pandaria

- [x] The Jade Forest
- [x] Valley of the Four Winds
- [x] Krasarang Wilds
- [x] Kun-Lai Summit
- [x] Townlong Steppes
- [x] Dread Wastes
- [x] Vale of Eternal Blossoms
- [x] Isle of Thunder
- [x] Timeless Isle
- [x] The Wandering Isle

### Warlords of Draenor

- [x] Frostfire Ridge
- [x] Shadowmoon Valley (alternate universe)
- [x] Gorgrond
- [x] Talador
- [x] Spires of Arak
- [x] Nagrand (alternate universe)
- [x] Tanaan Jungle
- [x] Ashran

### Legion — Broken Isles

- [x] Azsuna
- [x] Val'sharah
- [x] Highmountain
- [x] Stormheim
- [x] Suramar
- [x] Broken Shore
- [x] Krokuun
- [x] Eredath
- [x] Antoran Wastes

### Battle for Azeroth

- [x] Tiragarde Sound
- [x] Drustvar
- [x] Stormsong Valley
- [x] Zuldazar
- [x] Nazmir
- [x] Vol'dun
- [x] Nazjatar
- [x] Mechagon

### Shadowlands

- [x] Bastion
- [x] Maldraxxus
- [x] Ardenweald
- [x] Revendreth
- [x] The Maw
- [x] Korthia
- [x] Zereth Mortis
- [x] Oribos

### Dragonflight — Dragon Isles

- [x] The Waking Shores
- [x] Ohn'ahran Plains
- [x] The Azure Span
- [x] Thaldraszus
- [x] The Forbidden Reach
- [x] Zaralek Cavern
- [x] Emerald Dream

### The War Within — Khaz Algar

- [x] Isle of Dorn
- [x] The Ringing Deeps
- [x] Hallowfall
- [x] Azj-Kahet
- [x] Siren Isle
- [x] Undermine
- [x] K'aresh

### Midnight

Blizzard's own list is four zones. Eversong Woods is a *revamp* — it absorbs
Ghostlands and rebuilds Silvermoon — so it appears in Classic above as well;
one entry covers it and describes the place as it is now, which is the Midnight
version. Quel'Thalas is the kingdom rather than a zone and is kept only as
background for the two that sit inside it.

- [x] Eversong Woods — *also listed under Classic; write it once, as Midnight*
- [x] Zul'Aman
- [x] Harandar
- [x] Voidstorm
- [ ] Quel'Thalas — the kingdom, not a zone; background only

---

## Cost

Two to three fetches per zone, and batching several URLs into one call counts as
one. The main zone page alone carries the summary, the history and most of the
notables; the second call buys the landmark detail that makes a history
paragraph specific rather than generic. Roughly 150-250 fetches for the whole
list.

## How the writing pass runs

Three scripts and a fan-out, in this order. Nothing after the first step needs
the network.

1. `./scripts/pull.py maps` — the `UiMapID` table, once.
2. `./scripts/pull.py zones` — every checklist entry, ~2.5 minutes.
3. `./scripts/pull.py links` — the dozen pages each zone is *about*, batched
   twenty titles to a request.
4. `./scripts/brief.py` — turns the raw pages into one readable brief per zone
   under `_raw/_briefs/`. Markup gone, sections trimmed, landmarks inlined at a
   sentence each. This is what makes the writing pass cheap: an agent reads one
   file of about twenty-four thousand characters instead of forty thousand of
   wikitext and a shared prose blob.
5. **The writing pass**, an expansion at a time, five or six zones to an agent.
   No network at all — every agent reads briefs off disk and writes entries.
6. `./scripts/check_zones.py` after each batch.

### What the checker catches

The four things that actually go wrong, and the fourth is the one that matters:

- a missing field, or JSON that does not parse
- a `history` that has drifted into an essay
- a `map` that was guessed at rather than copied, or invented where the brief
  said there was none
- **prose lifted out of the brief.** A shared run of ninety characters between
  an entry and its source is not a coincidence — names and stock phrases do not
  reach that length. This is the licence position enforced mechanically rather
  than trusted to instructions.

## Order of work

Not alphabetical, and not chronological. **Do the zones this account has
actually been to first** — the chronicle knows which, and a zone page for
somewhere nobody has played is a page nobody opens. `model::tally`'s `Zone`
counter is the list, longest first.

Everything else is backfill, and can happen a expansion at a time whenever.