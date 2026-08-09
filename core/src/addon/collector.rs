//! Reading what the collector addon wrote.
//!
//! The shape is fixed by `Armory_Collector.lua`, which is in this repository —
//! both halves of this format are ours, so it can be narrow and explicit rather
//! than defensive about a schema someone else owns.
//!
//! Two files. `ArmoryCollectorDB` is account-wide: attribution, criteria,
//! collections, currencies, the Warband bank. `ArmoryCollectorCharDB` is one
//! character, and there is one of them per character folder. They are split
//! because a completed-quest list is several thousand ids and twenty-three of
//! those in one file would run at the Lua constant-table ceiling.
//!
//! Between them these are enough for Armory to work with no web API at all,
//! which matters more than it sounds: Blizzard's developer portal has been
//! answering 500 to client creation since late 2025, and an application that
//! cannot be used without it is an application that cannot be used.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, TimeZone, Utc};

use super::lua::{self, Value};
use crate::achievement::{Criterion, CriterionKind, PrimaryData};
use crate::character::{Character, CharacterKey, Detail, Equipped, Faction, Profession, RaidLock};
use crate::market::{Reagent, Recipe, RecipeBooks};
use crate::provenance::{Earned, EarnedCurrency, EarnedReputation};
use crate::source::blizzard::collections::{Collectible, Kind, Source};
use crate::source::blizzard::gamedata::Achievement;
use crate::source::blizzard::profile::AchievementProgress;
use crate::source::blizzard::realm_slug;
use crate::tally::{Counting, Tallies, Tally};

/// The account-wide global the addon declares.
const GLOBAL: &str = "ArmoryCollectorDB";
/// The per-character one.
const CHARACTER_GLOBAL: &str = "ArmoryCollectorCharDB";

/// The format this reader understands. Bumped when the addon's shape changes,
/// and checked so that a newer addon writing an older application's folder is
/// reported rather than misread.
pub const FORMAT: u32 = 5;

/// What one dump of the addon's account-wide data contains.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Collected {
    /// Achievement id to the character who earned it.
    pub earned_by: HashMap<u32, CharacterKey>,
    /// Achievement id to when the account finished it.
    pub completed: HashMap<u32, DateTime<Utc>>,
    /// Achievement id to its criteria ids, flat.
    ///
    /// Enough to rebuild the tree the web API would have given. Flat rather
    /// than nested because the game hands over a flat list, and every leaf is
    /// what observability turns on anyway.
    pub tree: HashMap<u32, Vec<u64>>,
    /// Criterion id to what it measures.
    pub criteria: HashMap<u64, CriterionKind>,
    /// The achievement catalogue: names, points, categories, descriptions.
    ///
    /// The web API has an endpoint for this, but it is one call per achievement
    /// across several thousand of them and it needs a client the developer
    /// portal will not always issue. The game already knows, so this is both
    /// cheaper and available when the API is not.
    pub catalogue: HashMap<u32, Achievement>,
    /// Currency id to amount, per character.
    pub currencies: HashMap<CharacterKey, HashMap<u32, u64>>,
    /// Item id to count, across the Warband bank.
    pub warband_bank: HashMap<u32, u64>,
    /// Mounts, pets and toys: everything that exists.
    pub collectibles: Vec<Collectible>,
    /// What of it the account has.
    pub owned: HashSet<(Kind, u32)>,
    /// What each character has personally been observed earning.
    ///
    /// The reputation half is what makes an inherited standing measurable
    /// again: the standing was at the ceiling before the run began and cannot
    /// move, but the work can still be counted as it arrives. The currency half
    /// is what tells earned from transferred from already-there. Neither has an
    /// endpoint, or could — no API attributes a point of reputation or a copper
    /// of a currency to a character.
    pub earned: HashMap<CharacterKey, Earned>,
    /// What each character can make, and what it takes.
    ///
    /// Read from the profession window rather than at login, because
    /// `GetAllRecipeIDs` answers an empty table until one has been opened.
    /// A character missing from here has not opened theirs, which is silence
    /// rather than a character who can make nothing.
    pub recipes: RecipeBooks,
    /// Counters no Blizzard system keeps, per character.
    ///
    /// Not session facts and not derivable from them: a chronicle covers the
    /// evenings Armory has seen and these cover every evening since the addon
    /// was installed. See `model::tally`.
    pub tallies: Tallies,
    /// Pet species to how many of it the journal holds.
    ///
    /// Account state, like [`Collected::owned`], rather than a property of the
    /// pet — which is why it is not on the catalogue row. A count above one is
    /// what makes a pet a spare, and caging the only copy of a pet takes it out
    /// of the collection, so nothing can be recommended for sale without this.
    pub pets_held: HashMap<u32, u32>,
    /// When the addon last wrote, as it saw the clock.
    pub written_at: Option<DateTime<Utc>>,
}

impl Collected {
    /// Rebuild the achievement list a run is planned from.
    ///
    /// This is what lets [`crate::plan::plan`] run with no web API. The
    /// tree is one level deep — a root with a child per criterion — which is
    /// the shape the evaluator wants and all the game gives.
    pub fn progress(&self) -> Vec<AchievementProgress> {
        let mut ids: Vec<u32> = self
            .tree
            .keys()
            .chain(self.completed.keys())
            .copied()
            .collect();
        ids.sort_unstable();
        ids.dedup();

        ids.into_iter()
            .map(|id| AchievementProgress {
                id,
                completed_at: self.completed.get(&id).copied(),
                criteria: self.tree.get(&id).map(|children| Criterion {
                    id: u64::from(id),
                    kind: CriterionKind::Unknown,
                    // Zero means "all of them", which is what an achievement
                    // wants of its criteria unless it says otherwise. The game
                    // does not expose the "any N of these" threshold, so
                    // demanding all is the conservative reading — it can leave
                    // a goal open that is really done, never the reverse.
                    required: 0,
                    children: children
                        .iter()
                        .map(|criterion| {
                            Criterion::leaf(
                                *criterion,
                                self.criteria
                                    .get(criterion)
                                    .copied()
                                    .unwrap_or(CriterionKind::Unknown),
                                1,
                            )
                        })
                        .collect(),
                }),
            })
            .collect()
    }
}

