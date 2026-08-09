//! The one source here that is not a game database.
//!
//! Everything else under `source/` asks Blizzard what is true. This asks a
//! language model to write four hundred words about an evening that already
//! happened, from facts Armory already holds. It is the same seam as the rest:
//! a pure function builds the request, a pure function reads the answer, and
//! `ui::http` is what opens the socket.
//!
//! **It talks to a `llama-server` on this machine**, over llama.cpp's
//! OpenAI-compatible `/v1/chat/completions`, at `http://127.0.0.1:8080` unless
//! told otherwise. That is the same server the sibling application Familiar
//! drives, and pointing at it rather than at a hosted API changes three things
//! that are worth naming:
//!
//! * **No credential.** Nothing in the keyring, nothing to leak, nothing to
//!   register for. The one prerequisite is a server that is already running.
//! * **No bill.** Which is why writing an evening up can happen on its own —
//!   the reason to hold back was somebody's money, and there is none at stake.
//! * **Nothing leaves the machine.** A journal is a record of somebody's
//!   evenings, and the sentences in it stay on the computer that recorded them.
//!
//! **The model is given facts and no research errand.** Everything in the brief
//! came off the player's own screen — the quest text is the sentences the game
//! showed them. The model is asked to write about that and explicitly told not
//! to invent events. What it may add is framing: it knows who the Mag'har are,
//! and that is the difference between a log and a journal. Where a person wants
//! to read further, [`crate::chronicle::Digest::further_reading`] hands
//! them links rather than fetching anything.
//!
//! Non-streaming, unlike Familiar's transport. A journal entry appears all at
//! once on a card that is already showing a spinner; there is nobody watching
//! it arrive, so an SSE parser would be machinery in exchange for nothing.

use serde_json::{json, Value};

use super::{parse_json, Method, Outcome, Reason, Request, SourceId};
use crate::chronicle::{money, spell, standing, Digest};
use crate::tally;

const SOURCE: SourceId = SourceId::Journal;

/// Where a `llama-server` usually is. The same default Familiar uses.
pub const DEFAULT_SERVER: &str = "http://127.0.0.1:8080";

/// How long a journal entry may run to, in tokens.
///
/// Generous for four hundred words, because a thinking model spends some of it
/// reasoning before it writes anything.
const MAX_TOKENS: u32 = 2_048;

/// A written entry, as the model returned it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub title: String,
    pub body: String,
    /// Which model answered, as the server named it. Recorded on every entry
    /// because a journal that spans years will span several models, and prose
    /// from one is not interchangeable with prose from another.
    pub model: String,
}

/// Ask the server what it is running.
///
/// llama.cpp's own endpoint, and the only way to put a name on an entry: the
/// server serves whatever model it was launched with and ignores the `model`
/// field in a request entirely.
pub fn identify(server: &str) -> Request {
    Request::get(SOURCE, format!("{}/props", server.trim_end_matches('/')))
}

/// Read `/props` for the model's name.
///
/// Everything in that response is optional — a gateway in front of the server
/// may answer a shape of its own — so a missing name is a thing not shown
/// rather than a failure.
pub fn parse_identity(body: &[u8]) -> Option<String> {
    let props: Value = serde_json::from_slice(body).ok()?;
    props
        .get("model_alias")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            props
                .get("model_path")
                .and_then(Value::as_str)
                .and_then(|path| path.rsplit(['/', '\\']).next())
                .map(|file| file.trim_end_matches(".gguf").to_string())
        })
        .filter(|name| !name.is_empty())
}

/// How the entry should read.
///
/// A short list of ways to be wrong, rather than a long list of ways to be
/// right. The failure modes worth naming are the ones that make a journal
/// worthless: inventing an event that did not happen, and padding a quiet
/// evening into an epic.
const VOICE: &str = "\
You keep a travel journal for a character in World of Warcraft. You are given \
the log of one play session and you write that evening's entry.

Write in the character's own voice: first person, past tense, the register of \
somebody writing at the end of a long day rather than reciting a report. Two \
to four short paragraphs, 200-350 words. No headings, no bullet lists, no \
summary of statistics — the application already shows the numbers beside your \
entry and repeating them wastes the reader's attention.

Every event you mention must come from the log. Do not invent quests, people, \
places, kills, loot or outcomes, and do not imply an outcome the log does not \
record. If the log says a boss was fought and lost to, it was lost to.

