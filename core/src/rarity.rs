//! Drop chances, read out of the Rarity addon a person already has installed.
//!
//! Armory has never had drop rates and cannot get any of its own: Blizzard
//! publishes none, and Wowhead — which is where the community's numbers come
//! from — forbids automated access. What it can do is read a database somebody
//! has already chosen to install on their own machine.
//!
//! ## Read, never shipped
//!
//! **Nothing from Rarity is redistributed with Armory and nothing is fetched.**
//! Rarity is GPL-2.0 with no "or later" grant and Armory is GPL-3.0-or-later,
//! which are not compatible licences — so its database cannot go in this
//! repository or in the binary. Reading a file on the machine that is already
//! running both is a different act entirely, and it is the same one Armory
//! already performs on its own collector addon. No Rarity, no rates, and the
//! pages fall back to what they said before.
//!
//! ## What the numbers are, and are not
//!
//! `chance = 100` means *one in a hundred*, not a hundred per cent. Rarity's
//! own documentation is plain about where the figures come from: Blizzard
//! publishes nothing, so its authors read Wowhead's observed rates and made a
//! best guess per item, and a user can override any of them in the addon's
//! options. They are estimates by people who care, not measurements — which is
//! exactly how the interface has to quote them.
//!
//! ## This is not a Lua interpreter, and must not become one
//!
//! The same rule as [`super::addon::lua`], for a harder input: these files are
//! hand-written Lua with `LibStub` calls, `CONSTANTS.UIMAPIDS.AZSUNA`
//! references and an early `return {}` guard, and evaluating them is not on the
//! table. What this does is scan for a shape it knows —
//!
//! ```lua
//! ["Cloudwing Hippogryph"] = {
//!     spellId = 242881,
//!     itemId = 147806,
//!     chance = 20,
//!     coords = { { m = CONSTANTS.UIMAPIDS.AZSUNA } },
//! },
//! ```
//!
//! — and take the four scalar fields it understands, skipping every nested
//! table and ignoring every expression. An entry it cannot read whole is
//! dropped rather than half-read, because a chance attached to the wrong item
//! is worse than no chance at all.

use std::collections::HashMap;
use std::path::Path;

use super::source::blizzard::collections::{Collectible, Kind};

/// The addon folder this reads, and the subdirectory the database is in.
const ADDON: &str = "Rarity";
const DATABASE: &str = "DB";

/// One item's estimated drop chance, and the ids it can be joined on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chance {
    /// The English name Rarity keys the entry by. Kept for the tooltip, not for
    /// joining — Armory's names come from the client in the user's own locale.
    pub name: String,
    /// A mount's summoning spell. What Armory's mount `link_id` is.
    pub spell_id: Option<u32>,
    /// A pet's creature. What Armory's pet `link_id` is.
    pub creature_id: Option<u32>,
    /// The item that teaches or contains it. What a toy's `link_id` is.
    pub item_id: Option<u32>,
    /// One in this many. Never zero — an entry saying `chance = 0` is dropped,
    /// because a one-in-nothing is not a probability.
    pub one_in: u32,
}

/// Every drop chance Rarity knows, indexed the three ways a collectible joins.
#[derive(Debug, Clone, Default)]
pub struct Chances {
    by_spell: HashMap<u32, u32>,
    by_creature: HashMap<u32, u32>,
    by_item: HashMap<u32, u32>,
    /// How many entries were read, for the line that says where this came from.
    pub known: usize,
}

impl Chances {
    pub fn from(entries: Vec<Chance>) -> Chances {
        let mut chances = Chances {
            known: entries.len(),
            ..Chances::default()
        };
        for entry in entries {
            if let Some(spell) = entry.spell_id {
                chances.by_spell.insert(spell, entry.one_in);
            }
            if let Some(creature) = entry.creature_id {
                chances.by_creature.insert(creature, entry.one_in);
            }
            if let Some(item) = entry.item_id {
                chances.by_item.insert(item, entry.one_in);
            }
        }
        chances
    }

    pub fn is_empty(&self) -> bool {
        self.known == 0
    }

