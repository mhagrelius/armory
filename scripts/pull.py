#!/usr/bin/env python3
"""Pull reference data Armory needs at build time.

Two sources, both of which permit this and neither of which is Wowhead:

* **warcraft.wiki.gg** through its MediaWiki API. wiki.gg documents automation
  as supported and the API returns structured text rather than rendered HTML,
  so this is both the sanctioned route and the better one. Content is
  CC BY-SA 4.0: attribute it, and write your own prose rather than pasting.
* **Blizzard's game data API**, which Armory is already licensed to use and
  already talks to. Item names come from here.

Wowhead is deliberately absent. Its terms forbid automated access, which is a
different thing from its robots.txt and applies to a script you run yourself
just as much as to a crawler. `CLAUDE.md` says the same. Link to it, do not
fetch it.

Usage
-----

    ./scripts/pull.py maps                 # every UiMapID, one page, once
    ./scripts/pull.py zones                # every unticked zone in docs/ZONES.md
    ./scripts/pull.py zones Nagrand Durotar
    ./scripts/pull.py links                # follow each zone's most specific links
    ./scripts/pull.py instances            # the raids Blizzard's guide left blank
    ./scripts/pull.py drops                # drop-rate estimates from the Rarity addon
    ./scripts/pull.py items                # needs BNET_ID and BNET_SECRET

Everything is cached under `data/`, nothing is re-fetched if it is already
there, and requests are spaced out. Run it again after a failure and it picks up
where it stopped.
"""

from __future__ import annotations

import collections
import glob
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path
from urllib.parse import quote, urlencode

ROOT = Path(__file__).resolve().parent.parent
ZONES = ROOT / "docs" / "ZONES.md"
RAW = ROOT / "data" / "zones" / "_raw"
ITEMS = ROOT / "data" / "items.json"

WIKI = "https://warcraft.wiki.gg/api.php"

# Identify the client and say who to complain to. A script that pulls a few
# hundred pages politely and leaves a contact address is a very different thing
# from one that hides.
AGENT = os.environ.get("ARMORY_AGENT", "Armory-build/0.1 (+matthew@hagreli.us)")

# Seconds between requests. The wiki is small and community-run; this is one
# page every second and a half, which no one will notice.
GAP = 1.5


def fetch(url: str, headers: dict[str, str] | None = None) -> bytes:
    """One GET, through curl, with retries on the failures worth retrying."""
    command = [
        "curl",
        "--silent",
        "--show-error",
        "--location",
        "--compressed",
        "--max-time",
        "45",
        "--retry",
        "3",
        "--retry-delay",
        "2",
        # Deliberately *not* `--retry-all-errors`. Curl's default retry covers
        # timeouts, 5xx and 429, which are the failures worth asking again
        # about; retrying a 404 is asking the same question more loudly.
        "--user-agent",
        AGENT,
    ]
    for key, value in (headers or {}).items():
        command += ["--header", f"{key}: {value}"]
    command.append(url)

    done = subprocess.run(command, capture_output=True)
    if done.returncode != 0:
        raise RuntimeError(done.stderr.decode(errors="replace").strip())
    return done.stdout


# -- zones --------------------------------------------------------------------


def slug(title: str) -> str:
    """A file name from a zone title, keeping the disambiguator."""
    title = title.replace("'", "").lower()
    title = re.sub(r"[^a-z0-9]+", "-", title)
    return title.strip("-")


def unticked() -> list[str]:
    """Zone titles from the checklist that have not been done yet."""
    if not ZONES.exists():
        sys.exit(f"no checklist at {ZONES}")
    out = []
    for line in ZONES.read_text().splitlines():
        found = re.match(r"- \[ \] (.+)", line.strip())
        if not found:
            continue
        # Every checklist entry is a real page title — the disambiguators are
        # the wiki's own, not ours — so nothing is rewritten here. A title that
        # turns out not to exist is reported by name rather than guessed at.
        out.append(found.group(1).strip())
    return out