You may use what you know of Warcraft's world to give the facts their setting \
— who a faction is, why a place matters, what a name means. That framing is \
what makes this a journal rather than a list. Keep it in service of the \
evening's own events; never let it become a lore essay the character was not \
part of.

Quest text in the log is what the game itself put on the screen. Treat it as \
the evening's source material and write from it.

The log has three kinds of dialogue and they are not worth the same. \
\"Overheard\" is what the world said unbidden — NPCs talking to each other, a \
boss mid-fight, an escort narrating itself — and it is the evening's \
atmosphere; use it freely. \"Spoken to you\" is what an NPC said when the \
character walked up and asked, and much of it is a shopkeeper's greeting or a \
flight master's patter that means nothing; take the lines that carry the \
evening and ignore the rest without remarking on them. A cutscene is listed \
only as having happened, because its contents cannot be read — do not describe \
one, though you may note that the character stood and watched something.

Some evenings are quiet. When the log is thin, write a short, honest entry \
about a quiet evening. Do not inflate it, and do not apologise for it.

The title is a short phrase, at most sixty characters, that could head a diary \
page. Not a summary sentence, and not the character's name.";

/// Ask for one entry.
pub fn write(server: &str, digest: &Digest) -> Request {
    let body = json!({
        // llama-server serves whatever it was launched with and ignores this,
        // but a gateway in front of it will not, so it is sent anyway.
        "model": "armory-chronicle",
        "max_tokens": MAX_TOKENS,
        "stream": false,
        // Low, because this is prose from a supplied set of facts rather than
        // a problem to solve, and because the failure mode that matters here is
        // invention — which more sampling entropy makes more likely, not less.
        "temperature": 0.7,
        // The shape is constrained rather than parsed out of prose. llama.cpp
        // compiles a JSON schema to a grammar and the sampler cannot leave it,
        // which is a stronger guarantee than a hosted API gives. A journal
        // entry contains its own blank lines and quotation marks, and "the
        // first line is the title" is a rule that breaks the first time a model
        // opens with dialogue.
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "name": "journal_entry",
                "strict": true,
                "schema": {
                    "type": "object",
                    "properties": {
                        "title": {
                            "type": "string",
                            "description": "A short diary-page heading, at most sixty characters."
                        },
                        "entry": {
                            "type": "string",
                            "description": "The journal entry itself, in the character's voice."
                        }
                    },
                    "required": ["title", "entry"],
                    "additionalProperties": false
                }
            }
        },
        "messages": [
            { "role": "system", "content": VOICE },
            { "role": "user", "content": brief(digest) },
        ],
    });

    Request {
        source: SOURCE,
        method: Method::Post,
        url: format!("{}/v1/chat/completions", server.trim_end_matches('/')),
        headers: vec![("content-type".into(), "application/json".into())],
        body: Some(body.to_string()),
    }
}

