//! A play session, and the journal entry written from it.
//!
//! Every other page here answers a question about *state* — what is owned, what
//! is left, what a character is worth. This one is the only thing in Armory
//! that is about *time*: what happened, in what order, on one evening.
//!
//! That distinction decides where the data comes from. Blizzard's profile API
//! is a logout snapshot with no history in it whatsoever; it will report 4,312
//! completed quests and never which twelve of them were finished tonight. So
//! the chronicle is fed entirely by the addon, and the useful consequence is
//! that it works with no Battle.net client at all — no token, no quota, no
//! thirty-day term, because none of it was obtained through the API.
//!
//! The shape is three steps, and each is a pure function over the last:
//!
//! 1. [`Session`] — what the addon recorded, in order.
//! 2. [`Digest`] — the same evening rolled up: a route rather than sixty zone
//!    changes, a purse rather than four hundred money events. This is what a
//!    person is shown, and it is complete on its own.
//! 3. [`Entry`] — the prose, written by a `llama-server` on this machine from
//!    the digest.
//!
//! Step three is optional and step two is not. An evening that is never written
//! up is still recorded, still shown, and still worth having; the entry is the
//! flourish on top of a log that stands by itself.
//!
//! **Nothing here fetches lore, and it does not need to.** The quest text the
//! addon captured is the real thing — the sentences the game put on the screen
//! — and the campaign summaries beside them are Blizzard's own. Both are better
//! than a third-party summary and both cost no request. Where somebody wants to
//! read further, [`Digest::further_reading`] hands them *links*, the way the
//! collections pages do.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::character::{CharacterKey, Faction};

/// How long an evening has to be, with nothing else to show for it, before it
/// counts as an evening at all.
///
/// Below this and with no quest, kill or acquisition in it, the session was
/// somebody logging in to check the mail. Recording those is harmless; showing
/// them is what turns a journal into a login log.
const IDLE: i64 = 15 * 60;

/// How long before a drop something may have died and still be credited with
/// it, in seconds.
///
/// Loot lands within a second or two of the kill. The window is wide enough to
/// survive a slow client and a full bag, and narrow enough that the previous
/// pull is never the answer.
const SPOILS: u32 = 30;

/// How long after the addon asked for a screenshot the file may appear, in
/// seconds.
///
/// The addon waits a beat before firing so the toast is on screen, and a busy
/// client takes a moment to write a PNG. Generous in one direction only: the
/// client cannot have written the file before it was asked, so a picture taken
/// *earlier* than the moment is somebody pressing Print Screen themselves and
/// is not ours to claim.
const SHUTTER: i64 = 15;

/// One recorded play session, exactly as the addon saw it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub character: CharacterKey,
    /// As the game capitalises it.
    pub display_name: String,
    pub realm_name: String,
    pub class: String,
    pub race: String,
    pub faction: Faction,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub start_level: u8,
    pub end_level: u8,
    /// Copper, both.
    pub start_money: u64,
    pub end_money: u64,
    pub start_item_level: u16,
    pub end_item_level: u16,
    pub moments: Vec<Moment>,

    /// How many things the party finished off, across the whole evening.
    ///
    /// A session total rather than a moment, because that is what it is — and
    /// because the addon produces it at logout, when the event list may
    /// already be full.
    #[serde(default)]
    pub kills: u32,
    /// Factions that went up a rank, with the rank reached. The threshold is
    /// the milestone; three hundred more reputation is not.
    #[serde(default)]
    pub risen: Vec<(String, u8)>,
    /// Yards covered, on foot and in the air together.
    ///
    /// A session total for the same reason, and a stronger one: there is no
    /// event for moving at all. The addon samples position, and a sample a
    /// second for four hours is fourteen thousand rows saying nothing.
    #[serde(default)]
    pub travelled: u64,
    /// The longest single fight, in seconds.
    ///
    /// The difference between an evening of six-second pulls and one boss that
    /// took eleven minutes, which is otherwise the same "1 boss".
    #[serde(default)]
    pub longest_fight: u32,
    /// The hardest single hit taken, and what landed it.
    #[serde(default)]
    pub worst_hit: u64,
    #[serde(default)]
    pub worst_hit_by: Option<String>,
    /// The lowest the health bar got without the character dying, as a
    /// percentage.
    ///
    /// Defaults to 100 rather than 0, because an addon that predates the field
    /// recorded nothing rather than a character who spent the evening at
    /// death's door.
    #[serde(default = "full_health")]
    pub lowest_health: u8,
}

/// What `lowest_health` means when nothing recorded it.
fn full_health() -> u8 {
    100
}

/// One thing that happened, and how far into the session it happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moment {
    /// Seconds since the session began.
    ///
    /// Relative rather than absolute, because that is the form every use wants:
    /// "an hour in" is a fact about the evening and a Unix timestamp is not.
    pub at: u32,
    pub what: Happening,
}