def wiki_page(title: str) -> dict:
    """A page's intro extract, its wikitext, and its categories.

    The extract is the readable summary, the wikitext is where the infobox
    lives (and so the `UiMapID`), and the categories are the most reliable way
    to reconstruct factions — the rendered `Major factions` section is a
    navbox template and comes back empty.
    """
    query = urlencode(
        {
            "action": "query",
            "format": "json",
            "formatversion": "2",
            "redirects": "1",
            "titles": title,
            "prop": "extracts|revisions|categories",
            "explaintext": "1",
            "exintro": "1",
            "rvprop": "content",
            "rvslots": "main",
            "cllimit": "max",
        }
    )
    body = json.loads(fetch(f"{WIKI}?{query}"))
    pages = body.get("query", {}).get("pages", [])
    if not pages or pages[0].get("missing"):
        return {}
    page = pages[0]
    revisions = page.get("revisions") or [{}]
    return {
        "title": page.get("title", title),
        "extract": page.get("extract", ""),
        "wikitext": revisions[0].get("slots", {}).get("main", {}).get("content", ""),
        "categories": [c["title"] for c in page.get("categories", [])],
    }


# The zone infobox does not carry a UiMapID — checked against seventy-four
# zones, none of them had one. The wiki keeps them on a single page instead,
# which is better: one fetch for the lot rather than one per zone.
MAPS = ROOT / "data" / "zones" / "_raw" / "_uimaps.json"
MAPS_RAW = ROOT / "data" / "zones" / "_raw" / "_uimaps.wikitext"


def pull_maps() -> dict[str, int]:
    """Every `UiMapID`, by zone name, from the wiki's own list.

    One page, one fetch, cached. This is what a session's zone joins to a lore
    entry on — the *name* is not unique, and two Nagrands in two expansions
    would otherwise share one history.
    """
    if MAPS.exists():
        return json.loads(MAPS.read_text())

    query = urlencode(
        {
            "action": "parse",
            "format": "json",
            "formatversion": "2",
            "page": "UiMapID",
            "prop": "wikitext",
        }
    )
    body = json.loads(fetch(f"{WIKI}?{query}"))
    text = body.get("parse", {}).get("wikitext", "")

    # Kept raw beside the parsed table. The page carries several tables — the
    # map *types* enum sits above the zone list and a loose "a number and a
    # name on a row" reader picks that one up instead, which is exactly what
    # happened the first time. Having the source on disk means the reader can
    # be fixed without asking the wiki again.
    MAPS_RAW.parent.mkdir(parents=True, exist_ok=True)
    MAPS_RAW.write_text(text)

    # The real shape, read off the page rather than guessed at:
    #
    #     ! ID !! Map Name !! Map Type !! Parent Map !! wago.tools !! Patch
    #     |-
    #     | 1 || [[:Durotar]] || Zone || <span title="ID 12">Kalimdor</span>
    #
    # The parent matters as much as the name. `Nagrand` appears twice — 107
    # under Outland and 550 under Draenor — so a reader that takes the first
    # gives the alternate-universe zone the wrong continent's history, which
    # is the exact failure the map id exists to prevent.
    KINDS = {"Zone", "Continent", "Micro"}
    rows: list[tuple[int, str, str]] = []
    retail = re.search(r"^==+ *Retail *=+\s*$", text, re.M)
    body_text = text[retail.end() :] if retail else text
    classic = re.search(r"^==+ *Classic *=+\s*$", body_text, re.M)
    if classic:
        body_text = body_text[: classic.start()]

    for line in body_text.splitlines():
        if not line.startswith("|") or "||" not in line:
            continue
        cells = [c.strip() for c in line.lstrip("|").split("||")]
        if len(cells) < 4 or not cells[0].isdigit() or cells[2] not in KINDS:
            continue
        name = re.match(r"\[\[:?([^\]|#]+)", cells[1])
        if not name:
            continue
        parent = re.sub(r"<[^>]+>", "", cells[3]).strip()
        rows.append((int(cells[0]), name.group(1).strip(), parent))

    # Keyed both by the plain name and by "Name (parent)", so a caller can ask
    # the precise question where the plain one is ambiguous.
    found: dict[str, int] = {}
    for number, name, parent in rows:
        found.setdefault(f"{name} ({parent})", number)
        found.setdefault(name, number)

    MAPS.parent.mkdir(parents=True, exist_ok=True)
    MAPS.write_text(json.dumps(found, indent=0, ensure_ascii=False, sort_keys=True))
    print(f"  {len(found)} map ids cached in {MAPS}")
    return found