/// One character, as its own file describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct CollectedCharacter {
    pub character: Character,
    pub detail: Detail,
    pub quests: HashSet<u32>,
}

impl CollectedCharacter {
    /// What this character's own data can answer.
    pub fn primary(&self) -> PrimaryData {
        PrimaryData {
            quests: self.quests.clone(),
            ..PrimaryData::default()
        }
    }
}

/// Why a collector file could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// The file is not Lua we will read.
    Unparsable(String),
    /// Parsed, but it is not the collector's file.
    NotCollectorData,
    /// Written by an addon newer than this application understands.
    ///
    /// Distinct from unparsable on purpose: the fix is to update Armory, and
    /// saying "unreadable" would send someone reinstalling the addon that is
    /// working correctly.
    FromTheFuture { format: u32 },
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Unparsable(detail) => write!(f, "{detail}"),
            ReadError::NotCollectorData => {
                write!(f, "this file was not written by the Armory collector")
            }
            ReadError::FromTheFuture { format } => write!(
                f,
                "the collector addon is writing format {format} and this version of \
                 Armory reads {FORMAT} — update Armory"
            ),
        }
    }
}

/// Read the account-wide collector file.
pub fn read(source: &str) -> Result<Collected, ReadError> {
    let globals = lua::parse(source).map_err(|error| ReadError::Unparsable(error.to_string()))?;
    let Some(db) = globals.get(GLOBAL) else {
        return Err(ReadError::NotCollectorData);
    };

    let format = db.get("format").and_then(Value::as_u32).unwrap_or(0);
    if format > FORMAT {
        return Err(ReadError::FromTheFuture { format });
    }

    let mut collected = Collected {
        written_at: db.get("writtenAt").and_then(Value::as_f64).and_then(epoch),
        ..Collected::default()
    };

    if let Some(table) = db.get("achievements") {
        for (id, who) in table.entries() {
            let (Some(id), Some(who)) = (id.as_u32(), who.as_str()) else {
                continue;
            };
            if let Some(key) = parse_character(who) {
                collected.earned_by.insert(id, key);
            }
        }
    }

    if let Some(table) = db.get("completed") {
        for (id, at) in table.entries() {
            let Some(id) = id.as_u32() else { continue };
            // `true` rather than a date means the game gave no date. The
            // achievement is still complete, and standing needs *a* time — the
            // epoch is the safe one, because it is before any baseline and so
            // reads as "earned long ago", which is what it means.
            let at = match at {
                Value::Number(seconds) => epoch(*seconds),
                Value::Bool(true) => Some(DateTime::UNIX_EPOCH),
                _ => None,
            };
            if let Some(at) = at {
                collected.completed.insert(id, at);
            }
        }
    }

    if let Some(table) = db.get("tree") {
        for (id, children) in table.entries() {
            let Some(id) = id.as_u32() else { continue };
            let ids: Vec<u64> = children
                .items()
                .iter()
                .filter_map(|child| child.as_f64().map(|id| id as u64))
                .collect();
            if !ids.is_empty() {
                collected.tree.insert(id, ids);
            }
        }
    }

    if let Some(table) = db.get("criteria") {
        for (id, pair) in table.entries() {
            // `{criteriaType, assetID}`, a positional pair. A row that is not
            // that shape is skipped rather than half-read: a criterion mapped
            // to the wrong asset draws a confident bar over the wrong number,
            // which is worse than no mapping at all.
            let (Some(id), [kind, asset]) = (id.as_str().parse::<u64>().ok(), pair.items()) else {
                continue;
            };
            let (Some(kind), Some(asset)) = (kind.as_u32(), asset.as_u32()) else {
                continue;
            };
            collected
                .criteria
                .insert(id, CriterionKind::from_catalogue(kind, asset));
        }
    }

    if let Some(table) = db.get("names") {
        for (id, row) in table.entries() {
            let Some(id) = id.as_u32() else { continue };
            let row = row.items();
            let Some(name) = row.first().and_then(Value::as_str) else {
                continue;
            };
            let category = row.get(2).and_then(Value::as_str).unwrap_or_default();

            collected.catalogue.insert(
                id,
                Achievement {
                    id,
                    name: strip_markup(name),
                    category: category.to_string(),
                    points: row.get(1).and_then(Value::as_u32).unwrap_or(0),
                    description: row
                        .get(3)
                        .and_then(Value::as_str)
                        .map(strip_markup)
                        .unwrap_or_default(),
                    // A Feat of Strength cannot be earned twice by anybody, so
                    // it leaves a run rather than sitting in it as a permanent
                    // zero. The category is the only thing that says so.
                    is_unrepeatable: category.contains("Feats of Strength")
                        || category.contains("Legacy"),
                },
            );
        }
    }

    if let Some(table) = db.get("currencies") {
        for (who, amounts) in table.entries() {
            let Some(key) = parse_character(who.as_str()) else {
                continue;
            };
            let per_character = amounts
                .entries()
                .filter_map(|(id, amount)| {
                    Some((id.as_u32()?, amount.as_f64().unwrap_or(0.0) as u64))
                })
                .collect();
            collected.currencies.insert(key, per_character);
        }
    }

    if let Some(table) = db.get("earned") {
        for (who, mine) in table.entries() {
            let Some(key) = parse_character(who.as_str()) else {
                continue;
            };
            let mut earned = Earned::default();

            if let Some(factions) = mine.get("rep") {
                for (id, row) in factions.entries() {
                    // `{ points, renownEarned, renownSeen, accountWide }`.
                    let (Some(id), [points, renown, seen, wide, ..]) = (id.as_u32(), row.items())
                    else {
                        continue;
                    };
                    earned.reputation.insert(
                        id,
                        EarnedReputation {
                            points: points.as_u32().unwrap_or(0),
                            renown: renown.as_u32().unwrap_or(0),
                            renown_seen: seen.as_u32().unwrap_or(0),
                            account_wide: wide.as_u32().unwrap_or(0) == 1,
                        },
                    );
                }
            }

            if let Some(currencies) = mine.get("currency") {
                for (id, row) in currencies.entries() {
                    // `{ gained, earned, accountWide, transferable, tracksEarned }`.
                    let (Some(id), [gained, counted, wide, transferable, tracks, ..]) =
                        (id.as_u32(), row.items())
                    else {
                        continue;
                    };
                    earned.currency.insert(
                        id,
                        EarnedCurrency {
                            gained: gained.as_f64().unwrap_or(0.0) as u64,
                            earned: counted.as_f64().unwrap_or(0.0) as u64,
                            // Read rather than inferred. The game returns a
                            // flat zero for `totalEarned` on currencies it does
                            // not track, and reading that as "earned nothing"
                            // would report every transferable currency as a
                            // transfer.
                            tracks_earned: tracks.as_u32().unwrap_or(0) == 1,
                            account_wide: wide.as_u32().unwrap_or(0) == 1,
                            transferable: transferable.as_u32().unwrap_or(0) == 1,
                        },
                    );
                }
            }

            collected.earned.insert(key, earned);
        }
    }

    // `recipes[character][recipeID] = { name, output, makes, { { qty, tiers } } }`.
    if let Some(table) = db.get("recipes") {
        for (who, mine) in table.entries() {
            let Some(character) = parse_character(who.as_str()) else {
                continue;
            };
            let book: Vec<Recipe> = mine
                .entries()
                .filter_map(|(id, row)| {
                    let (Some(id), [name, output, makes, reagents, ..]) =
                        (id.as_u32(), row.items())
                    else {
                        return None;
                    };
                    Some(Recipe {
                        id,
                        name: name.as_str()?.to_string(),
                        output: output.as_u32()?,
                        // A recipe that makes none of something is not a
                        // recipe; one is the floor and the addon's default.
                        makes: makes.as_u32().unwrap_or(1).max(1),
                        reagents: reagents
                            .items()
                            .iter()
                            .filter_map(|slot| {
                                let [quantity, tiers, ..] = slot.items() else {
                                    return None;
                                };
                                let tiers: Vec<u32> =
                                    tiers.items().iter().filter_map(Value::as_u32).collect();
                                (!tiers.is_empty()).then(|| Reagent {
                                    quantity: quantity.as_u32().unwrap_or(1).max(1),
                                    tiers,
                                })
                            })
                            .collect(),
                    })
                })
                .filter(|recipe| !recipe.reagents.is_empty())
                .collect();
            if !book.is_empty() {
                collected.recipes.insert(character, book);
            }
        }
    }

    // `tally[character][kind][key] = { count, label }`. One table for every
    // counter rather than one per kind — see `model::tally`.
    if let Some(table) = db.get("tally") {
        for (who, mine) in table.entries() {
            let Some(character) = parse_character(who.as_str()) else {
                continue;
            };
            let mut counted = Vec::new();
            for (token, rows) in mine.entries() {
                // A kind this version does not know is skipped rather than
                // filed under a plausible one. `FORMAT` is what reports it.
                let Some(kind) = Counting::from_token(token.as_str()) else {
                    continue;
                };
                for (key, row) in rows.entries() {
                    let [count, label, ..] = row.items() else {
                        continue;
                    };
                    // Keyed by a spell id as often as by a name, and either
                    // reads back as the text it was written as.
                    let Some(label) = label.as_str() else {
                        continue;
                    };
                    counted.push(Tally {
                        kind,
                        key: key.as_str().to_string(),
                        label: label.to_string(),
                        count: count.as_f64().unwrap_or(0.0) as u64,
                    });
                }
            }
            if !counted.is_empty() {
                collected.tallies.insert(character, counted);
            }
        }
    }

    if let Some(table) = db.get("warbandBank") {
        collected.warband_bank = table
            .entries()
            .filter_map(|(id, count)| Some((id.as_u32()?, count.as_f64().unwrap_or(0.0) as u64)))
            .collect();
    }

    for (key, kind) in [
        ("mounts", Kind::Mount),
        ("pets", Kind::Pet),
        ("toys", Kind::Toy),
    ] {
        let Some(table) = db.get(key) else { continue };

        // Both halves of the table, because WoW's serializer uses both. A
        // collection keyed by mount id comes out positional while the ids stay
        // dense — padded with `nil` for the holes — and switches to keyed
        // entries once they are not. Reading only the hash part loses the first
        // few hundred entries, and reading only the array part loses the rest.
        let positional = table
            .items()
            .iter()
            .enumerate()
            // Lua arrays are one-based, so the index *is* the id.
            .map(|(index, entry)| (index as u32 + 1, entry));
        let keyed = table
            .entries()
            .filter_map(|(id, entry)| Some((id.as_u32()?, entry)));

        for (id, entry) in positional.chain(keyed) {
            let row = entry.items();
            let [name, owned, source, ..] = row else {
                // `nil` padding, and anything else that is not a row.
                continue;
            };
            let Some(name) = name.as_str() else { continue };

            if owned.as_u32().unwrap_or(0) == 1 {
                collected.owned.insert((kind, id));
            }
            let source_text = strip_markup(source.as_str().unwrap_or_default());
            let flavour = row
                .get(5)
                .and_then(Value::as_str)
                .map(strip_markup)
                .filter(|text| !text.is_empty());

            collected.collectibles.push(Collectible {
                kind,
                id,
                name: name.to_string(),
                // A sentence rather than the web API's one word. This is the
                // whole reason the addon is the better source for collections:
                // "Vendor: Unger Statforth, Wetlands" answers the question, and
                // `VENDOR` does not.
                source: Source::from_text(&source_text),
                description: (!source_text.is_empty()).then_some(source_text),
                flavour,
                icon: row.get(6).and_then(Value::as_u32).filter(|icon| *icon > 0),
                display: row.get(7).and_then(Value::as_u32).filter(|id| *id > 0),
                // `-1` is the addon saying "anyone", which is not a faction.
                faction: match row.get(9).and_then(Value::as_f64) {
                    Some(0.0) => Some(Faction::Horde),
                    Some(1.0) => Some(Faction::Alliance),
                    _ => None,
                },
                // The spell for a mount, the creature for a pet, the item for a
                // toy — whichever Wowhead indexes this kind under. Falling back
                // to the collection id gives a link that lands somewhere wrong
                // rather than no link at all.
                link_id: row
                    .get(4)
                    .and_then(Value::as_u32)
                    .filter(|link| *link > 0)
                    .unwrap_or(id),
                // Pets only, and only from collectors new enough to write it.
                // An older file has no tenth column at all, which is silence
                // rather than "not tradeable" — see the field's own note.
                tradeable: (kind == Kind::Pet)
                    .then(|| row.get(10).and_then(Value::as_u32))
                    .flatten()
                    .map(|flag| flag == 1),
            });

            // How many of this species are in the journal. Account state rather
            // than a property of the pet, so it is kept beside the owned set and
            // not on the catalogue row.
            if kind == Kind::Pet {
                if let Some(held) = row.get(11).and_then(Value::as_u32) {
                    collected.pets_held.insert(id, held);
                }
            }
        }
    }

    Ok(collected)
}