    /// One in how many, for a thing Armory knows about.
    ///
    /// Joined on `link_id`, which is a different id space per kind and is
    /// exactly the one Rarity keys by in each case: a mount is its summoning
    /// spell, a pet is its creature, a toy is the item that wraps it. Joining
    /// on anything else would attach one thing's odds to another's name, and
    /// the reason `link_id` exists at all is that those id spaces are not
    /// interchangeable.
    ///
    /// A toy whose `link_id` is still the collection id standing in for an item
    /// is refused, for the same reason its Wowhead link is: the id is a guess,
    /// and a guess joined against a real table lands on a real wrong answer.
    pub fn one_in(&self, collectible: &Collectible) -> Option<u32> {
        if collectible.item_id_is_guessed() {
            return None;
        }
        match collectible.kind {
            Kind::Mount => self.by_spell.get(&collectible.link_id),
            Kind::Pet => self.by_creature.get(&collectible.link_id),
            Kind::Toy | Kind::Decor => self.by_item.get(&collectible.link_id),
        }
        .copied()
    }
}

/// Read every database file in an installed Rarity.
///
/// Absent, unreadable or unrecognisable all answer the same empty set: this is
/// an enrichment nobody has to have, and an installation that has moved on to a
/// shape this does not know must degrade to the page Armory drew before rather
/// than to a wrong number.
pub fn read(wow_path: &Path) -> Chances {
    let database = super::addon::addon_directory(wow_path, ADDON).join(DATABASE);
    let mut entries = Vec::new();
    collect(&database, &mut entries, 0);
    Chances::from(entries)
}

/// Walk the database directory, which is one level of subdirectories deep.
fn collect(directory: &Path, into: &mut Vec<Chance>, depth: usize) {
    // The database is `DB/`, `DB/Mounts/`, `DB/Pets/`, `DB/Toys/` and no
    // deeper. A bound rather than a trusted shape: this is somebody else's
    // folder and a symlink loop in it is not Armory's problem to discover the
    // hard way.
    if depth > 2 {
        return;
    }
    let Ok(read) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in read.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, into, depth + 1);
        } else if path.extension().is_some_and(|kind| kind == "lua") {
            if let Ok(source) = std::fs::read_to_string(&path) {
                into.extend(parse(&source));
            }
        }
    }
}

/// Pull every entry this understands out of one database file.
pub fn parse(source: &str) -> Vec<Chance> {
    let mut found = Vec::new();
    let mut open: Option<Entry> = None;
    // How deep inside the current entry's braces we are. An entry's own fields
    // are at one; anything deeper is `coords`, `npcs` or `items` and is skipped
    // whole, so a `m = 317` inside a coordinate can never be read as a field.
    let mut depth = 0usize;

    for line in source.lines() {
        let trimmed = line.trim();

        if open.is_none() {
            if let Some(name) = opens_entry(trimmed) {
                open = Some(Entry::new(name));
                depth = 1;
                // A single-line entry is not a shape this file uses, and
                // treating one as an opening brace would swallow the rest of
                // the table.
                depth += braces(trimmed) - 1;
            }
            continue;
        }

        let before = depth;
        depth = depth.saturating_add_signed(delta(trimmed));

        if depth == 0 {
            // The entry closed. Keep it only if it carried a usable chance and
            // at least one id to join it on.
            if let Some(entry) = open.take().and_then(Entry::finish) {
                found.push(entry);
            }
            continue;
        }
        if before == 1 && depth == 1 {
            if let Some(entry) = open.as_mut() {
                entry.read(trimmed);
            }
        }
    }
    found
}

/// The name a line opens an entry with, if it opens one.
///
/// `["Cloudwing Hippogryph"] = {`. Deliberately strict: the key has to be a
/// bracketed string and the line has to end in an opening brace, which is the
/// only form these files use for an item.
fn opens_entry(line: &str) -> Option<String> {
    let rest = line.strip_prefix("[\"")?;
    let (name, rest) = rest.split_once("\"]")?;
    let rest = rest.trim_start().strip_prefix('=')?.trim();
    rest.starts_with('{').then(|| name.to_string())
}