def pull_zones(titles: list[str]) -> None:
    RAW.mkdir(parents=True, exist_ok=True)
    maps = pull_maps()
    for index, title in enumerate(titles, 1):
        target = RAW / f"{slug(title)}.json"
        if target.exists():
            print(f"  [{index}/{len(titles)}] {title} — already have it")
            continue

        print(f"  [{index}/{len(titles)}] {title}")
        try:
            page = wiki_page(title)
        except RuntimeError as error:
            print(f"      failed: {error}", file=sys.stderr)
            continue
        if not page:
            print("      no such page — check the title", file=sys.stderr)
            continue

        target.write_text(
            json.dumps(
                {
                    "zone": page["title"],
                    # By the wiki's title, which is what the map list is keyed
                    # by too. Absent rather than guessed where they disagree:
                    # a wrong id silently attaches another zone's history to a
                    # real evening.
                    "map": maps.get(page["title"]),
                    "extract": page["extract"],
                    "categories": page["categories"],
                    "wikitext": page["wikitext"],
                    "sources": [
                        {
                            "title": page["title"],
                            "url": "https://warcraft.wiki.gg/wiki/"
                            + quote(page["title"].replace(" ", "_")),
                        }
                    ],
                    "licence": "CC BY-SA 4.0",
                },
                indent=2,
                ensure_ascii=False,
            )
        )
        time.sleep(GAP)

    print(
        f"\nRaw pages in {RAW}. These are source material, not entries:\n"
        "write the summary and history in your own words into\n"
        "data/zones/<slug>.json — see docs/ZONES.md for the shape and why."
    )


# -- following the links ------------------------------------------------------

LINKED = ROOT / "data" / "zones" / "_raw" / "_linked.json"

# Sections whose links are worth following. A zone page links to two hundred and
# fifty pages and most of them are quests, mobs and generic nouns; these are the
# parts that name the *place* — its settlements, its rulers, its history.
WANTED_SECTIONS = ("History", "Notable characters", "People and culture", "Geography")

# Infobox fields that name somewhere or someone rather than describing a stat.
WANTED_FIELDS = ("major", "minor", "capital", "rulers", "affiliation", "faffiliation")

# Namespaces that are not articles.
NOT_ARTICLES = re.compile(r"^(Category|File|Image|Template|Help|Special|Talk|User):", re.I)

# How many links to follow per zone. Twelve is enough to cover a zone's towns,
# its leaders and the two or three landmarks with pages of their own, and small
# enough that the whole corpus stays a few hundred pages rather than tens of
# thousands.
PER_ZONE = 12

# The API takes many titles per call, and extracts cap at twenty of them. Sixty
# requests for twelve hundred pages rather than twelve hundred requests is the
# difference between polite and rude.
BATCH = 20


# Titles that are a quest, an achievement or a faction-specific variant of one.
# A zone's History section cites quests constantly and none of them is a place.
QUESTY = re.compile(r"\(Horde\)|\(Alliance\)|quest chain|storyline$", re.I)


def candidates(wikitext: str) -> list[tuple[str, int]]:
    """Links that name the *place*, scored by how central they are to it.

    Two signals, and neither is rarity — a first attempt ranked by how few zones
    mentioned a page, which is exactly backwards: `Bombay Cat` appears in one
    zone and is worthless, `Stormwind City` appears in twenty and is not. What
    matters is centrality to *this* zone.

    So: everything the infobox names as a settlement, a capital or a ruler wins
    outright, because that is the zone's own list of what it consists of.
    Everything else is ranked by how many times the article links it, because a
    page an article reaches for nine times is what the article is about.
    """
    scored: dict[str, int] = {}

    infobox = wikitext[: wikitext.find("\n\n")] if "\n\n" in wikitext else wikitext[:3000]
    for field in WANTED_FIELDS:
        for line in re.findall(rf"\|\s*{field}\s*=([^\n]*)", infobox, re.I):
            for title in re.findall(r"\[\[([^\]|#]+)", line):
                # Far above anything a link count can reach, so the infobox
                # always fills the slots first.
                scored[title.strip()] = 1_000

    for name in WANTED_SECTIONS:
        found = re.search(rf"^(==+) *{re.escape(name)} *=+\s*$", wikitext, re.M)
        if not found:
            continue
        depth = len(found.group(1))
        rest = wikitext[found.end() :]
        stop = re.search(rf"^={{2,{depth}}}\s*[^=\s]", rest, re.M)
        for title in re.findall(r"\[\[([^\]|#]+)", rest[: stop.start()] if stop else rest):
            title = title.strip()
            if title not in scored:
                scored[title] = 0
            if scored[title] < 1_000:
                scored[title] += 1

    # Insertion order is kept as the tiebreak. Sorting ties alphabetically cut
    # Terokkar's landmark list off at "G" — Shattrath, Skettis and Tuurem, the
    # zone's headline places, lost to the alphabet.
    return [
        (title, score)
        for title, score in scored.items()
        if title and not NOT_ARTICLES.match(title) and not QUESTY.search(title)
    ]


