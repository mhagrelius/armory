//! Reading what the chronicle addon wrote.
//!
//! One global, `ArmoryChronicleDB`, per character. The shape is fixed by
//! `Chronicle.lua` in this repository, so this can be narrow and explicit
//! rather than defensive about a schema somebody else owns — the same call
//! [`super::collector`] makes.
//!
//! Every event row is the same five positions, `{ at, kind, a, b, c }`, with
//! absent fields written as empty strings rather than left nil. That is not a
//! tidiness choice in the addon and it is not one here either: WoW's serializer
//! writes a table with a hole in the middle as keyed entries rather than as a
//! padded array, so a row with an interior nil comes back in a different shape
//! from one without. Dense rows mean one shape to read.

use chrono::{DateTime, TimeZone, Utc};

use super::lua::{self, Value};
use crate::character::{CharacterKey, Faction};
use crate::chronicle::{Acquisition, Happening, Moment, Purpose, Session};
use crate::source::blizzard::realm_slug;

/// The per-character global the addon declares.
const GLOBAL: &str = "ArmoryChronicleDB";

/// The format this reader understands.
pub const FORMAT: u32 = 3;

/// Why a chronicle file could not be used.
///
/// Deliberately the same three cases as the collector's, and for the same
/// reasons — "this is not our file" and "this is from a newer addon" send
/// somebody to two different fixes, and neither of them is "reinstall the
/// addon that is working correctly".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    Unparsable(String),
    NotChronicleData,
    FromTheFuture { format: u32 },
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Unparsable(detail) => write!(f, "{detail}"),
            ReadError::NotChronicleData => {
                write!(f, "this file was not written by the Armory chronicle")
            }
            ReadError::FromTheFuture { format } => write!(
                f,
                "the chronicle addon is writing format {format} and this version of \
                 Armory reads {FORMAT} — update Armory"
            ),
        }
    }
}

/// Read one character's chronicle file into sessions.
///
/// A session that will not read is skipped rather than failing the file. One
/// unreadable evening is a much better outcome than losing the other thirty-
/// nine, and the game will not be rewriting the old ones.
pub fn read(source: &str) -> Result<Vec<Session>, ReadError> {
    let globals = lua::parse(source).map_err(|error| ReadError::Unparsable(error.to_string()))?;
    let Some(db) = globals.get(GLOBAL) else {
        return Err(ReadError::NotChronicleData);
    };

    let format = db.get("format").and_then(Value::as_u32).unwrap_or(0);
    if format > FORMAT {
        return Err(ReadError::FromTheFuture { format });
    }

    let Some(list) = db.get("sessions") else {
        return Ok(Vec::new());
    };

    Ok(list.items().iter().filter_map(session).collect())
}