/// Read one per-character collector file.
pub fn read_character(source: &str) -> Result<CollectedCharacter, ReadError> {
    let globals = lua::parse(source).map_err(|error| ReadError::Unparsable(error.to_string()))?;
    let Some(db) = globals.get(CHARACTER_GLOBAL) else {
        return Err(ReadError::NotCollectorData);
    };

    let format = db.get("format").and_then(Value::as_u32).unwrap_or(0);
    if format > FORMAT {
        return Err(ReadError::FromTheFuture { format });
    }

    let (Some(name), Some(realm)) = (
        db.get("name").and_then(Value::as_str),
        db.get("realm").and_then(Value::as_str),
    ) else {
        return Err(ReadError::NotCollectorData);
    };

    let character = Character {
        key: CharacterKey::new(realm_slug(realm), name),
        // The game does not know its own numeric ids, so these stay zero. Only
        // the protected endpoint wants them, and that is a web API call this
        // path is deliberately doing without.
        id: 0,
        realm_id: 0,
        display_name: name.to_string(),
        realm_name: realm.to_string(),
        level: db.get("level").and_then(Value::as_u32).unwrap_or(0) as u8,
        class: titlecase(db.get("class").and_then(Value::as_str).unwrap_or_default()),
        race: titlecase(db.get("race").and_then(Value::as_str).unwrap_or_default()),
        faction: match db.get("faction").and_then(Value::as_str) {
            Some("Alliance") => Faction::Alliance,
            Some("Horde") => Faction::Horde,
            _ => Faction::Neutral,
        },
        wow_account_id: 0,
    };

    let detail = Detail {
        item_level: db
            .get("itemLevel")
            .and_then(Value::as_f64)
            .map(|level| level as u16),
        equipped_item_level: None,
        spec: db.get("spec").and_then(Value::as_str).map(str::to_string),
        guild: db.get("guild").and_then(Value::as_str).map(str::to_string),
        money: db.get("money").and_then(Value::as_f64).map(|m| m as u64),
        achievement_points: None,
        last_login: db.get("scannedAt").and_then(Value::as_f64).and_then(epoch),
        professions: db
            .get("professions")
            .map(|list| {
                list.items()
                    .iter()
                    .filter_map(|entry| {
                        let [name, rank, max, primary, rest @ ..] = entry.items() else {
                            return None;
                        };
                        // The last two are newer than the first four, and an
                        // older addon writes a four-element row. Absent is
                        // silence rather than "no specialisations".
                        let (trees, learned) = match rest {
                            [trees, learned, ..] => (Some(trees), learned.as_u32()),
                            _ => (None, None),
                        };
                        Some(Profession {
                            name: name.as_str()?.to_string(),
                            tier: None,
                            skill: rank.as_u32().map(|r| r as u16),
                            max_skill: max.as_u32().map(|m| m as u16),
                            is_primary: primary.as_u32().unwrap_or(0) == 1,
                            specialisations: trees
                                .map(|trees| {
                                    trees
                                        .items()
                                        .iter()
                                        .filter_map(|tree| {
                                            let [name, open, ..] = tree.items() else {
                                                return None;
                                            };
                                            Some((
                                                name.as_str()?.to_string(),
                                                open.as_u32().unwrap_or(0) == 1,
                                            ))
                                        })
                                        .collect()
                                })
                                .unwrap_or_default(),
                            knowledge: learned.unwrap_or(0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        mythic_rating: None,
        renown: None,
        equipment: read_equipment(db),
        // Lifetime raid progress is the web API's answer and the client cannot
        // give it. What the client knows is the current lockout, which is the
        // field beside this one.
        raids: None,
        raid_locks: read_raid_locks(db),
    };

    Ok(CollectedCharacter {
        character,
        detail,
        quests: db
            .get("quests")
            .map(|list| {
                list.items()
                    .iter()
                    .filter_map(|id| id.as_f64().map(|id| id as u32))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// Strip WoW's UI markup out of a string.
///
/// The journals' source text is written for a tooltip, not for a database:
/// `|cFFFFD200Vendor: |rUnger Statforth|n|cFFFFD200Zone: |rWetlands` is one
/// mount's provenance with colour codes and line breaks woven through it.
/// Left in, the escape codes reach a GTK label as literal noise and the
/// leading-clause match in `Source::from_text` sees `cFFFFD200Vendor` rather
/// than `Vendor`.
///
/// The sequences, all introduced by a pipe: `cAARRGGBB` opens a colour and `r`
/// closes it, `n` is a newline, `T…|t` is an inline texture, `H…|h text |h` is
/// a hyperlink whose text is worth keeping, and `||` is a literal pipe.
fn strip_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();

    while let Some(character) = characters.next() {
        if character != '|' {
            out.push(character);
            continue;
        }
        match characters.next() {
            // A colour opens with eight hex digits that are not content.
            Some('c') => {
                for _ in 0..8 {
                    characters.next_if(|c| c.is_ascii_hexdigit());
                }
            }
            Some('r') => {}
            Some('n') => out.push('\n'),
            // A texture runs to its closing `|t` and has no text in it.
            Some('T') => {
                while let Some(c) = characters.next() {
                    if c == '|' && characters.next_if_eq(&'t').is_some() {
                        break;
                    }
                }
            }
            // A hyperlink's payload is `|Hlink|htext|h`: skip to the first
            // `|h`, keep what follows, stop at the second.
            Some('H') => {
                while let Some(c) = characters.next() {
                    if c == '|' && characters.next_if_eq(&'h').is_some() {
                        break;
                    }
                }
            }
            Some('h') => {}
            Some('|') => out.push('|'),
            // An escape we do not know: drop the pipe, keep the character, so
            // a future addition degrades to slightly odd text rather than to
            // markup on screen.
            Some(other) => out.push(other),
            None => break,
        }
    }

    out.trim().to_string()
}

/// What the character was wearing when the addon last looked.
///
/// The same rows the web API's `/equipment` produces, deliberately: the addon
/// writes Blizzard's own slot names, so a character known only through the
/// addon and one known through the API draw the same list and nothing
/// downstream has to ask which it got.
///
/// Absent is silence. A collector file written before this was recorded has no
/// `equipment` key at all, and answering `Some(vec![])` for it would say "this
/// character is naked" — which is a real state and not this one. The same rule
/// as the pets' two extra columns.
///
/// The item level is `""` for a cosmetic slot and that is read back as `None`,
/// never as zero: the character page sorts on this and a nought sorts a tabard
/// above a genuinely weak slot.
fn read_equipment(db: &Value) -> Option<Vec<Equipped>> {
    let worn: Vec<Equipped> = db
        .get("equipment")?
        .items()
        .iter()
        .filter_map(|entry| {
            let [slot, name, level, ..] = entry.items() else {
                return None;
            };
            let slot = slot.as_str()?.to_string();
            let slot_name = Equipped::SLOTS
                .iter()
                .chain(&[("SHIRT", "Shirt"), ("TABARD", "Tabard")])
                .find(|(key, _)| *key == slot)
                .map(|(_, name)| (*name).to_string())
                .unwrap_or_else(|| slot.clone());
            Some(Equipped {
                slot,
                slot_name,
                name: name.as_str()?.to_string(),
                level: level.as_u32().filter(|level| *level > 0).map(|l| l as u16),
            })
        })
        .collect();
    Some(worn)
}

/// Which raids this character is saved to for the current reset.
///
/// Absent is silence, present-and-empty is "saved to nothing", and those are
/// different: a character who has not raided this week and a collector file
/// written before lockouts were recorded must not read the same.
fn read_raid_locks(db: &Value) -> Option<Vec<RaidLock>> {
    let locks = db
        .get("raidLocks")?
        .items()
        .iter()
        .filter_map(|entry| {
            let [name, difficulty, defeated, total, ..] = entry.items() else {
                return None;
            };
            Some(RaidLock {
                name: name.as_str()?.to_string(),
                difficulty: difficulty.as_str()?.to_string(),
                defeated: defeated.as_u32().unwrap_or(0) as u16,
                total: total.as_u32().unwrap_or(0) as u16,
            })
        })
        .collect();
    Some(locks)
}

/// Seconds since the epoch, as the game counts them.
fn epoch(seconds: f64) -> Option<DateTime<Utc>> {
    Utc.timestamp_opt(seconds as i64, 0).single()
}

/// `WARRIOR` is how the game spells a class token; `Warrior` is how a person
/// does.
fn titlecase(token: &str) -> String {
    let mut characters = token.chars();
    match characters.next() {
        Some(first) => {
            first.to_uppercase().collect::<String>() + &characters.as_str().to_lowercase()
        }
        None => String::new(),
    }
}

/// Split the addon's `Name-Realm` spelling into a key.
///
/// The game writes the realm's display name, and every endpoint wants the slug,
/// so the conversion happens here rather than at each use.
fn parse_character(text: &str) -> Option<CharacterKey> {
    let (name, realm) = text.split_once('-')?;
    if name.is_empty() || realm.is_empty() {
        return None;
    }
    Some(CharacterKey::new(realm_slug(realm), name))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
ArmoryCollectorDB = {
	["format"] = 2,
	["writtenAt"] = 1785000000,
	["achievements"] = {
		[4956] = "Aeltor-Mannoroth",
		[1234] = "Somechar-Emerald Dream",
	},
	["completed"] = {
		[4956] = 1457000000,
		[1234] = true,
	},
	["tree"] = {
		[4956] = { 12345, 12346 },
	},
	["criteria"] = {
		[12345] = { 27, 5000 },
		[12346] = { 46, 2170 },
		[12347] = { 119, 9 },
	},
	["names"] = {
		[4956] = { "Loremaster of Kalimdor", 50, "Quests", "Complete the Kalimdor quest achievements.", "", 236443 },
		[1234] = { "Gone Forever", 10, "Feats of Strength", "", "", 0 },
	},
	["currencies"] = {
		["Somechar-Emerald Dream"] = { [2245] = 4200 },
	},
	["warbandBank"] = { [190456] = 40 },
	["recipes"] = {
		["Somechar-Emerald Dream"] = {
			[371637] = {
				"Flask of Alchemical Chaos",
				191318,
				1,
				{
					{ 3, { 210796, 210797, 210798 } },
					{ 1, { 212263 } },
				},
			},
		},
	},
	["tally"] = {
		["Somechar-Emerald Dream"] = {
			["recipe"] = {
				[371637] = { 412, "Flask of Alchemical Chaos" },
				[370582] = { 6, "Algari Mana Potion" },
			},
			["companion"] = {
				["Velkurai"] = { 34, "Velkurai" },
			},
			["zone"] = {
				["Nagrand"] = { 68400, "Nagrand" },
			},
			["nonsense"] = {
				["x"] = { 1, "x" },
			},
		},
	},
	["earned"] = {
		["Somechar-Emerald Dream"] = {
			["rep"] = {
				[2170] = { 21000, 4, 25, 1 },
			},
			["currency"] = {
				[3008] = { 1000, 600, 1, 1, 1 },
				[2245] = { 4200, 0, 0, 0, 0 },
			},
		},
	},
	["mounts"] = {
		[6] = { "Brown Horse", 1, "Vendor: Katie Hunter, Elwynn Forest", 2 },
		[7] = { "Ashes of Al'ar", 0, "Drop: Kael'thas Sunstrider, Tempest Keep", 1 },
	},
	["toys"] = {
		[128471] = { "Sturdy Love Fool", 0, "", 0 },
	},
}
"#;

    const CHARACTER: &str = r#"
ArmoryCollectorCharDB = {
	["format"] = 2,
	["name"] = "Somechar",
	["realm"] = "Emerald Dream",
	["level"] = 80,
	["money"] = 91234567,
	["class"] = "DRUID",
	["race"] = "Tauren",
	["faction"] = "Horde",
	["spec"] = "Restoration",
	["guild"] = "Dream Team",
	["itemLevel"] = 642.4,
	["scannedAt"] = 1785000000,
	["quests"] = { 100, 200, 300 },
	["professions"] = {
		{ "Alchemy", 84, 100, 1, { { "Potion Mastery", 1 }, { "Phial Mastery", 0 } }, 412 },
		{ "Cooking", 40, 100, 0 },
	},
	["equipment"] = {
		{ "HEAD", "Helm of the Broken", 639 },
		{ "OFF_HAND", "Bulwark of the Kurenai", 612 },
		{ "TABARD", "Tabard of the Kurenai", "" },
	},
	["raidLocks"] = {
		{ "Liberation of Undermine", "Heroic", 2, 8, 15 },
	},
}
"#;

    #[test]
    fn attribution_is_still_what_the_addon_is_for() {
        let collected = read(SAMPLE).expect("read");
        assert_eq!(
            collected.earned_by.get(&4956),
            Some(&CharacterKey::new("mannoroth", "Aeltor"))
        );
    }

    #[test]
    fn the_addon_supplies_the_achievement_list_a_run_is_planned_from() {
        // This is what lets a run happen with no web API at all — which matters
        // because Blizzard's developer portal has been refusing to create
        // clients since late 2025.
        let progress = read(SAMPLE).expect("read").progress();
        assert_eq!(progress.len(), 2);

        let with_tree = progress
            .iter()
            .find(|entry| entry.id == 4956)
            .expect("4956");
        let criteria = with_tree.criteria.as_ref().expect("a tree");
        assert_eq!(criteria.children.len(), 2);
        // The meaning is joined on from the criteria map, which the web API
        // never supplies at all.
        assert_eq!(criteria.children[0].kind, CriterionKind::Quest(5000));
        assert_eq!(criteria.children[1].kind, CriterionKind::Reputation(2170));
    }

    #[test]
    fn the_addon_names_the_achievements_so_the_interface_need_not_say_a_number() {
        // Without this the run page reads "Achievement 4956", which is what it
        // did until the addon started keeping the name it was already reading.
        let collected = read(SAMPLE).expect("read");
        let loremaster = collected.catalogue.get(&4956).expect("4956");

        assert_eq!(loremaster.name, "Loremaster of Kalimdor");
        assert_eq!(loremaster.points, 50);
        assert_eq!(loremaster.category, "Quests");
        assert!(loremaster.description.starts_with("Complete the Kalimdor"));
        assert!(!loremaster.is_unrepeatable);
    }

    #[test]
    fn a_recipe_book_carries_every_quality_of_every_reagent() {
        let collected = read(SAMPLE).unwrap();
        let key = CharacterKey::new("emerald-dream", "Somechar");
        let book = &collected.recipes[&key];

        assert_eq!(book.len(), 1);
        assert_eq!(book[0].name, "Flask of Alchemical Chaos");
        assert_eq!(book[0].output, 191_318);
        assert_eq!(book[0].makes, 1);
        // Three quality tiers of one reagent, which is what makes the cheapest
        // one costable.
        assert_eq!(book[0].reagents[0].quantity, 3);
        assert_eq!(book[0].reagents[0].tiers, [210_796, 210_797, 210_798]);
        assert_eq!(book[0].reagents[1].tiers, [212_263]);
    }

    #[test]
    fn the_counters_nothing_else_keeps_come_off_one_table() {
        use crate::tally;

        let collected = read(SAMPLE).unwrap();
        let key = CharacterKey::new("emerald-dream", "Somechar");
        let mine = &collected.tallies[&key];

        let made = tally::of(mine, Counting::Recipe);
        assert_eq!(made[0].label, "Flask of Alchemical Chaos");
        assert_eq!(made[0].count, 412);
        // Keyed by a spell id, which Lua hands back as a number.
        assert_eq!(made[0].key, "371637");
        assert_eq!(made[1].count, 6);

        assert_eq!(tally::of(mine, Counting::Companion)[0].count, 34);
        assert_eq!(tally::of(mine, Counting::Zone)[0].label, "Nagrand");

        // A kind this version does not know contributed nothing rather than a
        // row filed under something plausible.
        assert_eq!(mine.len(), 4);
    }

    #[test]
    fn what_a_character_earned_is_read_apart_from_what_the_account_holds() {
        // The reputation half is what makes an inherited standing measurable
        // again; the currency half is what tells earned from transferred. No
        // endpoint has either.
        use crate::provenance::Origin;

        let collected = read(SAMPLE).expect("read");
        let key = CharacterKey::new("emerald-dream", "Somechar");
        let earned = &collected.earned[&key];

        let with = earned.with(2170);
        assert_eq!(with.points, 21_000);
        assert_eq!(with.renown, 4);
        // What the account showed them, kept apart from what they earned.
        assert_eq!(with.renown_seen, 25);
        assert!(with.account_wide);

        // Transferable, with an earned total that does not cover the gain.
        assert_eq!(earned.currency[&3008].origin(), Origin::Transferred);
        // Not transferable at all, so whatever arrived was earned here.
        assert_eq!(earned.currency[&2245].origin(), Origin::Earned);
    }

    #[test]
    fn a_feat_of_strength_is_recognised_by_its_category() {
        // It can never be earned again by anybody, so it leaves a run rather
        // than sitting in it as a permanent zero. The category is the only
        // thing that says so.
        let collected = read(SAMPLE).expect("read");
        assert!(collected.catalogue[&1234].is_unrepeatable);
    }

    #[test]
    fn an_achievement_with_no_date_still_counts_as_earned_long_ago() {
        // `true` means the game gave no date. It is still complete, and
        // standing needs a time — the epoch is before any baseline, which is
        // exactly what "earned long ago" should mean.
        let collected = read(SAMPLE).expect("read");
        assert_eq!(collected.completed.get(&1234), Some(&DateTime::UNIX_EPOCH));
        assert!(collected.completed[&4956] > DateTime::UNIX_EPOCH);
    }

    #[test]
    fn collections_carry_a_sentence_rather_than_one_word() {
        // The whole reason the addon beats the web API here. `/data/wow/mount`
        // says "DROP"; the journal says which boss, in which raid.
        let collected = read(SAMPLE).expect("read");
        let ashes = collected
            .collectibles
            .iter()
            .find(|entry| entry.id == 7)
            .expect("Ashes of Al'ar");

        assert_eq!(ashes.name, "Ashes of Al'ar");
        assert_eq!(ashes.source, Source::Drop);
        assert!(!collected.owned.contains(&(Kind::Mount, 7)));
        assert!(collected.owned.contains(&(Kind::Mount, 6)));
    }

    #[test]
    fn the_journals_markup_is_stripped_to_the_sentence_underneath() {
        // Exactly what the game wrote for this mount, colour codes and all.
        // Left in, the escape codes reach a GTK label as literal noise and the
        // leading-clause match sees `cFFFFD200Drop` rather than `Drop`.
        assert_eq!(
            strip_markup(
                "|cFFFFD200Drop:|r Lord Aurius Rivendare|n|cFFFFD200Location:|r Stratholme"
            ),
            "Drop: Lord Aurius Rivendare\nLocation: Stratholme"
        );
        assert_eq!(strip_markup("|cFFFFD200Legacy|r"), "Legacy");
    }

    #[test]
    fn an_inline_texture_leaves_nothing_behind() {
        // Vendor costs carry a gold-coin texture mid-sentence.
        assert_eq!(
            strip_markup(
                "|cFFFFD200Vendor: |rHarb Clawhoof|n|cFFFFD200Cost: |r1|TINTERFACE\\MONEYFRAME\\UI-GOLDICON.BLP:0|t"
            ),
            "Vendor: Harb Clawhoof\nCost: 1"
        );
    }

    #[test]
    fn a_hyperlink_keeps_its_text_and_drops_its_payload() {
        assert_eq!(
            strip_markup("Quest: |Hquest:12345|hThe Battle for Gilneas|h"),
            "Quest: The Battle for Gilneas"
        );
    }

    #[test]
    fn a_literal_pipe_survives() {
        assert_eq!(strip_markup("a || b"), "a | b");
    }

    #[test]
    fn a_collection_written_as_a_sparse_array_reads_at_the_right_ids() {
        // What the game actually emits: positional entries padded with `nil`
        // while the ids are dense, then keyed once they are not. Reading only
        // one half loses most of the collection.
        let collected = read(
            r#"ArmoryCollectorDB = { ["format"] = 2, ["mounts"] = {
                nil, nil,
                { "Third Mount", 1, "|cFFFFD200Vendor:|r Someone" },
                [382] = { "Far Mount", 0, "" },
            } }"#,
        )
        .expect("read");

        assert_eq!(collected.collectibles.len(), 2);
        // Lua arrays are one-based, so the third slot is id 3.
        let third = collected
            .collectibles
            .iter()
            .find(|entry| entry.id == 3)
            .expect("the positional one");
        assert_eq!(third.name, "Third Mount");
        assert_eq!(third.description.as_deref(), Some("Vendor: Someone"));
        assert!(collected.owned.contains(&(Kind::Mount, 3)));

        assert!(collected
            .collectibles
            .iter()
            .any(|entry| entry.id == 382 && entry.name == "Far Mount"));
    }

    #[test]
    fn a_source_with_no_text_is_unknown_rather_than_guessed() {
        let collected = read(SAMPLE).expect("read");
        let toy = collected
            .collectibles
            .iter()
            .find(|entry| entry.kind == Kind::Toy)
            .expect("a toy");
        assert_eq!(toy.source, Source::Unknown);
    }

    #[test]
    fn a_character_file_is_a_whole_roster_row() {
        // No web API involved: every character you log in on describes itself.
        let read = read_character(CHARACTER).expect("read");
        assert_eq!(read.character.display_name, "Somechar");
        assert_eq!(read.character.key.realm_slug, "emerald-dream");
        assert_eq!(read.character.level, 80);
        // `DRUID` is how the game spells it; `Druid` is how a person does.
        assert_eq!(read.character.class, "Druid");
        assert_eq!(read.character.faction, Faction::Horde);
        assert_eq!(read.detail.item_level, Some(642));
        assert_eq!(read.detail.spec.as_deref(), Some("Restoration"));
        assert_eq!(read.detail.money, Some(91_234_567));
        assert_eq!(read.quests.len(), 3);
        assert_eq!(read.detail.professions.len(), 2);
        assert!(read.detail.professions[0].is_primary);
        assert!(!read.detail.professions[1].is_primary);

        // A progression system with no endpoint behind it at all.
        let alchemy = &read.detail.professions[0];
        assert_eq!(alchemy.knowledge, 412);
        assert_eq!(
            alchemy.specialisations,
            [
                ("Potion Mastery".to_string(), true),
                ("Phial Mastery".to_string(), false)
            ]
        );
        // An older addon writes four columns, and that is silence rather than
        // a character who has opened nothing.
        assert!(read.detail.professions[1].specialisations.is_empty());
    }

    #[test]
    fn quests_from_the_character_file_are_what_a_poisoned_goal_is_measured_against() {
        let read = read_character(CHARACTER).expect("read");
        assert!(read.primary().quests.contains(&200));
    }

    #[test]
    fn someone_elses_saved_variables_are_recognised_as_not_ours() {
        let error = read(r#"TradeSkillMasterDB = { ["x"] = 1 }"#).expect_err("not ours");
        assert_eq!(error, ReadError::NotCollectorData);
        assert_eq!(
            read_character(r#"TradeSkillMasterDB = { ["x"] = 1 }"#).expect_err("not ours"),
            ReadError::NotCollectorData
        );
    }

    #[test]
    fn a_newer_addon_says_so_rather_than_reading_as_broken() {
        let error = read(r#"ArmoryCollectorDB = { ["format"] = 99 }"#).expect_err("too new");
        assert_eq!(error, ReadError::FromTheFuture { format: 99 });
        assert!(error.to_string().contains("update Armory"));
    }

    #[test]
    fn a_truncated_file_is_reported_rather_than_half_read() {
        // WoW truncates SavedVariables on a hard exit. Half a file must not
        // become half a roster's worth of attribution.
        let error =
            read(r#"ArmoryCollectorDB = { ["achievements"] = { [1] = "#).expect_err("truncated");
        assert!(matches!(error, ReadError::Unparsable(_)), "{error:?}");
    }

    #[test]
    fn a_character_file_with_no_name_is_not_a_character() {
        assert_eq!(
            read_character(r#"ArmoryCollectorCharDB = { ["format"] = 2 }"#).expect_err("no name"),
            ReadError::NotCollectorData
        );
    }

    #[test]
    fn an_empty_dump_is_a_valid_dump() {
        let collected = read(r#"ArmoryCollectorDB = { ["format"] = 2 }"#).expect("read");
        assert_eq!(collected, Collected::default());
    }

    #[test]
    fn the_addon_answers_what_a_character_is_wearing() {
        // The whole point of reading this off the client: an account with no
        // Battle.net client still gets a character page. The rows come back in
        // the web API's own slot names so that neither the page nor the store
        // has to know which source it got.
        let read = read_character(CHARACTER).expect("read");
        let worn = read.detail.equipment.expect("equipment");
        assert_eq!(worn.len(), 3);

        let off_hand = worn
            .iter()
            .find(|item| item.slot == "OFF_HAND")
            .expect("off hand");
        assert_eq!(off_hand.slot_name, "Off Hand");
        assert_eq!(off_hand.level, Some(612));

        // A tabard is worn and is not gear. `""` rather than a number, read
        // back as nothing rather than as nought — the character page sorts on
        // this and a nought would sort the tabard above the weakest real slot,
        // which is the one fact that list exists to show.
        let tabard = worn
            .iter()
            .find(|item| item.slot == "TABARD")
            .expect("tabard");
        assert_eq!(tabard.level, None);
        assert!(tabard.is_cosmetic());
    }

    #[test]
    fn the_addon_answers_this_weeks_lockouts_and_not_a_lifetime() {
        let read = read_character(CHARACTER).expect("read");
        let locks = read.detail.raid_locks.expect("lockouts");
        assert_eq!(locks.len(), 1);
        assert_eq!(locks[0].defeated, 2);
        assert_eq!(locks[0].total, 8);
        // What the client cannot know, and does not claim to.
        assert_eq!(read.detail.raids, None);
    }

    #[test]
    fn a_collector_file_from_before_the_gear_scan_is_silence_not_a_naked_character() {
        // The same rule as the pets' two extra columns. `Some(vec![])` here
        // would say "this character is wearing nothing", which is a real state
        // and not this one.
        let older = r#"
ArmoryCollectorCharDB = {
	["format"] = 2,
	["name"] = "Somechar",
	["realm"] = "Emerald Dream",
	["class"] = "DRUID",
}
"#;
        let read = read_character(older).expect("read");
        assert_eq!(read.detail.equipment, None);
        assert_eq!(read.detail.raid_locks, None);
    }
}