/// What the addon can tell us happened.
///
/// A closed set, and deliberately narrow. Every variant here is something the
/// game raises a documented event for; there is no `Other(String)` because a
/// row nobody can interpret is a row that reaches a journal entry as noise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Happening {
    /// Entered a zone. Subzones are the difference between "Durotar" and "the
    /// Echo Isles", which is most of what makes a route readable.
    Arrived {
        zone: String,
        subzone: Option<String>,
        /// Blizzard's `UiMapID` for the zone.
        ///
        /// Carried because the *name* is not unique — there are two Nagrands
        /// and two Shadowmoon Valleys, on different continents in different
        /// expansions — so anything joining a session to a place joins on this
        /// and never on the string. `None` for a session recorded before the
        /// addon read it.
        map: Option<u32>,
    },
    /// Took a quest, with the premise the quest giver gave.
    Accepted {
        title: String,
        premise: Option<String>,
    },
    /// Turned a quest in, with the text shown at the turn-in.
    ///
    /// `story` is the single most valuable field in this file. It is the
    /// sentences the player actually read, which no endpoint returns and no
    /// summary elsewhere improves on.
    Completed {
        quest: u32,
        title: String,
        story: Option<String>,
    },
    /// What a quest paid, kept apart from the session's gold so that "the
    /// escort paid better than the whole afternoon" stays sayable.
    Paid {
        quest: u32,
        money: u64,
        experience: u64,
    },
    /// A storyline a quest belonged to, named once per campaign per session.
    ///
    /// The shape a dozen turn-ins have when they are not just a dozen titles.
    /// `summary` is Blizzard's own paragraph about the campaign, which is as
    /// close to an official plot synopsis as anything gets.
    Campaign {
        name: String,
        summary: Option<String>,
    },
    Levelled {
        level: u8,
        zone: String,
    },
    /// Died, and — where the combat log caught it — to what.
    Died {
        zone: String,
        subzone: Option<String>,
        /// The last thing to land a hit. `None` for a fall, a drowning, or a
        /// death the log did not attribute.
        to: Option<String>,
    },
    /// Entered an instance: a dungeon, a raid, a scenario, a battleground.
    ///
    /// Without this a Mythic+ run, a heroic raid night and walking through the
    /// front door of the same building are one zone name.
    Entered {
        name: String,
        /// `party`, `raid`, `scenario`, `arena` or `pvp`, with the difficulty
        /// where the game gives one.
        kind: String,
        group: u8,
    },
    /// A keystone finished.
    Keystone {
        dungeon: String,
        level: u8,
        /// Whether the timer held.
        in_time: bool,
        /// How many the key went up by, which is the number the group cared
        /// about.
        upgrades: u8,
        seconds: u32,
    },
    /// A scenario or delve completed.
    Scenario {
        name: String,
        /// The delve tier, where the thing finished was a delve.
        ///
        /// A delve is a scenario as far as `GetInstanceInfo` is concerned, and
        /// the tier is the entire difference between one and another. Absent
        /// for an ordinary scenario, which has no tier at all.
        tier: Option<String>,
    },
    /// Something rare, rare-elite or a world boss, killed and named.
    ///
    /// Separate from [`Happening::Felled`], which is instance bosses. This is
    /// the thing you were not looking for and stopped to fight.
    Rare {
        name: String,
        rank: String,
    },
    /// A profession that got better at something.
    Practised {
        profession: String,
        skill: u16,
    },
    /// Something better got worn. Only ever an upgrade; a trinket swapped for
    /// one fight and swapped back is not news.
    Equipped {
        name: String,
        item_level: u16,
        gained: u16,
    },
    /// An appearance the account had never seen.
    Appearance {
        name: String,
    },
    /// Who gave, or took, a quest.
    ///
    /// Two identifiers because they answer different questions. The **name**
    /// connects a character across expansions — Khadgar in Outland, Khadgar in
    /// Draenor and Khadgar in the Broken Isles are three creatures in
    /// Blizzard's data and one person in anybody's memory. The **creature id**
    /// tells two NPCs who share a name apart within one version, of which
    /// there are a great many called `Stormwind Guard`.
    ///
    /// `quest` is absent where the frame was open but the id was not yet
    /// known, which is every quest that was read and not accepted.
    Gave {
        who: String,
        quest: Option<u32>,
        creature: Option<u32>,
    },
    /// Something an NPC said, and who said it.
    ///
    /// The scripted lines: an NPC talking to another NPC, a boss mid-pull, an
    /// escort narrating itself. Written content the player read, that no
    /// endpoint has — the same argument as the quest text, applied to
    /// everything that happens between the quests.
    ///
    /// Never a player. The addon registers only the monster and boss chat
    /// events; what somebody said in party is their business.
    Said {
        who: String,
        line: String,
    },
    /// What an NPC said when the player clicked them.
    ///
    /// Kept apart from [`Happening::Said`] on purpose: this is something the
    /// player chose to read, and that is the whole difference. A lot of it is
    /// a vendor greeting, which is exactly why the distinction is carried all
    /// the way to the prompt — "much of this is functional" is a true thing to
    /// say about gossip and a false one to say about a boss mid-fight.
    Told {
        who: String,
        line: String,
    },
    /// A cutscene played, and where.
    ///
    /// `movie` is set only for a pre-rendered one, which raises `PLAY_MOVIE`
    /// with a stable `MovieID` that names the cinematic exactly. An in-engine
    /// cutscene has no identifier of any kind — so it is recorded as having
    /// happened, and the quest turned in a moment later is what names it.
    ///
    /// Subtitles are not readable by an addon. The dialogue usually is, through
    /// the ordinary monster chat events, which is what [`Happening::Said`]
    /// collects.
    Cutscene {
        zone: String,
        movie: Option<u32>,
    },
    /// An auction that came back unsold.
    ///
    /// The only evidence anywhere that something did *not* sell. Blizzard
    /// records a failure no more than it records a sale, and this one arrives
    /// as a mail with no money attached.
    Expired {
        what: String,
    },
    /// A recipe learned.
    ///
    /// Its own thing rather than a skill-up: a profession rank going up by one
    /// is a number, and "learned to make Flasks of Alchemical Chaos" is a
    /// thing that happened.
    Learned {
        name: String,
    },
    /// Money moved, and what it was in front of at the time.
    ///
    /// `PLAYER_MONEY` says the total changed and never why, so the addon
    /// records the frame that was open — which is the question the event will
    /// not answer. [`Purpose`] is where that becomes a source or a cost.
    Coin {
        purpose: Purpose,
        amount: u64,
        incoming: bool,
    },
    /// Something was made. Counted per recipe, which the game does not do.
    Crafted {
        recipe: u32,
        name: String,
    },
    /// A flight path taken, and where from.
    ///
    /// The map closing is not the flight — being on a taxi a moment later is.
    /// Recorded because "six flights" is the shape of an evening spent doing
    /// errands, and because where somebody flies *from* is where they keep
    /// having to go back to.
    Flew {
        from: String,
    },
    /// A screenshot the addon took, and what of.
    ///
    /// The addon cannot learn the filename the client wrote — there is no API
    /// for it — so what is recorded is the *moment*, and Armory matches that
    /// against the files' timestamps afterwards. See
    /// [`crate::chronicle::Digest::pictures`].
    Pictured {
        what: String,
        subject: String,
    },
    /// A boss that died.
    Felled {
        name: String,
    },
    /// An encounter that ended, won or lost. A wipe is as much of a story as a
    /// kill and more of one on the tenth attempt.
    Fought {
        name: String,
        won: bool,
    },
    Earned {
        achievement: u32,
        name: String,
    },
    /// A mount, pet or toy that arrived.
    Acquired {
        kind: Acquisition,
        name: String,
    },
    /// Loot worth mentioning — rare quality and above.
    Looted {
        item: u32,
        name: String,
        quality: u8,
    },
    /// The auction house sent money. `subject` is the mail's subject line,
    /// which carries the item's name.
    Sold {
        subject: String,
        money: u64,
    },
    /// Somebody who was in the party.
    Alongside {
        name: String,
    },
}

/// What money was doing when it moved.
///
/// Everything in this game that takes or gives gold does it through a frame, so
/// the frame that was open is the attribution. The two that are not frames —
/// a quest reward and coin off the ground — are the two the addon can name
/// outright.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Purpose {
    Quest,
    /// Coin picked up, with nothing open to explain it.
    Loot,
    Vendor,
    Repair,
    /// An auction bid or buyout.
    Bid,
    /// A listing deposit, told from a bid by an auction having just been made.
    Deposit,
    /// The auction house paying out. Income.
    Sale,
    /// Gold another character on this account sent over. Not income — the
    /// account had it already, and counting it would let somebody earn the
    /// same gold on every character they own.
    Transfer,
    /// Gold somebody else sent, which is neither a sale nor the account's own
    /// money moving.
    Gift,
    /// Money out of a mailbox whose sender was not readable. Honest rather
    /// than filed under one of the three above.
    Mail,
    Trade,
    Taxi,
    Trainer,
    Transmog,
    Barber,
    GuildBank,
    /// Money left with nothing open to account for it. Rare, and admitted
    /// rather than filed under something plausible.
    Unknown,
}

impl Purpose {
    pub fn from_token(token: &str) -> Purpose {
        match token {
            "quest" => Purpose::Quest,
            "loot" => Purpose::Loot,
            "vendor" => Purpose::Vendor,
            "repair" => Purpose::Repair,
            "bid" => Purpose::Bid,
            "deposit" => Purpose::Deposit,
            "sale" => Purpose::Sale,
            "transfer" => Purpose::Transfer,
            "gift" => Purpose::Gift,
            "mail" => Purpose::Mail,
            "trade" => Purpose::Trade,
            "taxi" => Purpose::Taxi,
            "trainer" => Purpose::Trainer,
            "transmog" => Purpose::Transmog,
            "barber" => Purpose::Barber,
            "guildbank" => Purpose::GuildBank,
            _ => Purpose::Unknown,
        }
    }

    /// How it reads on a card, as the receiving or the paying side.
    pub fn label(self, incoming: bool) -> &'static str {
        match (self, incoming) {
            (Purpose::Quest, _) => "Quest rewards",
            (Purpose::Loot, _) => "Found",
            (Purpose::Vendor, true) => "Sold to vendors",
            (Purpose::Vendor, false) => "Bought from vendors",
            (Purpose::Repair, _) => "Repairs",
            (Purpose::Bid, true) => "Auction refunds",
            (Purpose::Bid, false) => "Auction purchases",
            (Purpose::Deposit, _) => "Auction deposits",
            (Purpose::Sale, _) => "Auction sales",
            (Purpose::Transfer, _) => "Sent from another character",
            (Purpose::Gift, _) => "Sent by somebody else",
            (Purpose::Mail, true) => "Mail",
            (Purpose::Mail, false) => "Postage",
            (Purpose::Trade, true) => "Traded to you",
            (Purpose::Trade, false) => "Traded away",
            (Purpose::Taxi, _) => "Flights",
            (Purpose::Trainer, _) => "Training",
            (Purpose::Transmog, _) => "Transmogrification",
            (Purpose::Barber, _) => "The barber",
            (Purpose::GuildBank, true) => "From the guild bank",
            (Purpose::GuildBank, false) => "To the guild bank",
            (Purpose::Unknown, true) => "Arrived",
            (Purpose::Unknown, false) => "Spent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Acquisition {
    Mount,
    Pet,
    Toy,
}

impl Acquisition {
    pub fn from_token(token: &str) -> Option<Acquisition> {
        match token {
            "mount" => Some(Acquisition::Mount),
            "pet" => Some(Acquisition::Pet),
            "toy" => Some(Acquisition::Toy),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Acquisition::Mount => "mount",
            Acquisition::Pet => "pet",
            Acquisition::Toy => "toy",
        }
    }
}

/// Somewhere the character was, and for how long.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stop {
    pub zone: String,
    /// The subzones passed through while there, in order and deduplicated.
    pub within: Vec<String>,
    /// Seconds spent before moving on.
    pub stayed: u32,
}

