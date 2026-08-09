#!/usr/bin/env python3
"""Check the zone entries are what they claim to be.

Run after each writing batch. Cheap, local, and it catches the four things that
actually go wrong: a missing field, a history that has drifted into an essay, a
`map` that was guessed at, and — the one that matters most — prose lifted out of
the brief rather than written from it.

    ./scripts/check_zones.py
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ZONES = ROOT / "data" / "zones"
BRIEFS = ZONES / "_raw" / "_briefs"

# The Chronicle transcripts, if they are on this machine.
#
# These matter more than the briefs do. The wiki is CC BY-SA and an accidental
# echo of it is a licence footnote; the audiobooks are Blizzard's own prose read
# aloud, so a shared run with one is a passage of a copyrighted book sitting in
# a data file. The brief check would never have seen it — it compares against
# the wiki and nothing else.
CHRONICLE = Path.home() / "Downloads" / "World of Warcraft (Audiobooks)"
ABRIDGED = Path.home() / "Downloads"

REQUIRED = ("zone", "map", "expansion", "summary", "history", "factions", "notable", "sources", "licence")

# Long enough that a shared phrase is not a coincidence. Names and short stock
# phrases will always overlap; a run of this many characters means a sentence
# was copied.
LIFTED = 90


def shingles(text: str, size: int) -> set[str]:
    text = re.sub(r"\s+", " ", text.lower())
    return {text[i : i + size] for i in range(0, max(0, len(text) - size), 8)}


def source_prose() -> set[str]:
    """Every shingle of every Chronicle transcript on this machine.

    Built once and reused, because it is a couple of hundred thousand words and
    a hundred and forty-three entries would otherwise rebuild it each time.
    """
    found: set[str] = set()
    for folder, pattern in ((CHRONICLE, "*.txt"), (ABRIDGED, "*Abridgment*.txt")):
        if not folder.exists():
            continue
        for path in folder.glob(pattern):
            found |= shingles(path.read_text(errors="replace"), LIFTED)
    return found


def check(path: Path, chronicle: set[str]) -> list[str]:
    problems = []
    try:
        entry = json.loads(path.read_text())
    except json.JSONDecodeError as error:
        return [f"not valid JSON: {error}"]

    for field in REQUIRED:
        if field not in entry:
            problems.append(f"missing `{field}`")
    if problems:
        return problems

    # A ceiling rather than a target. The first pass capped this at 140 and
    # that was too tight: a zone whose history is genuinely eventful was made
    # to trade one fact for another, and several agents reported leaving real
    # material out because "nothing there was worth cutting for it". Concision
    # is still the default — most zones say what they have in 120 — but the
    # budget should not be what stops a place with more story from telling it.
    words = len(entry["history"].split())
    if not 60 <= words <= 260:
        problems.append(f"history is {words} words (60-260, aim for 120-160)")
    if not 1 <= len(entry["summary"].split(".")) - 1 <= 4:
        problems.append("summary is not 2-3 sentences")
    if not 3 <= len(entry["notable"]) <= 6:
        problems.append(f"{len(entry['notable'])} notable entries (want 3-6)")
    if not entry["sources"]:
        problems.append("no sources — attribution is not optional")
    if entry["licence"] != "CC BY-SA 4.0":
        problems.append(f"licence is {entry['licence']!r}")

    brief = BRIEFS / f"{path.stem}.md"
    if brief.exists():
        source = shingles(brief.read_text(), LIFTED)
        for field in ("summary", "history"):
            written = shingles(entry[field], LIFTED)
            if written & source:
                problems.append(f"`{field}` shares a {LIFTED}-char run with the brief — lifted?")
        stated = re.search(r"UiMapID \*\*(\d+)\*\*", brief.read_text())
        if stated and entry["map"] != int(stated.group(1)):
            problems.append(f"map is {entry['map']}, brief says {stated.group(1)}")
        if not stated and entry["map"] is not None:
            problems.append(f"map is {entry['map']} but the brief has none on record")

    if chronicle:
        for field in ("summary", "history"):
            if shingles(entry[field], LIFTED) & chronicle:
                problems.append(
                    f"`{field}` shares a {LIFTED}-char run with a Chronicle "
                    "transcript — that is Blizzard's own prose, rewrite it"
                )

    return problems


def main() -> None:
    files = sorted(p for p in ZONES.glob("*.json"))
    if not files:
        sys.exit("no entries written yet")

    chronicle = source_prose()
    if chronicle:
        print(f"checking against {len(chronicle):,} shingles of Chronicle prose\n")
    else:
        print("no Chronicle transcripts found — skipping that check\n")

    bad = 0
    for path in files:
        problems = check(path, chronicle)
        if problems:
            bad += 1
            print(f"{path.name}")
            for problem in problems:
                print(f"    {problem}")

    print(f"\n{len(files)} entries, {len(files) - bad} clean, {bad} with problems")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