def rank(scored: list[tuple[str, int]]) -> list[tuple[str, int]]:
    """Best first, ties broken by where the article mentions them.

    Not alphabetically. Sorting ties by title cut Terokkar's landmark list off
    at "G" — Shattrath, Skettis and Tuurem, the zone's headline places, lost to
    the alphabet.
    """
    return [pair for _, pair in sorted(enumerate(scored), key=lambda p: (-p[1][1], p[0]))]


def pull_links() -> None:
    """Follow each zone's most *specific* links and keep their summaries.

    See `candidates` for how the few are chosen. Pages shared between zones are
    fetched once — `Stormwind City` is named by a dozen of them — which is most
    of why this is a few hundred requests rather than a few thousand.
    """
    files = sorted(glob.glob(str(RAW / "*.json")))
    files = [f for f in files if not Path(f).name.startswith("_")]
    if not files:
        sys.exit("no raw zone pages yet — run `pull.py zones` first")

    wanted: set[str] = set()
    for path in files:
        scored = candidates(json.load(open(path)).get("wikitext", ""))
        ranked = rank(scored)
        wanted.update(title for title, _ in ranked[:PER_ZONE])

    known = json.loads(LINKED.read_text()) if LINKED.exists() else {}
    todo = sorted(t for t in wanted if t not in known)
    print(
        f"  {len(wanted)} pages worth following across {len(files)} zones\n"
        f"  {len(known)} already pulled, {len(todo)} to go "
        f"({(len(todo) + BATCH - 1) // BATCH} requests)\n"
    )

    for start in range(0, len(todo), BATCH):
        batch = todo[start : start + BATCH]
        query = urlencode(
            {
                "action": "query",
                "format": "json",
                "formatversion": "2",
                "redirects": "1",
                "titles": "|".join(batch),
                "prop": "extracts",
                "explaintext": "1",
                "exintro": "1",
                "exlimit": "max",
            }
        )
        try:
            body = json.loads(fetch(f"{WIKI}?{query}"))
        except RuntimeError as error:
            print(f"      batch failed: {error}", file=sys.stderr)
            continue

        for page in body.get("query", {}).get("pages", []):
            if page.get("missing"):
                continue
            extract = (page.get("extract") or "").strip()
            if extract:
                known[page["title"]] = extract
        # A title that redirected or does not exist is recorded as empty so the
        # next run does not ask about it again forever.
        for title in batch:
            known.setdefault(title, "")

        LINKED.write_text(json.dumps(known, indent=0, ensure_ascii=False, sort_keys=True))
        print(f"  {min(start + BATCH, len(todo)):>5}/{len(todo)}")
        time.sleep(GAP)

    filled = sum(1 for v in known.values() if v)
    print(f"\n{filled} linked pages in {LINKED}.")


# -- instances the Adventure Guide never wrote up -----------------------------

INSTANCES = ROOT / "data" / "instances"
INSTANCE_RAW = INSTANCES / "_raw"

# The raids Blizzard left blank, with the wiki title for each.
#
# Not an arbitrary list: the Adventure Guide arrived in Mists of Pandaria and
# nothing older was ever backfilled, so every raid released before it has an
# empty `description` in the API. They are also the ones whose story is most
# worth having and hardest to find now — nobody runs them at level, and the
# quests that used to explain them are gone.
BLANK = [
    ("Molten Core", "Molten Core"),
    ("Blackwing Lair", "Blackwing Lair"),
    ("Ruins of Ahn'Qiraj", "Ruins of Ahn%27Qiraj"),
    ("Temple of Ahn'Qiraj", "Ahn%27Qiraj_Temple"),
    ("Naxxramas", "Naxxramas"),
    ("Onyxia's Lair", "Onyxia%27s_Lair"),
    ("Karazhan", "Karazhan"),
    ("Gruul's Lair", "Gruul%27s_Lair"),
    ("Magtheridon's Lair", "Magtheridon%27s_Lair"),
    ("Serpentshrine Cavern", "Serpentshrine_Cavern"),
    # "The Eye" alone is a disambiguation page — the wiki qualifies the raid by
    # the structure it is a wing of. Caught by the pull coming back at a tenth
    # the size of every other raid.
    ("The Eye", "The Eye (Tempest Keep)"),
    ("The Battle for Mount Hyjal", "Battle_for_Mount_Hyjal"),
    ("Black Temple", "Black_Temple"),
    ("Sunwell Plateau", "Sunwell_Plateau"),
    ("Vault of Archavon", "Vault_of_Archavon"),
    ("The Obsidian Sanctum", "The_Obsidian_Sanctum"),
    ("The Eye of Eternity", "The_Eye_of_Eternity"),
    ("Ulduar", "Ulduar"),
    ("Trial of the Crusader", "Trial_of_the_Crusader"),
    ("Icecrown Citadel", "Icecrown_Citadel"),
    ("The Ruby Sanctum", "The_Ruby_Sanctum"),
]