fn session(entry: &Value) -> Option<Session> {
    let name = entry.get("name").and_then(Value::as_str)?;
    let realm = entry.get("realm").and_then(Value::as_str)?;
    let started_at = entry
        .get("startedAt")
        .and_then(Value::as_f64)
        .and_then(epoch)?;
    // A session with no end was interrupted — a crash, or a client killed
    // rather than logged out. The events in it are still real, so it is closed
    // at the last one rather than thrown away.
    let ended_at = entry
        .get("endedAt")
        .and_then(Value::as_f64)
        .and_then(epoch)
        .unwrap_or(started_at);

    let moments: Vec<Moment> = entry
        .get("events")
        .map(|events| events.items().iter().filter_map(moment).collect())
        .unwrap_or_default();

    let ended_at = ended_at.max(
        moments
            .last()
            .map(|last| started_at + chrono::Duration::seconds(i64::from(last.at)))
            .unwrap_or(started_at),
    );

    Some(Session {
        character: CharacterKey::new(realm_slug(realm), name),
        display_name: name.to_string(),
        realm_name: realm.to_string(),
        class: titlecase(
            entry
                .get("class")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ),
        race: titlecase(
            entry
                .get("race")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ),
        faction: match entry.get("faction").and_then(Value::as_str) {
            Some("Alliance") => Faction::Alliance,
            Some("Horde") => Faction::Horde,
            _ => Faction::Neutral,
        },
        started_at,
        ended_at,
        start_level: number(entry, "startLevel") as u8,
        end_level: number(entry, "endLevel") as u8,
        start_money: number(entry, "startMoney"),
        end_money: number(entry, "endMoney"),
        start_item_level: number(entry, "startItemLevel") as u16,
        end_item_level: number(entry, "endItemLevel") as u16,
        moments,
        // Session totals rather than moments, and written at logout — which is
        // also why they are fields and not events: by then the event list may
        // be at its cap and silently dropping what it is handed.
        kills: number(entry, "kills") as u32,
        travelled: number(entry, "travelled"),
        longest_fight: number(entry, "longestFight") as u32,
        worst_hit: number(entry, "worstHit"),
        worst_hit_by: entry
            .get("worstHitBy")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .map(str::to_string),
        // Absent means the addon predates the field, and "nothing ever touched
        // them" is the right reading of that: it suppresses the row rather
        // than reporting a character who spent every evening at nought percent.
        lowest_health: entry
            .get("lowestHealth")
            .and_then(Value::as_f64)
            .map(|percent| percent as u8)
            .unwrap_or(100),
        risen: entry
            .get("risen")
            .map(|list| {
                list.items()
                    .iter()
                    .filter_map(|row| {
                        let [name, rank, ..] = row.items() else {
                            return None;
                        };
                        Some((name.as_str()?.to_string(), rank.as_u32().unwrap_or(0) as u8))
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// One `{ at, kind, a, b, c }` row.
///
/// A row whose `kind` this version does not know is dropped, which is what
/// makes an older Armory readable by a newer addon within the same format: a
/// new kind of event is silence rather than a parse failure.
fn moment(row: &Value) -> Option<Moment> {
    let fields = row.items();
    let at = fields.first()?.as_f64()? as u32;
    let kind = fields.get(1)?.as_str()?;
    let a = text(fields.get(2));
    let b = text(fields.get(3));
    let c = text(fields.get(4));

    let what = match kind {
        "zone" => Happening::Arrived {
            zone: a?,
            subzone: b,
            map: c.and_then(|id| id.parse().ok()),
        },
        "accepted" => Happening::Accepted {
            title: a?,
            premise: b,
        },
        "quest" => Happening::Completed {
            // The id is written as a number; `text` renders it back so that one
            // reader handles both, and the parse below is what turns it into an
            // id again. A quest with an unreadable id is dropped, because the
            // id is what the outbound link is built from.
            quest: a?.parse().ok()?,
            title: b.unwrap_or_default(),
            story: c,
        },
        "questpay" => Happening::Paid {
            quest: a?.parse().ok()?,
            money: b.and_then(|money| money.parse().ok()).unwrap_or(0),
            experience: c.and_then(|xp| xp.parse().ok()).unwrap_or(0),
        },
        "campaign" => Happening::Campaign {
            name: a?,
            summary: b,
        },
        "level" => Happening::Levelled {
            level: a?.parse().ok()?,
            zone: b.unwrap_or_default(),
        },
        "death" => Happening::Died {
            zone: a?,
            subzone: b,
            to: c,
        },
        "instance" => Happening::Entered {
            name: a?,
            kind: b.unwrap_or_default(),
            group: c.and_then(|size| size.parse().ok()).unwrap_or(0),
        },
        // `dungeon|level|onTime|upgrades` in one field, because five positions
        // is what a row has and a keystone needs six. Packed at the addon end
        // and unpacked here rather than widening every row in the file for one
        // event kind that happens at most a handful of times an evening.
        "keystone" => {
            let packed = a?;
            let mut parts = packed.rsplitn(4, '|');
            let upgrades = parts.next()?.parse().ok()?;
            let in_time = parts.next()? == "1";
            let level = parts.next()?.parse().ok()?;
            let dungeon = parts.next()?.to_string();
            Happening::Keystone {
                dungeon,
                level,
                in_time,
                upgrades,
                seconds: b.and_then(|seconds| seconds.parse().ok()).unwrap_or(0),
            }
        }
        "said" => Happening::Said {
            // An unnamed speaker is the world itself — a zone-wide yell with no
            // source — and is worth keeping without one.
            who: a.unwrap_or_default(),
            line: b?,
        },
        "giver" => Happening::Gave {
            who: a?,
            quest: b.and_then(|id| id.parse().ok()),
            creature: c.and_then(|id| id.parse().ok()),
        },
        "gossip" => Happening::Told {
            who: a.unwrap_or_default(),
            line: b?,
        },
        "expired" => Happening::Expired { what: a? },
        "cutscene" => Happening::Cutscene {
            zone: a?,
            movie: b.and_then(|id| id.parse().ok()),
        },
        "recipe" => Happening::Learned { name: a? },
        "scenario" => Happening::Scenario { name: a?, tier: b },
        "rare" => Happening::Rare {
            name: a?,
            rank: b.unwrap_or_default(),
        },
        "skill" => Happening::Practised {
            profession: a?,
            skill: b.and_then(|rank| rank.parse().ok()).unwrap_or(0),
        },
        "equipped" => Happening::Equipped {
            name: a?,
            item_level: b.and_then(|level| level.parse().ok()).unwrap_or(0),
            gained: c.and_then(|gained| gained.parse().ok()).unwrap_or(0),
        },
        "appearance" => Happening::Appearance { name: a? },
        "coin" => Happening::Coin {
            purpose: Purpose::from_token(&a?),
            amount: b.and_then(|amount| amount.parse().ok()).unwrap_or(0),
            incoming: c.as_deref() == Some("1"),
        },
        "craft" => Happening::Crafted {
            recipe: a?.parse().ok()?,
            name: b.unwrap_or_default(),
        },
        "shot" => Happening::Pictured {
            what: a?,
            subject: b.unwrap_or_default(),
        },
        "boss" => Happening::Felled { name: a? },
        "encounter" => Happening::Fought {
            name: a?,
            won: b.as_deref() == Some("1"),
        },
        "achievement" => Happening::Earned {
            achievement: a?.parse().ok()?,
            name: b.unwrap_or_default(),
        },
        "gained" => Happening::Acquired {
            kind: Acquisition::from_token(&a?)?,
            name: b?,
        },
        "loot" => Happening::Looted {
            item: a?.parse().ok()?,
            name: b?,
            quality: c.and_then(|quality| quality.parse().ok()).unwrap_or(0),
        },
        "flight" => Happening::Flew { from: a? },
        "sale" => Happening::Sold {
            subject: a?,
            money: b.and_then(|money| money.parse().ok()).unwrap_or(0),
        },
        "with" => Happening::Alongside { name: a? },
        _ => return None,
    };

    Some(Moment { at, what })
}

/// A field as text, with the addon's empty-string stand-in read back as absent.
///
/// Numbers come through as numbers and are rendered rather than refused, so one
/// accessor serves both the string columns and the id ones.
fn text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Str(text) if text.is_empty() => None,
        Value::Str(text) => Some(text.clone()),
        Value::Number(number) if number.fract() == 0.0 => Some(format!("{}", *number as i64)),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn number(entry: &Value, field: &str) -> u64 {
    entry
        .get(field)
        .and_then(Value::as_f64)
        .filter(|number| *number >= 0.0)
        .unwrap_or(0.0) as u64
}

fn epoch(seconds: f64) -> Option<DateTime<Utc>> {
    Utc.timestamp_opt(seconds as i64, 0).single()
}

/// `DRUID` is how the game spells a class; `Druid` is how a person does.
fn titlecase(token: &str) -> String {
    let mut characters = token.chars();
    match characters.next() {
        Some(first) => {
            first.to_uppercase().collect::<String>() + &characters.as_str().to_lowercase()
        }
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the addon actually writes, in the shape WoW's serializer emits it.
    const SAMPLE: &str = r#"
ArmoryChronicleDB = {
	["format"] = 1,
	["sessions"] = {
		{
			["startedAt"] = 1785000000,
			["endedAt"] = 1785009000,
			["name"] = "Somechar",
			["realm"] = "Emerald Dream",
			["class"] = "DRUID",
			["race"] = "Tauren",
			["faction"] = "Horde",
			["startLevel"] = 70,
			["endLevel"] = 71,
			["startMoney"] = 1000000,
			["endMoney"] = 1250000,
			["startItemLevel"] = 600.4,
			["endItemLevel"] = 604.2,
			["events"] = {
				{ 0, "zone", "Nagrand", "", 107 },
				{ 12, "accepted", "Hero of the Mag'har", "Garrosh needs a champion.", "" },
				{ 640, "quest", 9999, "Hero of the Mag'har", "The Mag'har will sing of this." },
				{ 640, "questpay", 9999, 45000, 1200 },
				{ 900, "level", 71, "Nagrand", "" },
				{ 1200, "death", "Nagrand", "Halaa", "" },
				{ 1500, "encounter", "Durn the Hungerer", 0, 14 },
				{ 1800, "encounter", "Durn the Hungerer", 1, 14 },
				{ 2000, "loot", 32458, "Ashes of Al'ar", 5 },
				{ 2100, "gained", "mount", "Ashes of Al'ar", "" },
				{ 2200, "achievement", 4956, "Loremaster of Kalimdor", "" },
				{ 2300, "sale", "Auction successful: Mycobloom", 250000, "" },
				{ 2300, "coin", "sale", 250000, 1 },
				{ 2350, "coin", "repair", 9000, 0 },
				{ 2360, "craft", 371637, "Flask of Alchemical Chaos", "" },
				{ 2400, "flight", "Nagrand", "", "" },
				{ 2500, "scenario", "The Sinkhole", "Tier 11", "" },
				{ 640, "giver", "Garrosh Hellscream", 9999, 18166 },
				{ 2600, "recipe", "Flask of Alchemical Chaos", "", "" },
				{ 2700, "said", "Garrosh Hellscream", "The Mag'har will sing of this day.", "" },
				{ 2800, "expired", "Auction expired: Mycobloom", "", "" },
				{ 2850, "gossip", "Nisha", "The elements are restless in Nagrand.", "" },
				{ 2900, "cutscene", "Nagrand", 872, "" },
				{ 2950, "cutscene", "Nagrand", "", "" },
				{ 2400, "with", "Velkurai", "", "" },
			},
			-- Session totals, written at logout rather than as events.
			["kills"] = 214,
			["travelled"] = 41288,
			["longestFight"] = 664,
			["worstHit"] = 812004,
			["worstHitBy"] = "Durn the Hungerer",
			["lowestHealth"] = 7,
		},
	},
}
"#;

    #[test]
    fn a_session_reads_whole() {
        let sessions = read(SAMPLE).expect("read");
        assert_eq!(sessions.len(), 1);

        let session = &sessions[0];
        assert_eq!(session.character.realm_slug, "emerald-dream");
        assert_eq!(session.display_name, "Somechar");
        // `DRUID` is how the game spells it.
        assert_eq!(session.class, "Druid");
        assert_eq!(session.faction, Faction::Horde);
        assert_eq!(session.start_level, 70);
        assert_eq!(session.end_level, 71);
        assert_eq!(session.start_item_level, 600);
        assert_eq!(session.duration().num_minutes(), 150);
    }

    #[test]
    fn every_kind_of_moment_survives_the_round_trip() {
        let session = &read(SAMPLE).expect("read")[0];
        let digest = session.digest();

        assert_eq!(digest.route[0].zone, "Nagrand");
        assert_eq!(digest.quests.len(), 1);
        assert_eq!(digest.quests[0].id, 9999);
        assert_eq!(digest.quests[0].title, "Hero of the Mag'har");
        assert_eq!(
            digest.quests[0].premise.as_deref(),
            Some("Garrosh needs a champion.")
        );
        assert_eq!(
            digest.quests[0].story.as_deref(),
            Some("The Mag'har will sing of this.")
        );
        assert_eq!(digest.quests[0].money, 45_000);
        assert_eq!(digest.levels, [(71, "Nagrand".to_string())]);
        assert_eq!(digest.deaths[0].subzone.as_deref(), Some("Halaa"));
        // Wiped on, then killed. It counts as killed.
        assert_eq!(digest.felled, ["Durn the Hungerer"]);
        assert!(digest.lost_to.is_empty());
        assert_eq!(digest.loot, [(32458, "Ashes of Al'ar".to_string(), 5)]);
        assert_eq!(
            digest.acquired,
            [(Acquisition::Mount, "Ashes of Al'ar".to_string())]
        );
        assert_eq!(
            digest.achievements,
            [(4956, "Loremaster of Kalimdor".into())]
        );
        // Off the ledger, which is the only set of books money is counted in.
        assert_eq!(digest.sale_income, 250_000);
        assert_eq!(
            digest.spending,
            [(crate::chronicle::Purpose::Repair, 9_000)]
        );
        assert_eq!(
            digest.crafted,
            [("Flask of Alchemical Chaos".to_string(), 1)]
        );
        assert_eq!(digest.flights, 1);
        // A delve is a scenario with a tier, and the tier is the whole
        // difference between one and another.
        assert_eq!(digest.scenarios, ["The Sinkhole (Tier 11)"]);
        assert_eq!(digest.learned, ["Flask of Alchemical Chaos"]);
        assert_eq!(digest.questgivers, [("Garrosh Hellscream".to_string(), 1)]);
        assert_eq!(
            digest.overheard,
            [(
                "Garrosh Hellscream".to_string(),
                "The Mag'har will sing of this day.".to_string()
            )]
        );
        assert_eq!(digest.expired, ["Auction expired: Mycobloom"]);
        assert_eq!(
            digest.told,
            [(
                "Nisha".to_string(),
                "The elements are restless in Nagrand.".to_string()
            )]
        );
        // A pre-rendered movie names itself; an in-engine cutscene cannot.
        assert_eq!(
            digest.cutscenes,
            [
                ("Nagrand".to_string(), Some(872)),
                ("Nagrand".to_string(), None)
            ]
        );
        assert_eq!(digest.travelled, 41_288);
        assert_eq!(digest.longest_fight, 664);
        assert_eq!(digest.worst_hit, 812_004);
        assert_eq!(digest.worst_hit_by.as_deref(), Some("Durn the Hungerer"));
        assert_eq!(digest.lowest_health, 7);
        assert_eq!(digest.companions, ["Velkurai"]);
        assert_eq!(digest.purse, 250_000);
    }

    #[test]
    fn an_empty_string_is_read_back_as_absent() {
        // The addon writes "" rather than nil so its rows stay dense. Reading
        // that as a subzone called "" would put an empty bullet on every stop.
        let sessions = read(SAMPLE).expect("read");
        let arrived = &sessions[0].moments[0];
        assert_eq!(
            arrived.what,
            Happening::Arrived {
                zone: "Nagrand".into(),
                subzone: None,
                // The map id is what a lore corpus and a zone tally join on,
                // because two zones share this name across two expansions.
                map: Some(107),
            }
        );
    }

    #[test]
    fn an_event_kind_this_version_does_not_know_is_silence_rather_than_a_failure() {
        // What lets a newer addon write for an older Armory within one format:
        // the unknown row is dropped and the rest of the evening still reads.
        let sessions = read(
            r#"ArmoryChronicleDB = { ["format"] = 1, ["sessions"] = { {
                ["startedAt"] = 1785000000, ["name"] = "Somechar", ["realm"] = "Emerald Dream",
                ["events"] = {
                    { 0, "zone", "Durotar", "", "" },
                    { 5, "somethingnew", "x", "", "" },
                    { 9, "death", "Durotar", "", "" },
                } } } }"#,
        )
        .expect("read");
        assert_eq!(sessions[0].moments.len(), 2);
    }

    #[test]
    fn a_session_that_never_closed_ends_at_its_last_event() {
        // A crash, or a client killed rather than logged out. The events are
        // real, so the evening is closed at the last one rather than discarded
        // — and a zero-length session would divide by nothing later.
        let sessions = read(
            r#"ArmoryChronicleDB = { ["format"] = 1, ["sessions"] = { {
                ["startedAt"] = 1785000000, ["name"] = "Somechar", ["realm"] = "Emerald Dream",
                ["events"] = { { 3600, "zone", "Durotar", "", "" } } } } }"#,
        )
        .expect("read");
        assert_eq!(sessions[0].duration().num_minutes(), 60);
    }

    #[test]
    fn someone_elses_saved_variables_are_recognised_as_not_ours() {
        assert_eq!(
            read(r#"TradeSkillMasterDB = { ["x"] = 1 }"#).expect_err("not ours"),
            ReadError::NotChronicleData
        );
    }

    #[test]
    fn a_newer_addon_says_so_rather_than_reading_as_broken() {
        let error =
            read(r#"ArmoryChronicleDB = { ["format"] = 99 }"#).expect_err("from the future");
        assert_eq!(error, ReadError::FromTheFuture { format: 99 });
        assert!(error.to_string().contains("update Armory"));
    }

    #[test]
    fn a_file_with_no_sessions_yet_is_valid_and_empty() {
        // The first launch after installing the addon, before anybody has
        // logged out. Not an error, and not something to warn about.
        assert!(read(r#"ArmoryChronicleDB = { ["format"] = 1 }"#)
            .expect("read")
            .is_empty());
    }

    #[test]
    fn a_truncated_file_is_reported_rather_than_half_read() {
        let error = read(r#"ArmoryChronicleDB = { ["sessions"] = { {"#).expect_err("truncated");
        assert!(matches!(error, ReadError::Unparsable(_)), "{error:?}");
    }
}