/// Where a death happened, and what did it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Death {
    pub zone: String,
    pub subzone: Option<String>,
    /// The last thing to land a hit, where the combat log caught it. A fall,
    /// a drowning and a death nothing was blamed for all read as `None` —
    /// which is honest, and better than naming the wrong culprit.
    pub to: Option<String>,
}

/// A keystone run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keystone {
    pub dungeon: String,
    pub level: u8,
    pub in_time: bool,
    pub upgrades: u8,
    pub seconds: u32,
}

/// A piece of gear that was actually an upgrade, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Upgrade {
    pub name: String,
    pub item_level: u16,
    pub gained: u16,
    /// What dropped it, if the evening can say.
    ///
    /// Nothing in the game connects a piece of gear to the thing that dropped
    /// it — the loot event names an item and the encounter event names a boss,
    /// and they are minutes apart in a list of several hundred rows. So this is
    /// a join over the evening's own record: the item was looted, and something
    /// with a name went down shortly before. Absent whenever that cannot be
    /// said, because "from a quest" and "off the auction house" are also true
    /// of a lot of gear and neither is worth guessing at.
    pub from: Option<String>,
}

/// A quest, as the evening will remember it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quest {
    pub id: u32,
    pub title: String,
    /// The premise, from accepting it.
    pub premise: Option<String>,
    /// What the turn-in said. The story, in the game's own words.
    pub story: Option<String>,
    pub money: u64,
}

/// One evening, rolled up.
///
/// This is the whole feature standing on its own: everything below is drawn on
/// a card whether or not anybody ever spends a token writing prose about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Digest {
    pub character: CharacterKey,
    pub display_name: String,
    pub realm_name: String,
    pub class: String,
    pub race: String,
    pub faction: Faction,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,

    pub route: Vec<Stop>,
    /// The storylines the evening's quests belonged to, in the order they were
    /// first touched. This is what turns a list of turn-ins into an arc.
    pub campaigns: Vec<(String, Option<String>)>,
    pub quests: Vec<Quest>,
    /// Quests accepted and not turned in — what was left hanging.
    pub taken_up: Vec<String>,
    pub levels: Vec<(u8, String)>,
    /// Where each death happened, and what did it.
    pub deaths: Vec<Death>,
    pub felled: Vec<String>,
    pub lost_to: Vec<String>,
    /// Rares and world bosses, which are the ones you stopped for.
    pub rares: Vec<String>,
    /// Instances entered, deduplicated: `("Halls of Atonement", "party, Mythic Keystone")`.
    pub instances: Vec<(String, String)>,
    /// Keystones finished.
    pub keystones: Vec<Keystone>,
    pub scenarios: Vec<String>,
    pub achievements: Vec<(u32, String)>,
    pub acquired: Vec<(Acquisition, String)>,
    pub loot: Vec<(u32, String, u8)>,
    pub sales: Vec<(String, u64)>,
    pub companions: Vec<String>,
    /// Professions that improved, at the best skill reached.
    pub practised: Vec<(String, u16)>,
    /// Gear upgrades, best first — the one that mattered is the biggest jump.
    pub equipped: Vec<Upgrade>,
    pub appearances: Vec<String>,
    /// Recipes learned.
    pub learned: Vec<String>,
    /// What the world said, in the order it said it.
    pub overheard: Vec<(String, String)>,
    /// Auctions that came back unsold.
    pub expired: Vec<String>,
    /// Cutscenes that played, with the pre-rendered ones' movie ids.
    pub cutscenes: Vec<(String, Option<u32>)>,
    /// What NPCs said when the player clicked them.
    pub told: Vec<(String, String)>,
    /// Who handed out or took in this evening's quests, and how many each.
    ///
    /// Ordered by how many, because "Khadgar sent you on six of tonight's
    /// eight" is the shape of an evening and a list of names is not.
    pub questgivers: Vec<(String, u32)>,
    /// Where money came from, largest first.
    ///
    /// The ledger is the only source of truth for money in a digest. A quest
    /// reward is counted here and *not* again from the quest itself; a sale is
    /// counted here and the item names come from `sales`. One set of books.
    pub income: Vec<(Purpose, u64)>,
    /// Where money went, largest first.
    pub spending: Vec<(Purpose, u64)>,
    /// What was made, and how many times each.
    pub crafted: Vec<(String, u32)>,
    /// Flight paths taken.
    pub flights: u32,
    /// Screenshots the addon took, as seconds into the session and what of.
    ///
    /// Filenames are matched on afterwards, because the addon has no way to
    /// know them. See [`Digest::pictures`].
    pub shots: Vec<(u32, String)>,
    /// How much was killed, and which factions moved up a rank.
    pub kills: u32,
    pub risen: Vec<(String, u8)>,
    /// Yards covered, on foot and in the air together.
    pub travelled: u64,
    /// The longest single fight, in seconds.
    ///
    /// The difference between an evening of six-second pulls and one boss that
    /// took eleven minutes, which is otherwise the same "1 boss".
    pub longest_fight: u32,
    /// The hardest single hit taken, and what landed it.
    pub worst_hit: u64,
    pub worst_hit_by: Option<String>,
    /// The lowest the health bar got without the character dying, as a
    /// percentage. 100 means nothing ever touched them.
    pub lowest_health: u8,

    pub start_level: u8,
    pub end_level: u8,
    pub start_item_level: u16,
    pub end_item_level: u16,
    /// End minus start, in copper. Negative is a shopping trip.
    pub purse: i64,
    pub quest_income: u64,
    pub sale_income: u64,
}

impl Session {
    /// What dropped a piece of gear, if this evening can say.
    ///
    /// Two steps, both bounded. The item has to have been *looted* tonight —
    /// gear bought, crafted or handed over by a quest giver has no drop to name
    /// — and something with a name has to have died in the `SPOILS` before it.
    /// A kill further back than that is a different pull, and claiming it would
    /// put the wrong boss's name on somebody's belt.
    fn where_it_came_from(&self, item: &str) -> Option<String> {
        let looted = self.moments.iter().find_map(|moment| match &moment.what {
            Happening::Looted { name, .. } if name == item => Some(moment.at),
            _ => None,
        })?;

        self.moments
            .iter()
            .filter(|moment| moment.at <= looted && looted - moment.at <= SPOILS)
            .filter_map(|moment| match &moment.what {
                Happening::Felled { name } | Happening::Rare { name, .. } => Some(name.clone()),
                _ => None,
            })
            .next_back()
    }

    /// The key an entry is filed under: one character, one start time.
    pub fn id(&self) -> SessionId {
        SessionId {
            character: self.character.clone(),
            started_at: self.started_at,
        }
    }

    pub fn duration(&self) -> Duration {
        self.ended_at - self.started_at
    }