def pull_instances() -> None:
    """Wiki pages for the raids Blizzard's own guide has nothing to say about.

    The same shape as `pull.py zones`: this fetches source material, and the
    writing is a separate step in Armory's own words. See `docs/ZONES.md` —
    every rule there applies here, including that the wiki is CC BY-SA and gets
    summarised rather than pasted.
    """
    INSTANCE_RAW.mkdir(parents=True, exist_ok=True)
    for index, (name, title) in enumerate(BLANK, 1):
        target = INSTANCE_RAW / f"{slug(name)}.json"
        if target.exists():
            print(f"  [{index}/{len(BLANK)}] {name} — already have it")
            continue
        print(f"  [{index}/{len(BLANK)}] {name}")
        try:
            page = wiki_page(title.replace("_", " ").replace("%27", "'"))
        except RuntimeError as error:
            print(f"      failed: {error}", file=sys.stderr)
            continue
        if not page:
            print("      no such page — check the title", file=sys.stderr)
            continue
        target.write_text(
            json.dumps(
                {
                    "instance": name,
                    "extract": page["extract"],
                    "categories": page["categories"],
                    "wikitext": page["wikitext"],
                    "sources": [
                        {
                            "title": page["title"],
                            "url": "https://warcraft.wiki.gg/wiki/"
                            + quote(page["title"].replace(" ", "_")),
                        }
                    ],
                    "licence": "CC BY-SA 4.0",
                },
                indent=2,
                ensure_ascii=False,
            )
        )
        time.sleep(GAP)

    print(f"\nRaw pages in {INSTANCE_RAW}. Source material, not entries.")


# -- drop rates, from Rarity --------------------------------------------------

DROPS = ROOT / "data" / "drops.json"
RARITY = "https://api.github.com/repos/WowRarity/Rarity/contents/DB"
RARITY_RAW = "https://raw.githubusercontent.com/WowRarity/Rarity/master/"


def pull_drops() -> None:
    """Drop-rate estimates out of the Rarity addon's database.

    Blizzard publishes no drop chance for anything. Rarity's numbers are its
    authors' researched best guesses, largely from Wowhead's crowdsourced
    observations, and the addon lets a user override any of them — so they are
    estimates with a provenance, not facts, and this records which ones its own
    authors flagged as guesses.

    `chance = 100` means **one in a hundred**, not a certainty. Reading it as a
    percentage would turn every rare mount into a guaranteed one.

    Rarity is GPL v2 and Armory is GPL v3, which is a real incompatibility for
    anything *distributed*. This is a local build step for a personal install;
    if Armory is ever published, this file is the thing to remove.
    """
    listing = json.loads(fetch(RARITY, {"Accept": "application/vnd.github+json"}))
    folders = [entry["path"] for entry in listing if entry.get("type") == "dir"]

    files: list[str] = []
    for folder in folders:
        inner = json.loads(
            fetch(
                f"https://api.github.com/repos/WowRarity/Rarity/contents/{folder}",
                {"Accept": "application/vnd.github+json"},
            )
        )
        files += [e["path"] for e in inner if e.get("name", "").endswith(".lua")]
        time.sleep(0.3)

    print(f"  {len(files)} database files across {len(folders)} folders")

    drops: dict[str, dict] = {}
    for path in files:
        try:
            text = fetch(RARITY_RAW + quote(path)).decode("utf-8", "replace")
        except RuntimeError as error:
            print(f"      {path}: {error}", file=sys.stderr)
            continue

        # Entries open with `["Name"] = {` and close at the matching indent.
        for block in re.split(r'\n\t?\[".*?"\] = \{', "\n" + text)[1:]:
            block = block.split("\n\t},")[0]
            item = re.search(r"itemId = (\d+)", block)
            chance = re.search(r"chance = (\d+)(,\s*--\s*(.*))?", block)
            if not item or not chance:
                continue
            name = re.search(r'name = L\["(.*?)"\]', block)
            spell = re.search(r"spellId = (\d+)", block)
            drops[item.group(1)] = {
                "name": name.group(1) if name else None,
                "spell": int(spell.group(1)) if spell else None,
                # One in this many. Not a percentage.
                "one_in": int(chance.group(1)),
                # Rarity's own authors mark the ones they are unsure of.
                "estimated": bool(chance.group(3) and "estimate" in chance.group(3).lower()),
                "npcs": [int(n) for n in re.findall(r"\d+", (re.search(r"npcs = \{(.*?)\}", block, re.S) or re.match("", "")).group(1) if re.search(r"npcs = \{(.*?)\}", block, re.S) else "")],
                # The readable creature names, which the coords carry and
                # nothing else in the entry does.
                "from": re.findall(r'n = L\["(.*?)"\]', block),
            }
        time.sleep(0.2)

    DROPS.write_text(json.dumps(drops, indent=1, ensure_ascii=False, sort_keys=True))
    guessed = sum(1 for d in drops.values() if d["estimated"])
    print(f"\n{len(drops)} drop rates in {DROPS} ({guessed} flagged as estimates by Rarity)")