/// The evening, as the model is told about it.
///
/// A pure function over the digest and the only thing here worth testing hard:
/// it is the whole input to the entry, and everything the model is allowed to
/// say has to be visible in it.
pub fn brief(digest: &Digest) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "Character: {} of {}, a {} {} of the {}.\n",
        digest.display_name,
        digest.realm_name,
        digest.race.to_lowercase(),
        digest.class.to_lowercase(),
        digest.faction.label(),
    ));
    out.push_str(&format!(
        "Session: {} — {}, lasting {}.\n",
        digest.started_at.format("%A %-d %B %Y"),
        digest.started_at.format("%H:%M UTC"),
        spell(digest.duration()),
    ));

    if digest.end_level > digest.start_level {
        out.push_str(&format!(
            "Level {} at the start, {} at the end.\n",
            digest.start_level, digest.end_level
        ));
    } else {
        out.push_str(&format!("Level {}.\n", digest.end_level));
    }

    section(&mut out, "Where they went", &{
        digest
            .route
            .iter()
            .map(|stop| {
                let within = if stop.within.is_empty() {
                    String::new()
                } else {
                    format!(" (through {})", stop.within.join(", "))
                };
                format!(
                    "{}{}, about {}",
                    stop.zone,
                    within,
                    spell(chrono::Duration::seconds(i64::from(stop.stayed)))
                )
            })
            .collect::<Vec<_>>()
    });

    section(
        &mut out,
        "Instances entered",
        &digest
            .instances
            .iter()
            .map(|(name, kind)| format!("{name} ({kind})"))
            .collect::<Vec<_>>(),
    );
    section(
        &mut out,
        "Mythic keystones",
        &digest
            .keystones
            .iter()
            .map(|key| {
                format!(
                    "{} at +{}, {} in {}{}",
                    key.dungeon,
                    key.level,
                    if key.in_time {
                        "timed"
                    } else {
                        "over the timer"
                    },
                    spell(chrono::Duration::seconds(i64::from(key.seconds))),
                    match key.upgrades {
                        0 => String::new(),
                        n => format!(", key up {n}"),
                    }
                )
            })
            .collect::<Vec<_>>(),
    );
    section(&mut out, "Scenarios and delves finished", &digest.scenarios);

    // Before the quests, because it is the frame they hang in: the model has to
    // know that eight turn-ins were chapters of one story before it reads them
    // as eight errands.
    if !digest.campaigns.is_empty() {
        out.push_str("\nStorylines these quests belong to:\n");
        for (name, summary) in &digest.campaigns {
            out.push_str(&format!("- {name}\n"));
            if let Some(summary) = summary {
                out.push_str(&format!("    {summary}\n"));
            }
        }
    }

    if !digest.quests.is_empty() {
        out.push_str(
            "\nQuests completed, in order. The quoted text is what the game showed on screen:\n",
        );
        for quest in &digest.quests {
            out.push_str(&format!("- \"{}\"", quest.title));
            if quest.money > 0 {
                out.push_str(&format!(" (paid {})", money(quest.money)));
            }
            out.push('\n');
            if let Some(premise) = &quest.premise {
                out.push_str(&format!("    asked for: {premise}\n"));
            }
            if let Some(story) = &quest.story {
                out.push_str(&format!("    on finishing: {story}\n"));
            }
        }
    }

    section(&mut out, "Quests taken and not finished", &digest.taken_up);
    section(
        &mut out,
        "Levels gained",
        &digest
            .levels
            .iter()
            .map(|(level, zone)| format!("reached {level} in {zone}"))
            .collect::<Vec<_>>(),
    );
    section(&mut out, "Bosses defeated", &digest.felled);
    section(
        &mut out,
        "Fought and lost to (no kill followed)",
        &digest.lost_to,
    );
    section(&mut out, "Rares and world bosses killed", &digest.rares);
    section(
        &mut out,
        "Deaths",
        &digest
            .deaths
            .iter()
            .map(|death| {
                let mut line = format!("died in {}", death.zone);
                if let Some(within) = &death.subzone {
                    line.push_str(&format!(" ({within})"));
                }
                // Named only where the combat log caught it. A death with
                // nothing blamed is a fall or a drowning, and inventing a
                // culprit for it is exactly the sort of thing the model is
                // being told not to do.
                if let Some(to) = &death.to {
                    line.push_str(&format!(", killed by {to}"));
                }
                line
            })
            .collect::<Vec<_>>(),
    );
    section(
        &mut out,
        "Achievements earned",
        &digest
            .achievements
            .iter()
            .map(|(_, name)| name.clone())
            .collect::<Vec<_>>(),
    );
    section(
        &mut out,
        "Added to the collection",
        &digest
            .acquired
            .iter()
            .map(|(kind, name)| format!("{name} (a {})", kind.label()))
            .collect::<Vec<_>>(),
    );
    section(
        &mut out,
        "Notable loot",
        &digest
            .loot
            .iter()
            .map(|(_, name, quality)| format!("{name} ({})", quality_name(*quality)))
            .collect::<Vec<_>>(),
    );
    section(
        &mut out,
        "Sold at auction",
        &digest
            .sales
            .iter()
            .map(|(subject, amount)| format!("{subject} — {}", money(*amount)))
            .collect::<Vec<_>>(),
    );
    section(
        &mut out,
        "Reputation ranks reached",
        &digest
            .risen
            .iter()
            .map(|(name, rank)| format!("{name} — now {}", standing(*rank)))
            .collect::<Vec<_>>(),
    );
    section(
        &mut out,
        "Gear upgraded",
        &digest
            .equipped
            .iter()
            .take(5)
            .map(|gear| match &gear.from {
                // The model is told what dropped it, because that is the whole
                // difference between a list of loot and a story about an
                // evening.
                Some(source) => format!(
                    "{} (item level {}, up {}, off {source})",
                    gear.name, gear.item_level, gear.gained
                ),
                None => format!(
                    "{} (item level {}, up {})",
                    gear.name, gear.item_level, gear.gained
                ),
            })
            .collect::<Vec<_>>(),
    );
    section(
        &mut out,
        "Professions improved",
        &digest
            .practised
            .iter()
            .map(|(name, skill)| format!("{name} now at {skill}"))
            .collect::<Vec<_>>(),
    );
    section(&mut out, "New appearances collected", &digest.appearances);
    // Written by Blizzard, read by the player, and available nowhere else. The
    // same argument as the quest text: this is what the evening actually
    // sounded like, and a model given it stops having to invent atmosphere.
    section(
        &mut out,
        "Overheard",
        &digest
            .overheard
            .iter()
            .map(|(who, line)| {
                if who.is_empty() {
                    line.clone()
                } else {
                    format!("{who}: \"{line}\"")
                }
            })
            .collect::<Vec<_>>(),
    );
    // Told plainly rather than described. A cutscene is a strong signal that
    // something narratively significant happened here, and the model should
    // weight the quest text around it accordingly — but Armory cannot read a
    // cutscene's contents, so saying more would be inventing it.
    section(
        &mut out,
        "Cutscenes that played (contents unknown — the dialogue, if any, is under Overheard)",
        &digest
            .cutscenes
            .iter()
            .map(|(zone, _)| format!("one in {zone}"))
            .collect::<Vec<_>>(),
    );
    section(
        &mut out,
        "Spoken to you (much of this is functional — use only what carries the evening)",
        &digest
            .told
            .iter()
            .map(|(who, line)| {
                if who.is_empty() {
                    line.clone()
                } else {
                    format!("{who}: \"{line}\"")
                }
            })
            .collect::<Vec<_>>(),
    );
    // Named, because a quest giver is a person the character met and the entry
    // reads completely differently for it — "Khadgar sent me" against "I was
    // sent". They also recur across a career, which is the whole reason the
    // addon records the name and not only the creature id.
    section(
        &mut out,
        "Who sent you out",
        &digest
            .questgivers
            .iter()
            .map(|(who, given)| match given {
                1 => who.clone(),
                given => format!("{who} ({given} quests)"),
            })
            .collect::<Vec<_>>(),
    );
    section(&mut out, "Recipes learned", &digest.learned);
    section(&mut out, "In the party", &digest.companions);

    section(
        &mut out,
        "Money in",
        &digest
            .income
            .iter()
            .map(|(purpose, amount)| format!("{}: {}", purpose.label(true), money(*amount)))
            .collect::<Vec<_>>(),
    );
    section(
        &mut out,
        "Money out",
        &digest
            .spending
            .iter()
            .map(|(purpose, amount)| format!("{}: {}", purpose.label(false), money(*amount)))
            .collect::<Vec<_>>(),
    );
    section(
        &mut out,
        "Made at the workbench",
        &digest
            .crafted
            .iter()
            .map(|(name, made)| match made {
                1 => name.clone(),
                made => format!("{name} ×{made}"),
            })
            .collect::<Vec<_>>(),
    );

    if digest.kills > 0 {
        out.push_str(&format!(
            "\nAbout {} enemies were killed over the session, which is a measure of how \
             busy it was rather than a list worth naming.\n",
            digest.kills
        ));
    }

    // The three numbers that are a sentence rather than a statistic. The
    // longest fight is what separates a boss that took eleven minutes from an
    // evening of six-second pulls, and the other two are the near-death the
    // character would actually be thinking about afterwards.
    if digest.longest_fight >= 60 {
        out.push_str(&format!(
            "\nThe longest single fight lasted {}.\n",
            tally::spent(u64::from(digest.longest_fight))
        ));
    }
    if digest.lowest_health < 35 {
        out.push_str(&match &digest.worst_hit_by {
            Some(who) => format!(
                "\nThe closest call of the evening was {}% health, and the hardest single blow \
                 taken was {} from {who}.\n",
                digest.lowest_health, digest.worst_hit
            ),
            None => format!(
                "\nThe closest call of the evening was {}% health.\n",
                digest.lowest_health
            ),
        });
    }
    if digest.flights > 0 || digest.travelled > 0 {
        out.push_str(&format!(
            "\nGround covered: {}, {}.\n",
            tally::far(digest.travelled),
            crate::chronicle::plural(digest.flights as usize, "flight taken", "flights taken")
        ));
    }

    out.push_str(&format!(
        "\nPurse: {} over the session.",
        crate::chronicle::purse(digest.purse)
    ));
    if digest.quest_income > 0 {
        out.push_str(&format!(" Quests paid {}.", money(digest.quest_income)));
    }
    if digest.sale_income > 0 {
        out.push_str(&format!(" Auctions paid {}.", money(digest.sale_income)));
    }
    out.push('\n');

    out
}