    /// Roll the evening up.
    ///
    /// Every list here is deduplicated and ordered, and that ordering is the
    /// point: a journal entry is a narrative, so the inputs arrive as a
    /// sequence rather than as a bag of counts.
    pub fn digest(&self) -> Digest {
        let mut route: Vec<Stop> = Vec::new();
        let mut campaigns: Vec<(String, Option<String>)> = Vec::new();
        let mut quests: Vec<Quest> = Vec::new();
        let mut premises: Vec<(String, Option<String>)> = Vec::new();
        let mut levels = Vec::new();
        let mut deaths: Vec<Death> = Vec::new();
        let mut felled = Vec::new();
        let mut lost_to = Vec::new();
        let mut rares: Vec<String> = Vec::new();
        let mut instances: Vec<(String, String)> = Vec::new();
        let mut keystones = Vec::new();
        let mut scenarios: Vec<String> = Vec::new();
        let mut achievements = Vec::new();
        let mut acquired = Vec::new();
        let mut loot = Vec::new();
        let mut sales = Vec::new();
        let mut practised: Vec<(String, u16)> = Vec::new();
        let mut equipped: Vec<Upgrade> = Vec::new();
        let mut appearances: Vec<String> = Vec::new();
        let mut learned: Vec<String> = Vec::new();
        let mut overheard: Vec<(String, String)> = Vec::new();
        let mut expired: Vec<String> = Vec::new();
        let mut cutscenes: Vec<(String, Option<u32>)> = Vec::new();
        let mut told: Vec<(String, String)> = Vec::new();
        let mut givers: BTreeMap<String, u32> = BTreeMap::new();
        let mut shots: Vec<(u32, String)> = Vec::new();
        let mut income: BTreeMap<Purpose, u64> = BTreeMap::new();
        let mut spending: BTreeMap<Purpose, u64> = BTreeMap::new();
        let mut crafted: BTreeMap<String, u32> = BTreeMap::new();
        let mut flights = 0u32;
        let mut companions: BTreeSet<String> = BTreeSet::new();
        // When the stop currently at the end of the route was arrived at.
        let mut entered = 0u32;

        for moment in &self.moments {
            match &moment.what {
                Happening::Arrived { zone, subzone, .. } => {
                    if let Some(last) = route.last_mut() {
                        // Still in the same zone: a subzone crossing, which is
                        // detail on this stop rather than a new one.
                        if &last.zone == zone {
                            if let Some(subzone) = subzone {
                                if !last.within.contains(subzone) {
                                    last.within.push(subzone.clone());
                                }
                            }
                            continue;
                        }
                        // Close the stop being left, so the route carries dwell
                        // times and not only an order.
                        last.stayed = moment.at.saturating_sub(entered);
                    }
                    entered = moment.at;
                    route.push(Stop {
                        zone: zone.clone(),
                        within: subzone.clone().into_iter().collect(),
                        stayed: 0,
                    });
                }
                Happening::Accepted { title, premise } => {
                    premises.push((title.clone(), premise.clone()));
                }
                Happening::Completed {
                    quest,
                    title,
                    story,
                } => {
                    // The premise came from accepting it, which may have been
                    // this session or an earlier one. Matched by title because
                    // the accept event has no id — the quest log gives one only
                    // once the quest is in it.
                    let premise = premises
                        .iter()
                        .find(|(taken, _)| taken == title)
                        .and_then(|(_, premise)| premise.clone());
                    quests.push(Quest {
                        id: *quest,
                        title: title.clone(),
                        premise,
                        story: story.clone(),
                        money: 0,
                    });
                }
                // Itemisation only. The money is the ledger's — counting it
                // here as well would put every quest reward in the books twice.
                Happening::Paid { quest, money, .. } => {
                    if let Some(entry) = quests.iter_mut().rev().find(|q| q.id == *quest) {
                        entry.money = *money;
                    }
                }
                Happening::Campaign { name, summary } => {
                    if !campaigns.iter().any(|(seen, _)| seen == name) {
                        campaigns.push((name.clone(), summary.clone()));
                    }
                }
                Happening::Levelled { level, zone } => levels.push((*level, zone.clone())),
                Happening::Died { zone, subzone, to } => deaths.push(Death {
                    zone: zone.clone(),
                    subzone: subzone.clone(),
                    to: to.clone(),
                }),
                Happening::Entered { name, kind, .. } => {
                    let entry = (name.clone(), kind.clone());
                    if !instances.contains(&entry) {
                        instances.push(entry);
                    }
                }
                Happening::Keystone {
                    dungeon,
                    level,
                    in_time,
                    upgrades,
                    seconds,
                } => keystones.push(Keystone {
                    dungeon: dungeon.clone(),
                    level: *level,
                    in_time: *in_time,
                    upgrades: *upgrades,
                    seconds: *seconds,
                }),
                Happening::Scenario { name, tier } => {
                    let said = match tier {
                        Some(tier) => format!("{name} ({tier})"),
                        None => name.clone(),
                    };
                    if !scenarios.contains(&said) {
                        scenarios.push(said);
                    }
                }
                Happening::Rare { name, .. } => {
                    if !rares.contains(name) {
                        rares.push(name.clone());
                    }
                }
                Happening::Practised { profession, skill } => {
                    // The best reached, not every step: an evening of
                    // herbalism is thirty skill-ups and one fact.
                    match practised.iter_mut().find(|(seen, _)| seen == profession) {
                        Some(entry) => entry.1 = entry.1.max(*skill),
                        None => practised.push((profession.clone(), *skill)),
                    }
                }
                Happening::Equipped {
                    name,
                    item_level,
                    gained,
                } => equipped.push(Upgrade {
                    name: name.clone(),
                    item_level: *item_level,
                    gained: *gained,
                    // Filled in below, once the whole evening is known: what
                    // dropped a piece happened before it was worn, and this
                    // pass is still in front of it.
                    from: None,
                }),
                Happening::Said { who, line } => {
                    overheard.push((who.clone(), line.clone()));
                }
                Happening::Gave { who, .. } => {
                    *givers.entry(who.clone()).or_default() += 1;
                }
                Happening::Told { who, line } => {
                    told.push((who.clone(), line.clone()));
                }
                Happening::Cutscene { zone, movie } => {
                    cutscenes.push((zone.clone(), *movie));
                }
                Happening::Expired { what } => {
                    if !expired.contains(what) {
                        expired.push(what.clone());
                    }
                }
                Happening::Learned { name } => {
                    if !learned.contains(name) {
                        learned.push(name.clone());
                    }
                }
                Happening::Appearance { name } => {
                    if !appearances.contains(name) {
                        appearances.push(name.clone());
                    }
                }
                Happening::Coin {
                    purpose,
                    amount,
                    incoming,
                } => {
                    let book = if *incoming {
                        &mut income
                    } else {
                        &mut spending
                    };
                    *book.entry(*purpose).or_default() += amount;
                }
                Happening::Crafted { name, .. } => {
                    *crafted.entry(name.clone()).or_default() += 1;
                }
                Happening::Flew { .. } => flights += 1,
                Happening::Pictured { what, subject } => shots.push((
                    moment.at,
                    if subject.is_empty() {
                        what.clone()
                    } else {
                        format!("{what}: {subject}")
                    },
                )),
                Happening::Felled { name } => {
                    if !felled.contains(name) {
                        felled.push(name.clone());
                    }
                }
                Happening::Fought { name, won } => {
                    if *won {
                        if !felled.contains(name) {
                            felled.push(name.clone());
                        }
                    } else if !lost_to.contains(name) {
                        lost_to.push(name.clone());
                    }
                }
                Happening::Earned { achievement, name } => {
                    achievements.push((*achievement, name.clone()))
                }
                Happening::Acquired { kind, name } => acquired.push((*kind, name.clone())),
                Happening::Looted {
                    item,
                    name,
                    quality,
                } => loot.push((*item, name.clone(), *quality)),
                // Same: the item names, not the money.
                Happening::Sold { subject, money } => sales.push((subject.clone(), *money)),
                Happening::Alongside { name } => {
                    companions.insert(name.clone());
                }
            }
        }

        // The last stop ran to the end of the session.
        if let Some(last) = route.last_mut() {
            last.stayed = (self.duration().num_seconds().max(0) as u32).saturating_sub(entered);
        }

        // A boss that was both won and lost against was, on balance, killed.
        lost_to.retain(|name| !felled.contains(name));
        // A rare that also came through as a boss kill is one thing, not two.
        rares.retain(|name| !felled.contains(name));
        // The upgrade that mattered is the biggest jump, so it leads.
        for upgrade in &mut equipped {
            upgrade.from = self.where_it_came_from(&upgrade.name);
        }
        equipped.sort_by_key(|upgrade| std::cmp::Reverse(upgrade.gained));

        // Biggest first in both books, because the question a person asks of a
        // ledger is "where did it mostly go".
        let mut income: Vec<(Purpose, u64)> = income.into_iter().collect();
        income.sort_by_key(|(_, amount)| std::cmp::Reverse(*amount));
        let mut spending: Vec<(Purpose, u64)> = spending.into_iter().collect();
        spending.sort_by_key(|(_, amount)| std::cmp::Reverse(*amount));
        let mut crafted: Vec<(String, u32)> = crafted.into_iter().collect();
        crafted.sort_by_key(|(_, made)| std::cmp::Reverse(*made));
        let mut questgivers: Vec<(String, u32)> = givers.into_iter().collect();
        questgivers.sort_by_key(|(_, given)| std::cmp::Reverse(*given));

        // Both read off the one set of books rather than summed a second time
        // from the events that itemise them.
        let of = |book: &[(Purpose, u64)], want: Purpose| {
            book.iter()
                .find(|(purpose, _)| *purpose == want)
                .map(|(_, amount)| *amount)
                .unwrap_or(0)
        };
        let quest_income = of(&income, Purpose::Quest);
        // A sale is what the auction house paid. Mail whose sender could not
        // be read is not counted as one — it may have been an alt moving the
        // account's own gold, which is not income to anybody.
        let sale_income = of(&income, Purpose::Sale);

        let turned_in: BTreeSet<&String> = quests.iter().map(|quest| &quest.title).collect();
        let taken_up: Vec<String> = premises
            .iter()
            .map(|(title, _)| title)
            .filter(|title| !turned_in.contains(*title))
            .cloned()
            .collect::<BTreeSet<String>>()
            .into_iter()
            .collect();

        Digest {
            character: self.character.clone(),
            display_name: self.display_name.clone(),
            realm_name: self.realm_name.clone(),
            class: self.class.clone(),
            race: self.race.clone(),
            faction: self.faction,
            started_at: self.started_at,
            ended_at: self.ended_at,
            route,
            campaigns,
            quests,
            taken_up,
            levels,
            deaths,
            felled,
            lost_to,
            rares,
            instances,
            keystones,
            scenarios,
            achievements,
            acquired,
            loot,
            sales,
            companions: companions.into_iter().collect(),
            practised,
            equipped,
            appearances,
            learned,
            overheard,
            expired,
            cutscenes,
            told,
            questgivers,
            income,
            spending,
            crafted,
            flights,
            shots,
            kills: self.kills,
            risen: self.risen.clone(),
            travelled: self.travelled,
            longest_fight: self.longest_fight,
            worst_hit: self.worst_hit,
            worst_hit_by: self.worst_hit_by.clone(),
            lowest_health: self.lowest_health,
            start_level: self.start_level,
            end_level: self.end_level,
            start_item_level: self.start_item_level,
            end_item_level: self.end_item_level,
            purse: self.end_money as i64 - self.start_money as i64,
            quest_income,
            sale_income,
        }
    }
}