# -- items --------------------------------------------------------------------


def token() -> str:
    """A client-credentials token, from `BNET_ID` and `BNET_SECRET`."""
    client = os.environ.get("BNET_ID")
    secret = os.environ.get("BNET_SECRET")
    if not client or not secret:
        sys.exit(
            "set BNET_ID and BNET_SECRET.\n"
            "These are the same credentials Armory itself uses; the secret is "
            "in your keyring under us.hagreli.Armory."
        )
    body = subprocess.run(
        [
            "curl",
            "--silent",
            "--show-error",
            "--user",
            f"{client}:{secret}",
            "--data",
            "grant_type=client_credentials",
            "https://oauth.battle.net/token",
        ],
        capture_output=True,
        check=True,
    ).stdout
    return json.loads(body)["access_token"]


def pull_items(region: str = "us") -> None:
    """Every item id and name Blizzard's search will hand over.

    This is the bulk answer to the thing the market browser does slowly: it
    fetches names one at a time, a hundred and fifty a sync, against a market of
    tens of thousands. A file built here fills the `item` table in one go.

    Sliced by id range because the search endpoint caps how deep paging can go —
    a thousand rows a page and a ceiling on the page number means the only way
    to see everything is to ask narrower questions.
    """
    access = token()
    headers = {"Authorization": f"Bearer {access}"}
    known: dict[str, str] = {}
    if ITEMS.exists():
        known = json.loads(ITEMS.read_text())
        print(f"  starting from {len(known)} names already pulled")

    STEP = 5_000
    CEILING = 250_000
    for low in range(0, CEILING, STEP):
        high = low + STEP
        page = 1
        while True:
            query = urlencode(
                {
                    "namespace": "static-" + region,
                    "id": f"[{low},{high})",
                    "orderby": "id",
                    "_page": page,
                    "_pageSize": 1000,
                    "locale": "en_US",
                }
            )
            url = f"https://{region}.api.blizzard.com/data/wow/search/item?{query}"
            try:
                body = json.loads(fetch(url, headers))
            except RuntimeError as error:
                print(f"      {low}-{high} page {page} failed: {error}", file=sys.stderr)
                break

            results = body.get("results", [])
            for entry in results:
                data = entry.get("data", {})
                item = data.get("id")
                name = (data.get("name") or {}).get("en_US")
                if item and name:
                    known[str(item)] = name

            if len(results) < 1000:
                break
            page += 1
            time.sleep(0.2)

        print(f"  {low:>7}-{high:<7} {len(known)} names")
        ITEMS.write_text(json.dumps(known, indent=0, ensure_ascii=False, sort_keys=True))

    print(f"\n{len(known)} item names in {ITEMS}.")


# -- entry --------------------------------------------------------------------


def main() -> None:
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    what = sys.argv[1]

    if what == "maps":
        pull_maps()
    elif what == "zones":
        titles = sys.argv[2:] or unticked()
        print(f"Pulling {len(titles)} zone pages from warcraft.wiki.gg\n")
        pull_zones(titles)
    elif what == "links":
        pull_links()
    elif what == "instances":
        pull_instances()
    elif what == "drops":
        pull_drops()
    elif what == "items":
        pull_items(os.environ.get("BNET_REGION", "us"))
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