/// A heading and its lines, or nothing at all.
///
/// An empty section printed as a heading with nothing under it invites the
/// model to fill it in, which is the one thing it is being told not to do.
fn section(out: &mut String, heading: &str, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    out.push_str(&format!("\n{heading}:\n"));
    for line in lines {
        out.push_str(&format!("- {line}\n"));
    }
}

fn quality_name(quality: u8) -> &'static str {
    match quality {
        5 => "legendary",
        4 => "epic",
        3 => "rare",
        _ => "uncommon",
    }
}

/// Read the response into an entry.
///
/// OpenAI's shape, which llama.cpp implements:
/// `{choices: [{message: {content, reasoning_content}, finish_reason}], model}`.
pub fn parse_written(body: &[u8]) -> Outcome<Written> {
    let value: Value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let Some(choice) = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
    else {
        return Outcome::Stale(Reason::Malformed("the reply carried no choices".into()));
    };

    // Checked before the content. An entry cut off mid-JSON cannot be
    // salvaged, and saying so as a shape problem is better than an empty
    // answer — one that silently vanished is indistinguishable from one never
    // asked for.
    if choice.get("finish_reason").and_then(Value::as_str) == Some("length") {
        return Outcome::Stale(Reason::Malformed(
            "the entry ran past the token budget before it finished".into(),
        ));
    }

    let Some(content) = choice
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
    else {
        return Outcome::Stale(Reason::Malformed("the reply carried no message".into()));
    };

    let Ok(entry) = serde_json::from_str::<Value>(strip_thinking(content).trim()) else {
        return Outcome::Stale(Reason::Malformed(
            "the reply was not the shape that was asked for".into(),
        ));
    };

    let (Some(title), Some(prose)) = (
        entry.get("title").and_then(Value::as_str),
        entry.get("entry").and_then(Value::as_str),
    ) else {
        return Outcome::Stale(Reason::Malformed(
            "the reply had no title and entry in it".into(),
        ));
    };

    if prose.trim().is_empty() {
        return Outcome::Empty;
    }

    Outcome::Found(Written {
        title: title.trim().to_string(),
        body: prose.trim().to_string(),
        // The server names the model it is running. `/props` is the better
        // answer and the application asks it once; this is the fallback for a
        // server that puts something useful here instead.
        model: value
            .get("model")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty() && *name != "armory-chronicle")
            .unwrap_or("a local model")
            .to_string(),
    })
}