fn braces(line: &str) -> usize {
    line.chars().filter(|c| *c == '{').count()
}

/// How much deeper a line leaves the nesting.
fn delta(line: &str) -> isize {
    let opened = line.chars().filter(|c| *c == '{').count() as isize;
    let closed = line.chars().filter(|c| *c == '}').count() as isize;
    opened - closed
}

/// One entry, part-read.
struct Entry {
    name: String,
    spell_id: Option<u32>,
    creature_id: Option<u32>,
    item_id: Option<u32>,
    one_in: Option<u32>,
}

impl Entry {
    fn new(name: String) -> Entry {
        Entry {
            name,
            spell_id: None,
            creature_id: None,
            item_id: None,
            one_in: None,
        }
    }

    /// Take one of the four fields this understands, and ignore the rest.
    ///
    /// Only a number is accepted. `chance = CONSTANTS.SOMETHING`,
    /// `chance = true` and anything else that needs evaluating is left alone,
    /// because guessing is how a wrong figure reaches somebody's screen.
    ///
    /// The trailing comment is cut first. `chance = 100, -- Blind guess` is a
    /// real line in this database and eighty-odd entries carry one; a reader
    /// that stopped at the comma alone silently dropped every one of them.
    fn read(&mut self, line: &str) {
        let line = match line.split_once("--") {
            Some((before, _)) => before,
            None => line,
        };
        let Some((field, value)) = line.split_once('=') else {
            return;
        };
        let field = field.trim();
        let value = value.trim().trim_end_matches(',').trim();

        // Parsed as a float and rounded: `chance` is an integer for all but one
        // entry, which says `2.5`. One in two and a half is not something an
        // interface can say, and refusing the entry would lose a real figure
        // over a rounding decision.
        let Ok(number) = value.parse::<f64>() else {
            return;
        };
        if !number.is_finite() || number < 0.0 {
            return;
        }
        let number = number.round() as u32;

        match field {
            "spellId" => self.spell_id = Some(number),
            "creatureId" => self.creature_id = Some(number),
            "itemId" => self.item_id = Some(number),
            "chance" => self.one_in = Some(number),
            _ => {}
        }
    }

