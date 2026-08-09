#!/usr/bin/env python3
"""Turn raw wiki pages into one readable brief per zone.

Everything the writing pass needs, in one file, with the markup gone: the
sections that describe the place, the infobox facts, and the intro paragraph of
every landmark the zone links to.

This exists because the raw pages are twenty thousand characters of wikitext
each and most of it is quest tables, creature lists and reference footnotes. A
brief is a tenth the size and reads like prose, which makes the writing pass
both cheaper and more consistent — every zone is looked at through the same
window rather than through whatever part of the markup happened to be noticed.

No network. Everything it reads is already on disk.

    ./scripts/brief.py            # every zone
    ./scripts/brief.py nagrand    # one, by slug
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from pull import candidates, rank  # noqa: E402  — same ranking, one implementation

ROOT = Path(__file__).resolve().parent.parent
RAW = ROOT / "data" / "zones" / "_raw"
OUT = RAW / "_briefs"

# The sections worth keeping. Everything else on a zone page is a table of
# quests, mobs, herbs or fish.
KEEP = (
    "History",
    # 251,000 characters across 67 zones, and it was being discarded whole.
    # Found by auditing which top-level sections the brief drops rather than by
    # anybody noticing — the technique is worth more than this one result. The
    # RPG sourcebooks are where a zone's deep past usually lives; an agent
    # writing Borean Tundra said its history was "almost entirely RPG-sourced"
    # and had to reconstruct from linked pages what was sitting in this section.
    "In the RPG",
    # The in-game blurb, where a page carries one.
    "Description",
    "People and culture",
    "Geography",
    "Notable characters",
    "Adjacent regions",
    "Travel hubs",
    "Notes",
    "Notes and trivia",
)

FIELDS = (
    ("faction", "Controlled by"),
    ("capital", "Capital"),
    ("races", "Races"),
    ("rulers", "Rulers"),
    ("major", "Major settlements"),
    ("minor", "Minor settlements"),
    ("affiliation", "Affiliation"),
    ("faffiliation", "Hostile affiliation"),
    ("loc", "Location"),
)


def plain(text: str) -> str:
    """Wikitext to something a person can read.

    Deliberately lossy and in this order — refs first, because a citation can
    contain a link and stripping links first leaves its punctuation behind.
    """
    text = re.sub(r"<ref[^>]*/>", "", text)
    text = re.sub(r"<ref.*?</ref>", "", text, flags=re.S)
    text = re.sub(r"<!--.*?-->", "", text, flags=re.S)
    # `[[Page|shown]]` keeps what was shown; `[[Page]]` keeps the page.
    text = re.sub(r"\[\[[^\]|]*\|([^\]]*)\]\]", r"\1", text)
    text = re.sub(r"\[\[:?([^\]|#]*)[^\]]*\]\]", r"\1", text)
    # Templates. A great many of them are links wearing a template's clothes —
    # `{{Au|Tuurem}}`, `{{Au|Blackhand|Blackhand the Destroyer}}` — and deleting
    # them whole is how Talador's history came out reading "the draenei city of
    # ___" and "the ruthless Warlord ___". Every proper noun in the paragraph
    # was inside one.
    #
    # So a template *with* parameters keeps its last one, which is the display
    # text by MediaWiki convention. A template with none is an icon or a stub
    # and is worth nothing. Run twice for the nested ones.
    for _ in range(2):
        text = re.sub(r"\{\{[^{}|]*\|([^{}]*)\}\}", lambda m: m.group(1).split("|")[-1], text)
        text = re.sub(r"\{\{[^{}]*\}\}", "", text)
    text = re.sub(r"'''?", "", text)
    text = re.sub(r"<[^>]+>", "", text)
    text = re.sub(r"^[*#:]+ *", "- ", text, flags=re.M)
    text = re.sub(r"\n{3,}", "\n\n", text)
    return text.strip()


def sections(wikitext: str) -> list[tuple[str, str]]:
    """The kept sections, in the order the article puts them."""
    out = []
    for name in KEEP:
        found = re.search(rf"^(==+) *{re.escape(name)} *=+\s*$", wikitext, re.M)
        if not found:
            continue
        depth = len(found.group(1))
        rest = wikitext[found.end() :]
        # The stop pattern must not require a space after the equals signs.
        # `==Notes==` is as valid as `== Notes ==`, and demanding the space
        # meant most sections never terminated — every one ran to the end of
        # the article, so a brief repeated its own content four or five times.
        stop = re.search(rf"^={{2,{depth}}}\s*[^=\s]", rest, re.M)
        body = rest[: stop.start()] if stop else rest
        body = plain(body)
        if len(body) > SECTION_CHARS:
            cut = body.rfind("\n\n", 0, SECTION_CHARS)
            body = (body[:cut] if cut > SECTION_CHARS // 2 else body[:SECTION_CHARS]) + "\n\n[…]"
        if body:
            out.append((name, body))
    return out


def infobox(wikitext: str) -> list[tuple[str, str]]:
    head = wikitext[: wikitext.find("\n\n")] if "\n\n" in wikitext else wikitext[:3000]
    out = []
    for field, label in FIELDS:
        found = re.search(rf"\|\s*{field}\s*=([^\n]*)", head, re.I)
        if not found:
            continue
        value = plain(found.group(1)).replace("\n", " ").strip(" -,")
        if value:
            out.append((label, value))
    return out


# How much of a section to keep, and how many landmarks.
#
# A brief exists to be *read whole*. A first cut inlined every linked page that
# had prose and came out at seven million characters — larger than the corpus it
# was summarising, because the shared pages were repeated into every zone that
# mentioned them. These caps are what make it a brief.
SECTION_CHARS = 6_000
PLACES = 12
PLACE_CHARS = 320


def linked(wikitext: str, prose: dict[str, str]) -> list[tuple[str, str]]:
    """The landmarks this zone is *about*, with a sentence or two each.

    The same selection `pull.py links` made — infobox settlements and rulers
    first, then whatever the article reaches for most — rather than every link
    on the page. Ranking matters here for the same reason it did there: a zone
    names two hundred and fifty pages and a dozen of them are the place.
    """
    scored = candidates(wikitext)
    ranked = rank(scored)
    out = []
    for title, _ in ranked:
        text = prose.get(title)
        if not text:
            continue
        text = text.strip().split("\n")[0]
        if len(text) > PLACE_CHARS:
            cut = text.rfind(". ", 0, PLACE_CHARS)
            text = text[: cut + 1] if cut > PLACE_CHARS // 2 else text[:PLACE_CHARS] + "…"
        out.append((title, text))
        if len(out) >= PLACES:
            break
    return out


def brief(slug: str, raw: dict, prose: dict[str, str]) -> str:
    lines = [f"# {raw['zone']}", ""]
    if raw.get("map"):
        lines += [f"UiMapID **{raw['map']}** — this is the join key.", ""]
    else:
        lines += [
            "No UiMapID on record. The wiki's table stops at patch 10.1.7, so",
            "newer zones get theirs from the addon instead. Leave `map` null.",
            "",
        ]

    facts = infobox(raw.get("wikitext", ""))
    if facts:
        lines.append("## What the infobox says")
        lines += [f"- **{label}**: {value}" for label, value in facts]
        lines.append("")

    if raw.get("extract"):
        lines += ["## Opening paragraph", "", raw["extract"].strip(), ""]

    for name, body in sections(raw.get("wikitext", "")):
        lines += [f"## {name}", "", body, ""]

    places = linked(raw.get("wikitext", ""), prose)
    if places:
        lines += ["## Places and people this zone is about", ""]
        for title, text in places:
            lines += [f"**{title}** — {text}", ""]

    lines += [
        "## Attribution",
        "",
        "Everything above is from warcraft.wiki.gg, CC BY-SA 4.0. Summarise it",
        "in your own words; do not paste it. Cite every page used in `sources`.",
        "",
    ]
    return "\n".join(lines)


def main() -> None:
    prose = json.loads((RAW / "_linked.json").read_text())
    OUT.mkdir(parents=True, exist_ok=True)

    wanted = sys.argv[1:]
    total = 0
    for path in sorted(RAW.glob("*.json")):
        if path.name.startswith("_"):
            continue
        slug = path.stem
        if wanted and slug not in wanted:
            continue
        raw = json.loads(path.read_text())
        text = brief(slug, raw, prose)
        (OUT / f"{slug}.md").write_text(text)
        total += len(text)
        print(f"  {slug:<34} {len(text):>7,} chars")

    print(f"\n{total:,} chars of briefs in {OUT}")


if __name__ == "__main__":
    main()