/// What identifies a session everywhere it is stored or referred to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SessionId {
    pub character: CharacterKey,
    pub started_at: DateTime<Utc>,
}

impl Digest {
    pub fn id(&self) -> SessionId {
        SessionId {
            character: self.character.clone(),
            started_at: self.started_at,
        }
    }

    pub fn duration(&self) -> Duration {
        self.ended_at - self.started_at
    }

    /// Whether this evening is worth putting in front of somebody.
    ///
    /// Not a filter on what is recorded — everything is recorded — but on what
    /// is shown and on what an entry may be spent writing. Logging in to post
    /// an auction and logging out is not a chapter, and a journal whose first
    /// screen is nine of those is a journal nobody opens twice.
    pub fn is_worth_writing(&self) -> bool {
        if !self.quests.is_empty()
            || !self.levels.is_empty()
            || !self.felled.is_empty()
            || !self.lost_to.is_empty()
            || !self.rares.is_empty()
            || !self.keystones.is_empty()
            || !self.scenarios.is_empty()
            || !self.achievements.is_empty()
            || !self.acquired.is_empty()
            || !self.loot.is_empty()
            || !self.sales.is_empty()
            || !self.appearances.is_empty()
            || !self.learned.is_empty()
            || !self.crafted.is_empty()
            || !self.risen.is_empty()
        {
            return true;
        }
        // Nothing happened that the game raised an event for — but an hour
        // wandering three zones is still an evening, and somebody who spent it
        // that way probably has more to say about it than the log does.
        self.duration().num_seconds() >= IDLE && self.route.len() > 1
    }

    /// The one line a card leads with before there is any prose.
    pub fn headline(&self) -> String {
        let mut parts = Vec::new();
        // A keystone leads, because "+18 Halls of Atonement" is what the
        // evening was and the zone list is where it happened.
        if let Some(key) = self.keystones.first() {
            parts.push(format!("+{} {}", key.level, key.dungeon));
        } else if let Some(first) = self.route.first() {
            match self.route.len() {
                1 => parts.push(first.zone.clone()),
                length => parts.push(format!("{} and {} more", first.zone, length - 1)),
            }
        }
        // The storyline, when there was one and only one — two campaigns in an
        // evening is not a headline, it is a list.
        if self.campaigns.len() == 1 {
            parts.push(self.campaigns[0].0.clone());
        }
        if !self.quests.is_empty() {
            parts.push(plural(self.quests.len(), "quest", "quests"));
        }
        if let Some((level, _)) = self.levels.last() {
            parts.push(format!("level {level}"));
        }
        if !self.felled.is_empty() {
            parts.push(plural(self.felled.len(), "boss", "bosses"));
        }
        if !self.acquired.is_empty() {
            parts.push(plural(self.acquired.len(), "new thing", "new things"));
        }
        if parts.is_empty() {
            parts.push("a quiet hour".into());
        }
        parts.join(" · ")
    }

    /// Match the addon's screenshot moments to the files the client wrote.
    ///
    /// **The addon cannot know the filename.** `Screenshot()` writes a
    /// timestamped file into the client's own folder and returns nothing, and
    /// there is no API that reports what it was called. So the addon records
    /// when it fired and this joins the two by time.
    ///
    /// `taken` is the files it found, as `(when, path)`. A file within
    /// [`SHUTTER`] of a recorded moment is that moment's picture; anything else
    /// is a screenshot the person took themselves, which is not ours to claim.
    /// The window is generous in one direction only — the client writes the
    /// file after the addon asked, never before.
    pub fn pictures(&self, taken: &[(DateTime<Utc>, String)]) -> Vec<Picture> {
        self.shots
            .iter()
            .filter_map(|(at, subject)| {
                let asked = self.started_at + Duration::seconds(i64::from(*at));
                let file = taken
                    .iter()
                    .filter(|(when, _)| {
                        *when >= asked && (*when - asked) <= Duration::seconds(SHUTTER)
                    })
                    .min_by_key(|(when, _)| *when)?;
                Some(Picture {
                    subject: subject.clone(),
                    taken_at: file.0,
                    path: file.1.clone(),
                })
            })
            .collect()
    }

    /// Where to read more, for the things this evening touched.
    ///
    /// Links, never fetches. Wowhead's terms forbid automated access and name
    /// `ClaudeBot` in robots.txt; `warcraft.wiki.gg` disallows `/api.php` and
    /// `/rest.php` for every user agent and names `ClaudeBot` too. Deep-linking
    /// to a page a person then chooses to open is what both of those are for,
    /// and it is the same call this project already made for collections.
    ///
    /// The video channels are search URLs rather than specific videos on
    /// purpose: a link to a playlist rots when the playlist is re-cut, and a
    /// search for the zone keeps working.
    pub fn further_reading(&self) -> Vec<Link> {
        let mut links = Vec::new();

        for quest in self.quests.iter().take(8) {
            links.push(Link {
                label: quest.title.clone(),
                sort: Reading::Quest,
                url: format!("https://www.wowhead.com/quest={}", quest.id),
            });
        }

        for stop in self.route.iter().take(4) {
            links.push(Link {
                label: stop.zone.clone(),
                sort: Reading::Zone,
                url: format!("https://warcraft.wiki.gg/wiki/{}", wiki_title(&stop.zone)),
            });
            links.push(Link {
                label: format!("{} lore — Nobbel87", stop.zone),
                sort: Reading::Watch,
                url: format!(
                    "https://www.youtube.com/@Nobbel87/search?query={}",
                    encode(&stop.zone)
                ),
            });
            links.push(Link {
                label: format!("{} — The Karazhan Library", stop.zone),
                sort: Reading::Watch,
                url: format!(
                    "https://www.youtube.com/@TheKarazhanLibrary/search?query={}",
                    encode(&stop.zone)
                ),
            });
        }

        for (id, name) in self.achievements.iter().take(4) {
            links.push(Link {
                label: name.clone(),
                sort: Reading::Achievement,
                url: format!("https://www.wowhead.com/achievement={id}"),
            });
        }

        links
    }
}

/// A screenshot the addon asked for, matched to the file the client wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Picture {
    /// What it is a picture of, as the addon described it at the time.
    pub subject: String,
    pub taken_at: DateTime<Utc>,
    pub path: String,
}

/// Somewhere worth sending a person, and what sort of place it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub label: String,
    pub sort: Reading,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Reading {
    Quest,
    Zone,
    Achievement,
    Watch,
}

impl Reading {
    pub fn label(self) -> &'static str {
        match self {
            Reading::Quest => "Quest",
            Reading::Zone => "Zone",
            Reading::Achievement => "Achievement",
            Reading::Watch => "Watch",
        }
    }
}