    /// The entry, if it is worth keeping.
    ///
    /// A chance with nothing to join it to is not usable, and a chance of zero
    /// is not a probability. Both are dropped rather than carried as an entry
    /// that will silently never match or will match with a nonsense figure.
    fn finish(self) -> Option<Chance> {
        let one_in = self.one_in.filter(|chance| *chance > 0)?;
        if self.spell_id.is_none() && self.creature_id.is_none() && self.item_id.is_none() {
            return None;
        }
        Some(Chance {
            name: self.name,
            spell_id: self.spell_id,
            creature_id: self.creature_id,
            item_id: self.item_id,
            one_in,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTS: &str = r#"
local addonName, addonTable = ...

local L = LibStub("AceLocale-3.0"):GetLocale("Rarity")
local CONSTANTS = addonTable.constants

if LE_EXPANSION_LEVEL_CURRENT < LE_EXPANSION_LEGION then
	return {}
end

local legionMounts = {
	-- 7.0
	["Cloudwing Hippogryph"] = {
		cat = CONSTANTS.ITEM_CATEGORIES.LEGION,
		type = CONSTANTS.ITEM_TYPES.MOUNT,
		method = CONSTANTS.DETECTION_METHODS.USE,
		name = L["Cloudwing Hippogryph"],
		spellId = 242881,
		itemId = 147806,
		items = { 152102 },
		chance = 20,
		coords = { { m = CONSTANTS.UIMAPIDS.AZSUNA } },
	},
	["Deathcharger's Reins"] = {
		method = CONSTANTS.DETECTION_METHODS.NPC,
		name = L["Deathcharger's Reins"],
		spellId = 17481,
		itemId = 13335,
		npcs = { 99999 },
		tooltipNpcs = { 45412 },
		chance = 100,
		statisticId = { 1097 },
		coords = { { m = 317, x = 38.6, y = 20, i = true } },
	},
}

Rarity.ItemDB.MergeItems(Rarity.ItemDB.mounts, legionMounts)
return legionMounts
"#;

    #[test]
    fn the_four_fields_come_out_and_the_rest_is_ignored() {
        let read = parse(MOUNTS);
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].name, "Cloudwing Hippogryph");
        assert_eq!(read[0].spell_id, Some(242_881));
        assert_eq!(read[0].item_id, Some(147_806));
        assert_eq!(read[0].one_in, 20);
        assert_eq!(read[1].one_in, 100);
    }

    #[test]
    fn a_number_inside_a_nested_table_is_never_read_as_a_field() {
        // `coords = { { m = 317, x = 38.6 } }` sits inside the entry and has
        // fields with the same shape as the entry's own. Reading one would put
        // a map id where an item id goes.
        let read = parse(MOUNTS);
        let deathcharger = &read[1];
        assert_eq!(deathcharger.item_id, Some(13_335));
        assert_eq!(deathcharger.creature_id, None);
    }

    #[test]
    fn the_lua_around_the_table_is_not_evaluated_and_not_mistaken_for_data() {
        // `LibStub(...)`, a `CONSTANTS` reference and an early `return {}` are
        // all in the fixture above. None of them produced an entry.
        assert_eq!(parse(MOUNTS).len(), 2);
        assert!(parse("local x = LibStub(\"Ace\"):GetLocale(\"Rarity\")").is_empty());
    }

    #[test]
    fn a_field_that_is_not_a_bare_number_is_left_alone() {
        let source = r#"
	["Odd One"] = {
		spellId = CONSTANTS.SOMETHING,
		itemId = 42,
		chance = 15,
	},
"#;
        let read = parse(source);
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].spell_id, None, "an expression is not a number");
        assert_eq!(read[0].item_id, Some(42));
    }

    #[test]
    fn an_entry_with_no_chance_or_nothing_to_join_on_is_dropped() {
        // Half an entry is worse than none: a chance with no id silently never
        // matches, and an id with no chance is a row that promises a figure it
        // does not have.
        assert!(parse("\t[\"No Chance\"] = {\n\t\titemId = 42,\n\t},").is_empty());
        assert!(parse("\t[\"No Id\"] = {\n\t\tchance = 20,\n\t},").is_empty());
        assert!(parse("\t[\"Zero\"] = {\n\t\titemId = 42,\n\t\tchance = 0,\n\t},").is_empty());
    }

    #[test]
    fn a_trailing_comment_does_not_hide_the_number() {
        // `chance = 100, -- Blind guess` is a real line and eighty-odd entries
        // carry one. Stopping at the comma alone silently dropped every one of
        // them, which read as Rarity simply not knowing those items.
        let source = r#"
	["Arfus"] = {
		spellId = 406225,
		itemId = 211271,
		items = { 209024 },
		chance = 100, -- Blind guess
		creatureId = 203463,
	},
"#;
        let read = parse(source);
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].one_in, 100);
        assert_eq!(read[0].creature_id, Some(203_463));
    }

    #[test]
    fn a_fractional_chance_is_rounded_rather_than_refused() {
        // Exactly one entry in the database says `2.5`. "One in two and a half"
        // is not something an interface can say, and dropping a real figure
        // over a rounding decision is the worse of the two answers.
        let read = parse("\t[\"Fel-Spotted Egg\"] = {\n\t\titemId = 1,\n\t\tchance = 2.5,\n\t},");
        assert_eq!(read[0].one_in, 3);
    }

    #[test]
    fn a_chance_is_one_in_that_many_and_not_a_percentage() {
        // Rarity's own documentation: "The estimated drop rate is 1 in 60,
        // i.e., 1.667%". A hundred here is a hundredth, not a certainty.
        let read = parse(MOUNTS);
        assert_eq!(read[1].one_in, 100);
        assert!(read[1].one_in > 1, "a hundred per cent would be one in one");
    }
}