/// Cut a `<think>` block off the front of a reply.
///
/// llama-server puts a model's reasoning in `reasoning_content`, which this
/// never reads — but that only happens when the server was launched with a
/// template that knows how, and plenty of GGUFs emit the tags inline instead.
/// The grammar constrains what the *sampler* may produce, and a model that
/// opens with `<think>` has produced something the JSON parser will refuse.
///
/// Everything after the closing tag, or the whole string if there is no block.
fn strip_thinking(content: &str) -> &str {
    match content.find("</think>") {
        Some(end) => &content[end + "</think>".len()..],
        None => content,
    }
}

/// Read the error message out of a refused request.
///
/// llama.cpp answers a malformed request or an overloaded server with a body
/// that says which in plain words. `ui::http` hands that here rather than
/// reporting the status code, because "HTTP 400" sends somebody looking for a
/// bug in Armory and "context shift is disabled" does not.
pub fn parse_error(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    value
        .get("error")
        .and_then(|error| error.get("message").or(Some(error)))
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::{CharacterKey, Faction};
    use crate::chronicle::{Happening, Moment, Session};
    use chrono::TimeZone;

    fn digest() -> Digest {
        Session {
            character: CharacterKey::new("emerald-dream", "Somechar"),
            display_name: "Somechar".into(),
            realm_name: "Emerald Dream".into(),
            class: "Druid".into(),
            race: "Tauren".into(),
            faction: Faction::Horde,
            started_at: chrono::Utc.with_ymd_and_hms(2026, 8, 3, 19, 0, 0).unwrap(),
            ended_at: chrono::Utc.with_ymd_and_hms(2026, 8, 3, 21, 0, 0).unwrap(),
            start_level: 70,
            end_level: 71,
            start_money: 1_000_000,
            end_money: 1_250_000,
            start_item_level: 600,
            end_item_level: 604,
            moments: vec![
                Moment {
                    at: 0,
                    what: Happening::Arrived {
                        zone: "Nagrand".into(),
                        subzone: Some("Halaa".into()),
                        map: None,
                    },
                },
                Moment {
                    at: 10,
                    what: Happening::Accepted {
                        title: "Hero of the Mag'har".into(),
                        premise: Some("Garrosh needs a champion.".into()),
                    },
                },
                Moment {
                    at: 600,
                    what: Happening::Completed {
                        quest: 9999,
                        title: "Hero of the Mag'har".into(),
                        story: Some("The Mag'har will sing of this.".into()),
                    },
                },
                Moment {
                    at: 600,
                    what: Happening::Paid {
                        quest: 9999,
                        money: 45_000,
                        experience: 1200,
                    },
                },
                Moment {
                    at: 900,
                    what: Happening::Fought {
                        name: "Durn the Hungerer".into(),
                        won: false,
                    },
                },
            ],
            kills: 412,
            risen: vec![("The Severed Threads".into(), 7)],
            travelled: 41_288,
            longest_fight: 664,
            worst_hit: 812_004,
            worst_hit_by: Some("Durn the Hungerer".into()),
            lowest_health: 7,
        }
        .digest()
    }

    #[test]
    fn the_request_goes_to_the_servers_completions_endpoint() {
        let request = write("http://127.0.0.1:8080", &digest());
        assert_eq!(request.url, "http://127.0.0.1:8080/v1/chat/completions");
        // A trailing slash on a hand-typed address is the usual mistake, and
        // `//v1/chat/completions` is a 404 that reads like a broken feature.
        assert_eq!(
            write("http://127.0.0.1:8080/", &digest()).url,
            "http://127.0.0.1:8080/v1/chat/completions"
        );
        assert!(request
            .headers
            .iter()
            .any(|(name, value)| name == "content-type" && value == "application/json"));
        // Nothing to authenticate with, and nothing that could be.
        assert!(!request
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("authorization")
                || name.eq_ignore_ascii_case("x-api-key")));
    }

    #[test]
    fn the_model_is_asked_for_by_props_because_the_request_cannot_say() {
        // llama-server serves whatever it was launched with and ignores the
        // `model` field, so an entry's attribution can only come from here.
        assert_eq!(
            identify("http://127.0.0.1:8080/").url,
            "http://127.0.0.1:8080/props"
        );
        assert_eq!(
            parse_identity(br#"{"model_alias":"qwen3-30b"}"#).as_deref(),
            Some("qwen3-30b")
        );
        // No alias: the file name, minus the extension and the path.
        assert_eq!(
            parse_identity(br#"{"model_path":"/srv/models/Qwen3-30B-Q6_K.gguf"}"#).as_deref(),
            Some("Qwen3-30B-Q6_K")
        );
        // A gateway answering a shape of its own is a thing not shown rather
        // than a failure.
        assert_eq!(parse_identity(br#"{"something":"else"}"#), None);
        assert_eq!(parse_identity(b"not json"), None);
    }

    #[test]
    fn the_request_body_is_json_and_asks_for_a_titled_entry() {
        let request = write("k", &digest());
        let body: Value =
            serde_json::from_str(request.body.as_deref().expect("a body")).expect("json");

        assert_eq!(body["response_format"]["type"], "json_schema");
        let schema = &body["response_format"]["json_schema"]["schema"];
        let required = schema["required"].as_array().expect("required");
        assert!(required.contains(&json!("title")));
        assert!(required.contains(&json!("entry")));
        // Non-streaming: there is nobody watching an entry arrive, and an SSE
        // parser would be machinery in exchange for nothing.
        assert_eq!(body["stream"], json!(false));
        // The voice is a system message, the evening is a user message.
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
    }

    #[test]
    fn the_brief_carries_the_quest_text_the_game_put_on_screen() {
        // The single most valuable thing the addon captures. If it stops
        // reaching the model, entries go back to being lists of titles.
        let brief = brief(&digest());
        assert!(brief.contains("Garrosh needs a champion."), "{brief}");
        assert!(brief.contains("The Mag'har will sing of this."), "{brief}");
    }

    #[test]
    fn the_brief_says_a_wipe_was_a_wipe() {
        // The model is told not to imply an outcome the log does not record,
        // and this is the line that makes that possible to obey.
        let brief = brief(&digest());
        assert!(brief.contains("Fought and lost to"), "{brief}");
        assert!(brief.contains("Durn the Hungerer"), "{brief}");
        assert!(!brief.contains("Bosses defeated"), "{brief}");
    }

    #[test]
    fn an_empty_section_is_left_out_rather_than_left_blank() {
        // A heading with nothing under it is an invitation to fill it in,
        // which is the one thing the model is being told not to do.
        let brief = brief(&digest());
        assert!(!brief.contains("Sold at auction"), "{brief}");
        assert!(!brief.contains("Achievements earned"), "{brief}");
    }

    #[test]
    fn the_brief_names_the_character_the_way_a_voice_needs() {
        let brief = brief(&digest());
        assert!(brief.contains("Somechar of Emerald Dream"), "{brief}");
        assert!(brief.contains("tauren druid of the Horde"), "{brief}");
    }

    #[test]
    fn a_written_entry_reads_out_of_the_first_choice() {
        let body = br#"{"model":"qwen3-30b","choices":[{"finish_reason":"stop","message":
            {"role":"assistant","content":"{\"title\":\"Halaa Again\",\"entry\":\"The wind off the plains.\"}"}}]}"#;
        let written = parse_written(body).found().expect("an entry");
        assert_eq!(written.title, "Halaa Again");
        assert_eq!(written.body, "The wind off the plains.");
        assert_eq!(written.model, "qwen3-30b");
    }

    #[test]
    fn a_thinking_block_in_the_content_is_cut_off_rather_than_choked_on() {
        // llama-server puts reasoning in `reasoning_content` only when the
        // template it was launched with knows how. Plenty of GGUFs emit the
        // tags inline instead, and the JSON parser refuses the lot.
        let body = br#"{"choices":[{"message":{"content":
            "<think>The player went to Halaa. I should write about that.</think>\n{\"title\":\"Halaa\",\"entry\":\"I went.\"}"}}]}"#;
        let written = parse_written(body).found().expect("an entry");
        assert_eq!(written.title, "Halaa");
        assert_eq!(written.body, "I went.");
    }

    #[test]
    fn an_entry_cut_off_by_the_token_budget_is_reported_rather_than_half_saved() {
        let body =
            br#"{"choices":[{"finish_reason":"length","message":{"content":"{\"title\":\"Hal"}}]}"#;
        assert!(matches!(parse_written(body), Outcome::Stale(_)));
    }

    #[test]
    fn a_reply_in_a_shape_we_do_not_know_is_stale_and_never_empty() {
        // Collapsing these would mean a changed response silently producing a
        // journal with no entries in it, which reads as "nothing happened".
        assert!(matches!(
            parse_written(br#"{"choices":[]}"#),
            Outcome::Stale(_)
        ));
        assert!(matches!(
            parse_written(br#"{"choices":[{"message":{"content":"just prose"}}]}"#),
            Outcome::Stale(_)
        ));
        assert!(matches!(
            parse_written(b"<html>nope</html>"),
            Outcome::Stale(_)
        ));
    }

    #[test]
    fn an_error_body_is_read_for_what_it_actually_says() {
        // "HTTP 400" sends somebody looking for a bug in Armory. This does not.
        let body = br#"{"error":{"code":500,"message":"the slot is not available","type":"server_error"}}"#;
        assert_eq!(
            parse_error(body).as_deref(),
            Some("the slot is not available")
        );
        assert_eq!(parse_error(b"not json"), None);
    }
}