/// A written-up evening.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub session: SessionId,
    pub title: String,
    /// Markdown, as the model wrote it.
    pub body: String,
    /// Which model wrote it, recorded because prose written by one is not
    /// interchangeable with prose written by another and a journal that spans
    /// years will span several.
    pub model: String,
    pub written_at: DateTime<Utc>,
}

/// Copper as a person says it: `1,204g 30s 05c`.
pub fn money(copper: u64) -> String {
    let gold = copper / 10_000;
    let silver = (copper % 10_000) / 100;
    let bronze = copper % 100;
    if gold > 0 {
        format!("{}g {silver:02}s {bronze:02}c", thousands(gold))
    } else if silver > 0 {
        format!("{silver}s {bronze:02}c")
    } else {
        format!("{bronze}c")
    }
}

/// The same, signed, for a delta.
pub fn purse(copper: i64) -> String {
    if copper < 0 {
        format!("−{}", money(copper.unsigned_abs()))
    } else {
        format!("+{}", money(copper as u64))
    }
}

fn thousands(number: u64) -> String {
    let digits = number.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// A reputation rank as its name.
///
/// The game's own order, one-based, as `C_Reputation` reports it. Anything
/// outside it — a paragon level, or a rank Blizzard adds later — is described
/// rather than guessed at: a wrong standing is a wrong claim about somebody's
/// play, which is the one thing this whole feature must not do.
///
/// Here rather than beside either caller, because there are two of them — the
/// brief and the card — and two tables is how "Revered" and "rank 7" end up on
/// one screen.
pub fn standing(rank: u8) -> String {
    match rank {
        1 => "Hated".into(),
        2 => "Hostile".into(),
        3 => "Unfriendly".into(),
        4 => "Neutral".into(),
        5 => "Friendly".into(),
        6 => "Honored".into(),
        7 => "Revered".into(),
        8 => "Exalted".into(),
        other => format!("rank {other}"),
    }
}

/// `1 quest`, `3 quests`.
///
/// Public because the page counts the same things in a different place, and two
/// pluralisers is how "1 quests" reaches a screen.
pub fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

/// How long something took, said the way a person would.
pub fn spell(duration: Duration) -> String {
    let minutes = duration.num_minutes().max(0);
    if minutes < 60 {
        return format!("{minutes} min");
    }
    let hours = minutes / 60;
    let rest = minutes % 60;
    if rest == 0 {
        format!("{hours} hr")
    } else {
        format!("{hours} hr {rest} min")
    }
}

/// A zone name as a wiki article title: spaces become underscores.
fn wiki_title(zone: &str) -> String {
    encode(&zone.replace(' ', "_"))
}

/// Percent-encode everything that is not safe in a URL path or query.
///
/// Small and local rather than a dependency: the only things going through it
/// are zone names, and the alternative is a crate to encode apostrophes in
/// "Zul'Drak".
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{}@{}",
            self.character.realm_slug,
            self.character.name,
            self.started_at.to_rfc3339()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(seconds: u32, what: Happening) -> Moment {
        Moment { at: seconds, what }
    }

    fn session(moments: Vec<Moment>) -> Session {
        Session {
            character: CharacterKey::new("emerald-dream", "Somechar"),
            display_name: "Somechar".into(),
            realm_name: "Emerald Dream".into(),
            class: "Druid".into(),
            race: "Tauren".into(),
            faction: Faction::Horde,
            started_at: Utc.with_ymd_and_hms(2026, 8, 3, 19, 0, 0).unwrap(),
            ended_at: Utc.with_ymd_and_hms(2026, 8, 3, 21, 30, 0).unwrap(),
            start_level: 70,
            end_level: 71,
            start_money: 1_000_000,
            end_money: 1_250_000,
            start_item_level: 600,
            end_item_level: 604,
            moments,
            kills: 0,
            risen: Vec::new(),
            travelled: 0,
            longest_fight: 0,
            worst_hit: 0,
            worst_hit_by: None,
            lowest_health: 100,
        }
    }

    #[test]
    fn a_route_keeps_its_order_and_collects_its_subzones() {
        // Orgrimmar → Durotar → Orgrimmar is a route. Collapsing it to a set
        // loses the shape of the evening, which is the one thing a journal
        // entry is made of.
        let digest = session(vec![
            at(
                0,
                Happening::Arrived {
                    zone: "Orgrimmar".into(),
                    subzone: None,
                    map: None,
                },
            ),
            at(
                600,
                Happening::Arrived {
                    zone: "Durotar".into(),
                    subzone: Some("Razor Hill".into()),
                    map: None,
                },
            ),
            at(
                900,
                Happening::Arrived {
                    zone: "Durotar".into(),
                    subzone: Some("Echo Isles".into()),
                    map: None,
                },
            ),
            at(
                1800,
                Happening::Arrived {
                    zone: "Orgrimmar".into(),
                    subzone: None,
                    map: None,
                },
            ),
        ])
        .digest();

        let zones: Vec<&str> = digest.route.iter().map(|s| s.zone.as_str()).collect();
        assert_eq!(zones, ["Orgrimmar", "Durotar", "Orgrimmar"]);
        assert_eq!(digest.route[1].within, ["Razor Hill", "Echo Isles"]);
    }

    #[test]
    fn a_quest_carries_the_text_the_player_actually_read() {
        // The whole reason the addon exists for this feature. No endpoint
        // returns these sentences and no summary elsewhere improves on them.
        let digest = session(vec![
            at(
                10,
                Happening::Accepted {
                    title: "The Battle for Gilneas".into(),
                    premise: Some("The Forsaken are at the wall.".into()),
                },
            ),
            at(
                400,
                Happening::Completed {
                    quest: 12345,
                    title: "The Battle for Gilneas".into(),
                    story: Some("You have done Gilneas proud.".into()),
                },
            ),
            at(
                400,
                Happening::Paid {
                    quest: 12345,
                    money: 45_000,
                    experience: 1200,
                },
            ),
        ])
        .digest();

        assert_eq!(digest.quests.len(), 1);
        assert_eq!(
            digest.quests[0].premise.as_deref(),
            Some("The Forsaken are at the wall.")
        );
        assert_eq!(
            digest.quests[0].story.as_deref(),
            Some("You have done Gilneas proud.")
        );
        // The quest itemises its own reward. The *total* is the ledger's, and
        // this fixture has no ledger rows — see the dedicated test for that.
        assert_eq!(digest.quests[0].money, 45_000);
        assert_eq!(digest.quest_income, 0);
    }

    #[test]
    fn a_quest_taken_and_not_finished_is_reported_as_left_hanging() {
        let digest = session(vec![
            at(
                10,
                Happening::Accepted {
                    title: "Into the Maw".into(),
                    premise: None,
                },
            ),
            at(
                20,
                Happening::Accepted {
                    title: "A Simple Errand".into(),
                    premise: None,
                },
            ),
            at(
                30,
                Happening::Completed {
                    quest: 1,
                    title: "A Simple Errand".into(),
                    story: None,
                },
            ),
        ])
        .digest();

        assert_eq!(digest.taken_up, ["Into the Maw"]);
    }

    #[test]
    fn a_boss_wiped_on_and_then_killed_counts_as_killed() {
        // Otherwise the evening reads as a defeat it recovered from, which is
        // the opposite of what happened.
        let digest = session(vec![
            at(
                100,
                Happening::Fought {
                    name: "Sire Denathrius".into(),
                    won: false,
                },
            ),
            at(
                200,
                Happening::Fought {
                    name: "Sire Denathrius".into(),
                    won: false,
                },
            ),
            at(
                300,
                Happening::Fought {
                    name: "Sire Denathrius".into(),
                    won: true,
                },
            ),
        ])
        .digest();

        assert_eq!(digest.felled, ["Sire Denathrius"]);
        assert!(digest.lost_to.is_empty());
    }

    #[test]
    fn a_wipe_with_no_kill_after_it_is_still_a_story() {
        let digest = session(vec![at(
            100,
            Happening::Fought {
                name: "Fyrakk".into(),
                won: false,
            },
        )])
        .digest();

        assert_eq!(digest.lost_to, ["Fyrakk"]);
        assert!(digest.felled.is_empty());
        assert!(digest.is_worth_writing());
    }

    #[test]
    fn checking_the_mail_is_not_an_evening() {
        // The filter that stops a journal becoming a login log.
        let mut quiet = session(vec![at(
            0,
            Happening::Arrived {
                zone: "Orgrimmar".into(),
                subzone: None,
                map: None,
            },
        )]);
        quiet.ended_at = quiet.started_at + Duration::minutes(4);
        assert!(!quiet.digest().is_worth_writing());
    }

    #[test]
    fn a_long_wander_with_no_events_is_still_an_evening() {
        // Somebody who spent an hour crossing three zones without the client
        // raising a single event has more to say about it than the log does.
        let mut wander = session(vec![
            at(
                0,
                Happening::Arrived {
                    zone: "Nagrand".into(),
                    subzone: None,
                    map: None,
                },
            ),
            at(
                1800,
                Happening::Arrived {
                    zone: "Zangarmarsh".into(),
                    subzone: None,
                    map: None,
                },
            ),
        ]);
        wander.ended_at = wander.started_at + Duration::minutes(50);
        assert!(wander.digest().is_worth_writing());
    }

    #[test]
    fn a_storyline_is_named_once_however_many_of_its_chapters_were_finished() {
        // The reason campaigns are captured at all: eight turn-ins are eight
        // titles until something says they were one story. Repeating that
        // story eight times would bury it again.
        let digest = session(vec![
            at(
                10,
                Happening::Campaign {
                    name: "The Severed Threads".into(),
                    summary: Some("The Nerubians are not finished.".into()),
                },
            ),
            at(
                20,
                Happening::Campaign {
                    name: "The Severed Threads".into(),
                    summary: None,
                },
            ),
        ])
        .digest();

        assert_eq!(digest.campaigns.len(), 1);
        // The first mention wins, and it is the one that carried the summary.
        assert_eq!(
            digest.campaigns[0].1.as_deref(),
            Some("The Nerubians are not finished.")
        );
    }

    #[test]
    fn a_death_carries_what_did_it_where_the_log_caught_it() {
        // "Died to a Gorian Warlock at Halaa" is a story beat. "Died in
        // Nagrand" is a coordinate.
        let digest = session(vec![
            at(
                100,
                Happening::Died {
                    zone: "Nagrand".into(),
                    subzone: Some("Halaa".into()),
                    to: Some("Gorian Warlock".into()),
                },
            ),
            at(
                200,
                Happening::Died {
                    zone: "Nagrand".into(),
                    subzone: None,
                    // A fall, a drowning, or a death nothing was blamed for.
                    // Naming a culprit for it would be inventing one.
                    to: None,
                },
            ),
        ])
        .digest();

        assert_eq!(digest.deaths[0].to.as_deref(), Some("Gorian Warlock"));
        assert_eq!(digest.deaths[1].to, None);
    }

    #[test]
    fn a_keystone_leads_the_headline_because_it_is_what_the_evening_was() {
        let digest = session(vec![
            at(
                0,
                Happening::Arrived {
                    zone: "Dornogal".into(),
                    subzone: None,
                    map: None,
                },
            ),
            at(
                60,
                Happening::Entered {
                    name: "Ara-Kara, City of Echoes".into(),
                    kind: "party, Mythic Keystone".into(),
                    group: 5,
                },
            ),
            at(
                2000,
                Happening::Keystone {
                    dungeon: "Ara-Kara, City of Echoes".into(),
                    level: 18,
                    in_time: true,
                    upgrades: 2,
                    seconds: 1_620,
                },
            ),
        ])
        .digest();

        assert!(
            digest.headline().starts_with("+18 Ara-Kara"),
            "{}",
            digest.headline()
        );
        assert_eq!(digest.instances.len(), 1);
        assert!(digest.keystones[0].in_time);
    }

    #[test]
    fn a_profession_reports_the_best_it_reached_rather_than_every_step() {
        // An afternoon of herbalism is thirty skill-ups and one fact.
        let digest = session(vec![
            at(
                10,
                Happening::Practised {
                    profession: "Alchemy".into(),
                    skill: 84,
                },
            ),
            at(
                20,
                Happening::Practised {
                    profession: "Alchemy".into(),
                    skill: 91,
                },
            ),
        ])
        .digest();

        assert_eq!(digest.practised, [("Alchemy".to_string(), 91)]);
    }

    #[test]
    fn the_biggest_gear_upgrade_leads() {
        // The one that mattered is the biggest jump, not the last one worn.
        let digest = session(vec![
            at(
                10,
                Happening::Equipped {
                    name: "Cloak of Small Favours".into(),
                    item_level: 604,
                    gained: 2,
                },
            ),
            at(
                20,
                Happening::Equipped {
                    name: "Sureki Zealot's Insignia".into(),
                    item_level: 639,
                    gained: 26,
                },
            ),
        ])
        .digest();

        assert_eq!(digest.equipped[0].name, "Sureki Zealot's Insignia");
    }

    #[test]
    fn a_rare_that_was_also_a_boss_kill_is_one_thing() {
        let digest = session(vec![
            at(
                10,
                Happening::Felled {
                    name: "Doomwalker".into(),
                },
            ),
            at(
                10,
                Happening::Rare {
                    name: "Doomwalker".into(),
                    rank: "worldboss".into(),
                },
            ),
        ])
        .digest();

        assert_eq!(digest.felled, ["Doomwalker"]);
        assert!(digest.rares.is_empty());
    }

    #[test]
    fn a_piece_of_gear_is_credited_to_what_dropped_it_and_to_nothing_else() {
        // Nothing in the game connects an item to the thing that dropped it.
        // The evening's own record does, and only within a pull.
        let digest = session(vec![
            at(
                100,
                Happening::Felled {
                    name: "Rasha'nan".into(),
                },
            ),
            at(
                104,
                Happening::Looted {
                    item: 221_023,
                    name: "Wingcarver Sabatons".into(),
                    quality: 4,
                },
            ),
            at(
                120,
                Happening::Equipped {
                    name: "Wingcarver Sabatons".into(),
                    item_level: 639,
                    gained: 26,
                },
            ),
            // A boss half an hour ago is a different pull, and an item bought
            // rather than looted has no drop to name at all.
            at(
                200,
                Happening::Felled {
                    name: "Nexus-Princess Ky'veza".into(),
                },
            ),
            at(
                2_000,
                Happening::Equipped {
                    name: "Cloak of Small Favours".into(),
                    item_level: 604,
                    gained: 2,
                },
            ),
        ])
        .digest();

        let sabatons = digest
            .equipped
            .iter()
            .find(|gear| gear.name == "Wingcarver Sabatons")
            .expect("the sabatons");
        assert_eq!(sabatons.from.as_deref(), Some("Rasha'nan"));

        let cloak = digest
            .equipped
            .iter()
            .find(|gear| gear.name == "Cloak of Small Favours")
            .expect("the cloak");
        assert_eq!(cloak.from, None);
    }

    #[test]
    fn the_ledger_is_the_only_set_of_books_money_is_counted_in() {
        // A net purse says an evening cost forty gold. The books say it earned
        // three hundred questing and spent three hundred and forty at the
        // auction house, which is a different evening — and the same gold must
        // not appear in both a quest reward and a "found on the ground".
        let digest = session(vec![
            at(
                100,
                Happening::Completed {
                    quest: 9_923,
                    title: "Hero of the Mag'har".into(),
                    story: None,
                },
            ),
            at(
                100,
                Happening::Paid {
                    quest: 9_923,
                    money: 84_500,
                    experience: 0,
                },
            ),
            at(
                100,
                Happening::Coin {
                    purpose: Purpose::Quest,
                    amount: 84_500,
                    incoming: true,
                },
            ),
            at(
                200,
                Happening::Coin {
                    purpose: Purpose::Loot,
                    amount: 12_000,
                    incoming: true,
                },
            ),
            at(
                300,
                Happening::Coin {
                    purpose: Purpose::Bid,
                    amount: 400_000,
                    incoming: false,
                },
            ),
            at(
                310,
                Happening::Coin {
                    purpose: Purpose::Repair,
                    amount: 9_000,
                    incoming: false,
                },
            ),
        ])
        .digest();

        // Biggest first, both books.
        assert_eq!(
            digest.income,
            [(Purpose::Quest, 84_500), (Purpose::Loot, 12_000)]
        );
        assert_eq!(
            digest.spending,
            [(Purpose::Bid, 400_000), (Purpose::Repair, 9_000)]
        );
        // Counted once, off the ledger — the quest still itemises its reward.
        assert_eq!(digest.quest_income, 84_500);
        assert_eq!(digest.quests[0].money, 84_500);
    }

    #[test]
    fn gold_an_alt_sent_over_is_not_income_to_anybody() {
        // Three facts wearing one event. The auction house paying is income; an
        // alt sending gold is the account's own money moving and counting it
        // would let somebody earn the same gold on every character they own.
        let digest = session(vec![
            at(
                100,
                Happening::Coin {
                    purpose: Purpose::Sale,
                    amount: 250_000,
                    incoming: true,
                },
            ),
            at(
                200,
                Happening::Coin {
                    purpose: Purpose::Transfer,
                    amount: 5_000_000,
                    incoming: true,
                },
            ),
        ])
        .digest();

        assert_eq!(digest.sale_income, 250_000);
        // Present in the books, because it did arrive and the card should say
        // so — and absent from what the evening *earned*. Largest first, like
        // every other book.
        assert_eq!(
            digest.income,
            [(Purpose::Transfer, 5_000_000), (Purpose::Sale, 250_000)]
        );
    }

    #[test]
    fn a_purpose_reads_differently_depending_which_way_the_money_went() {
        // The same frame is two facts. Selling junk to a vendor and buying from
        // one are not the same line on a ledger.
        assert_eq!(Purpose::Vendor.label(true), "Sold to vendors");
        assert_eq!(Purpose::Vendor.label(false), "Bought from vendors");
        assert_eq!(Purpose::from_token("deposit"), Purpose::Deposit);
        // A token this version does not know is admitted, not filed under
        // something plausible.
        assert_eq!(Purpose::from_token("something-new"), Purpose::Unknown);
    }

    #[test]
    fn crafting_is_counted_per_recipe_because_nothing_in_the_game_does() {
        let digest = session(vec![
            at(
                10,
                Happening::Crafted {
                    recipe: 1,
                    name: "Flask of Alchemical Chaos".into(),
                },
            ),
            at(
                20,
                Happening::Crafted {
                    recipe: 1,
                    name: "Flask of Alchemical Chaos".into(),
                },
            ),
            at(
                30,
                Happening::Crafted {
                    recipe: 2,
                    name: "Algari Mana Potion".into(),
                },
            ),
        ])
        .digest();

        // Most-made first.
        assert_eq!(
            digest.crafted,
            [
                ("Flask of Alchemical Chaos".to_string(), 2),
                ("Algari Mana Potion".to_string(), 1)
            ]
        );
        assert!(digest.is_worth_writing());
    }

    #[test]
    fn a_screenshot_is_matched_to_the_moment_that_asked_for_it() {
        // The addon cannot learn the filename — `Screenshot()` returns nothing
        // and no API reports it — so the join is by time, and this is the
        // whole of it.
        let session = session(vec![at(
            600,
            Happening::Pictured {
                what: "achievement".into(),
                subject: "Loremaster of Kalimdor".into(),
            },
        )]);
        let asked = session.started_at + Duration::seconds(600);

        let taken = vec![
            // Somebody pressing Print Screen an hour earlier. Not ours.
            (session.started_at, "/wow/Screenshots/early.jpg".to_string()),
            // The one the addon asked for, written a couple of seconds later.
            (
                asked + Duration::seconds(2),
                "/wow/Screenshots/wanted.jpg".to_string(),
            ),
            // And one taken by hand well afterwards.
            (
                asked + Duration::seconds(600),
                "/wow/Screenshots/later.jpg".to_string(),
            ),
        ];

        let pictures = session.digest().pictures(&taken);
        assert_eq!(pictures.len(), 1);
        assert_eq!(pictures[0].path, "/wow/Screenshots/wanted.jpg");
        assert_eq!(pictures[0].subject, "achievement: Loremaster of Kalimdor");
    }

    #[test]
    fn a_picture_taken_before_the_moment_is_somebody_elses() {
        // The client cannot have written the file before the addon asked for
        // it, so an earlier one is a person pressing the key themselves — and
        // claiming it would put a stranger's screenshot in their journal.
        let session = session(vec![at(
            600,
            Happening::Pictured {
                what: "rare".into(),
                subject: "Time-Lost Proto-Drake".into(),
            },
        )]);
        let asked = session.started_at + Duration::seconds(600);
        let taken = vec![(asked - Duration::seconds(3), "/before.jpg".to_string())];

        assert!(session.digest().pictures(&taken).is_empty());
    }

    #[test]
    fn companions_are_deduplicated_because_the_roster_event_repeats() {
        // `GROUP_ROSTER_UPDATE` fires on every change, so a two-hour dungeon
        // run reports the same three names dozens of times.
        let digest = session(vec![
            at(
                1,
                Happening::Alongside {
                    name: "Velkurai".into(),
                },
            ),
            at(
                2,
                Happening::Alongside {
                    name: "Velkurai".into(),
                },
            ),
            at(
                3,
                Happening::Alongside {
                    name: "Aeltor".into(),
                },
            ),
        ])
        .digest();

        assert_eq!(digest.companions, ["Aeltor", "Velkurai"]);
    }

    #[test]
    fn a_purse_that_went_down_says_so() {
        let mut spent = session(vec![]);
        spent.start_money = 500_000;
        spent.end_money = 100_000;
        assert_eq!(spent.digest().purse, -400_000);
        assert_eq!(purse(-400_000), "−40g 00s 00c");
        assert_eq!(purse(400_000), "+40g 00s 00c");
    }

    #[test]
    fn money_reads_the_way_a_person_says_it() {
        assert_eq!(money(12_043_005), "1,204g 30s 05c");
        assert_eq!(money(4_205), "42s 05c");
        assert_eq!(money(7), "7c");
    }

    #[test]
    fn a_headline_summarises_before_anything_is_written() {
        // What the card says when there is no entry — and there may never be
        // one, because writing is opt-in and costs the user's own credit.
        let digest = session(vec![
            at(
                0,
                Happening::Arrived {
                    zone: "Nagrand".into(),
                    subzone: None,
                    map: None,
                },
            ),
            at(
                60,
                Happening::Arrived {
                    zone: "Shadowmoon Valley".into(),
                    subzone: None,
                    map: None,
                },
            ),
            at(
                120,
                Happening::Completed {
                    quest: 1,
                    title: "A Task".into(),
                    story: None,
                },
            ),
            at(
                200,
                Happening::Levelled {
                    level: 71,
                    zone: "Nagrand".into(),
                },
            ),
        ])
        .digest();

        assert_eq!(digest.headline(), "Nagrand and 1 more · 1 quest · level 71");
    }

    #[test]
    fn further_reading_links_out_and_never_fetches() {
        // Wowhead's terms forbid automated access and its robots.txt names
        // ClaudeBot; warcraft.wiki.gg disallows /api.php for everybody and
        // names ClaudeBot too. Deep-linking is a different act, and it is the
        // one this project already settled on for collections.
        let digest = session(vec![
            at(
                0,
                Happening::Arrived {
                    zone: "Zul'Drak".into(),
                    subzone: None,
                    map: None,
                },
            ),
            at(
                10,
                Happening::Completed {
                    quest: 12345,
                    title: "The Storm King's Vengeance".into(),
                    story: None,
                },
            ),
        ])
        .digest();

        let links = digest.further_reading();
        assert!(links
            .iter()
            .any(|link| link.url == "https://www.wowhead.com/quest=12345"));
        // The apostrophe has to be encoded or the wiki link lands nowhere.
        assert!(links
            .iter()
            .any(|link| link.url == "https://warcraft.wiki.gg/wiki/Zul%27Drak"));
        assert!(links
            .iter()
            .any(|link| link.sort == Reading::Watch && link.url.contains("Nobbel87")));
        // Nothing here is an endpoint anything would be fetched from.
        assert!(links.iter().all(|link| !link.url.contains("api.php")));
    }

    #[test]
    fn a_session_is_identified_by_a_character_and_a_start() {
        let session = session(vec![]);
        assert_eq!(
            session.id().to_string(),
            "emerald-dream/somechar@2026-08-03T19:00:00+00:00"
        );
    }

    #[test]
    fn a_spell_of_time_reads_in_hours_and_minutes() {
        assert_eq!(spell(Duration::minutes(45)), "45 min");
        assert_eq!(spell(Duration::minutes(120)), "2 hr");
        assert_eq!(spell(Duration::minutes(150)), "2 hr 30 min");
    }
}
