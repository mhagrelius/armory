//! What is missing, and on sale.
//!
//! The one thing every collection tracker gets asked for and almost none of
//! them answer: not "what have I not collected" and not "what is the auction
//! house selling", but the intersection. A collector with nine realms' worth of
//! characters and eleven hundred missing pets cannot read either list; they can
//! read the twelve things that are both.
//!
//! Armory already holds both halves — the catalogue with its owned set, and the
//! listings it takes hourly snapshots of — so this is a join rather than a
//! feature. Pure, and tested as a join.
//!
//! **Only pets join cleanly, and that is a fact about the game rather than a
//! shortcoming here.** Every caged battle pet is the same item — 82800 — with
//! the species in a field beside it, which is exactly why the listing parser
//! keeps `pet_species`. A mount is a distinct item per mount, so the join is
//! item id to the mount's own item, which the web API does not give: a mount
//! record names its *spell*, and the item that teaches it is a different
//! number that appears nowhere in the profile API. Toys and decor are items
//! outright and join by item id, but neither is tradeable, so in practice the
//! answer is pets plus whatever recipes and toy-adjacent items a person has
//! chosen to watch.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};

use super::character::CharacterKey;
use super::source::blizzard::auctions::{self, Listing};
use super::source::blizzard::collections::{Collectible, Kind};

/// One thing the account has not collected, currently for sale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub kind: Kind,
    /// The collectible's own id — a species for a pet, a decor id for decor.
    pub collectible_id: u32,
    pub name: String,
    /// The connected realm it is listed on, or zero for region-wide
    /// commodities. Which realm matters more than the price: a bargain on a
    /// realm with no character on it is a transfer, not a purchase.
    pub realm: u32,
    /// Copper, for one.
    pub unit_price: u64,
    /// How many are up. A single listing is somebody's hopeful price; ten is a
    /// market.
    pub quantity: u32,
}

/// Which of the missing entries are on sale right now.
///
/// `catalogue` and `owned` are one collection; `listings` is one snapshot of
/// one realm. Returns cheapest-first, because for a thing a person has decided
/// they want, price is the only remaining question.
pub fn on_sale(
    catalogue: &[Collectible],
    owned: &HashSet<u32>,
    listings: &[Listing],
    realm: u32,
) -> Vec<Offer> {
    let missing: HashMap<u32, &Collectible> = catalogue
        .iter()
        .filter(|entry| !owned.contains(&entry.id))
        .map(|entry| (join_key(entry), entry))
        .collect();

    if missing.is_empty() {
        return Vec::new();
    }

    // Cheapest per collectible, and the quantities added up. Two listings of
    // the same pet are one answer with a count, not two rows.
    let mut best: HashMap<u32, Offer> = HashMap::new();

    for listing in listings {
        let Some(key) = listing_key(listing) else {
            continue;
        };
        let Some(entry) = missing.get(&key) else {
            continue;
        };

        let offer = best.entry(entry.id).or_insert_with(|| Offer {
            kind: entry.kind,
            collectible_id: entry.id,
            name: entry.name.clone(),
            realm,
            unit_price: listing.unit_price,
            quantity: 0,
        });
        offer.unit_price = offer.unit_price.min(listing.unit_price);
        offer.quantity = offer.quantity.saturating_add(listing.quantity);
    }

    let mut offers: Vec<Offer> = best.into_values().collect();
    offers.sort_by(|a, b| {
        a.unit_price
            .cmp(&b.unit_price)
            .then_with(|| a.name.cmp(&b.name))
    });
    offers
}

/// What a catalogue entry would be listed under.
///
/// A pet is listed by species and everything else by item. `link_id` is the
/// item for a toy or a piece of decor and the *spell* for a mount, which is why
/// mounts do not join — see the note at the top of this file.
fn join_key(entry: &Collectible) -> u32 {
    match entry.kind {
        Kind::Pet => entry.id,
        _ => entry.link_id,
    }
}

/// What a listing is offering, in the same space as [`join_key`].
///
/// A caged pet's item id is the same for every pet in the game, so a listing
/// with a species is answered by its species and never by its item — matching
/// on the item would make every missing pet look available the moment anybody
/// listed any pet at all.
fn listing_key(listing: &Listing) -> Option<u32> {
    match listing.pet_species {
        Some(species) => Some(species),
        None => Some(listing.item_id),
    }
}

// -- browsing the market ------------------------------------------------------

/// One item as the market currently has it.
///
/// The whole snapshot rather than the watch list, which is affordable for
/// exactly one reason: the response it comes from is downloaded in full every
/// sync already and then thrown away. Browsing is a question about *now*;
/// keeping a history is what watching an item is for, and that stays opt-in
/// because it is the expensive half.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub item_id: u32,
    /// What it is called, once a name has been fetched for it.
    ///
    /// `None` is normal and not an error. A listing carries an id and no name,
    /// and there is no endpoint that turns a list of ids into a list of names,
    /// so names arrive one at a time over successive syncs — the same way
    /// artwork does, and prioritised the same way, by what is on screen.
    pub name: Option<String>,
    pub cheapest: u64,
    pub quantity: u32,
    pub listings: u32,
    pub tenth: u64,
    pub median: u64,
    /// Units that left the listings across the watched window, where there is
    /// one. Zero for an item nobody is watching, which is the usual case.
    pub sold: u32,
    pub span_hours: u32,
}

impl Listed {
    /// What a person would call it: the name, or the id until one arrives.
    pub fn title(&self) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => format!("Item {}", self.item_id),
        }
    }

    /// What the whole listed stock is worth at the cheapest price.
    ///
    /// The rough measure of how much market there is here — a thing worth
    /// four hundred gold with two listed is a different market from a thing
    /// worth two silver with a million listed, and sorting by price alone puts
    /// the first at the top of a page nobody wanted.
    pub fn depth(&self) -> u64 {
        self.cheapest.saturating_mul(u64::from(self.quantity))
    }
}

/// Which unnamed items are worth a name first.
///
/// **Not the display order**, which is what this used to be and was the bug: the
/// browser defaults to alphabetical, unnamed rows sort last, and taking the
/// first hundred and fifty of *those* named a Pink Mageweave Shirt with one
/// listing while Copper Ore — four hundred thousand units across a hundred and
/// fifty-three listings — stayed "Item 2770" and could not be searched for.
///
/// Market presence is the right priority. The items somebody will type the name
/// of are the ones being traded in volume, and a name is what makes them
/// findable at all.
pub fn worth_naming(market: &[Listed], budget: usize) -> Vec<u32> {
    let mut unnamed: Vec<&Listed> = market.iter().filter(|l| l.name.is_none()).collect();
    unnamed.sort_by_key(|l| std::cmp::Reverse((l.listings, l.quantity)));
    unnamed
        .into_iter()
        .take(budget)
        .map(|l| l.item_id)
        .collect()
}

/// Search one realm's snapshot.
///
/// Filtering only. Ordering is the column view's, through a sorter per column,
/// because a table whose headers cannot be clicked is a table that looks broken
/// — and having both a dropdown and clickable headers would be two affordances
/// for one action. This function used to sort as well, which is why `Order`
/// existed; it did not survive contact with somebody trying to click "Cheapest".
///
/// Matching is on the name where there is one and on the id otherwise, because
/// an id is what somebody pasting from a wiki has. An item whose name has not
/// arrived is *not* filtered out by a search that would have matched it — it
/// simply cannot match, which is the honest behaviour and is why the count of
/// unnamed items is worth showing beside the results.
pub fn browse(market: &[Listed], needle: &str) -> Vec<Listed> {
    let needle = needle.trim().to_lowercase();
    market
        .iter()
        .filter(|listed| {
            if needle.is_empty() {
                return true;
            }
            match &listed.name {
                Some(name) => name.to_lowercase().contains(&needle),
                None => listed.item_id.to_string().contains(&needle),
            }
        })
        .cloned()
        .collect()
}

// -- what a recipe book is worth ----------------------------------------------

/// One thing a character knows how to make.
///
/// The crafting tally says what somebody has *made*; this says what they *can*
/// make, which is a different question and the one a flip needs. Neither has an
/// endpoint — the profile API reports a profession's skill level and stops
/// there, and does not know recipes exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipe {
    /// The recipe's spell id, which is what the game keys it by.
    pub id: u32,
    pub name: String,
    /// The item the craft produces.
    ///
    /// Recipes with no output item — enchants, recrafts — are not recorded at
    /// all, because there is no price to look up for one.
    pub output: u32,
    /// How many it makes at minimum.
    ///
    /// The minimum and never the maximum: a recipe that *may* make three is a
    /// recipe that makes one, and costing a flip against the lucky outcome is
    /// how a margin becomes fiction.
    pub makes: u32,
    pub reagents: Vec<Reagent>,
}

/// One required reagent slot, and every quality it can be filled with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reagent {
    pub quantity: u32,
    /// The item ids of each quality tier, as the game lists them.
    ///
    /// Separate ids rather than variants of one, which the auction house
    /// proves: reagents are commodities and a commodity carries no bonus ids to
    /// vary by. So each tier is priced on its own and the cheapest one that has
    /// a price is what a craft is costed at.
    pub tiers: Vec<u32>,
}

/// What every character can make, keyed by who can make it.
pub type RecipeBooks = HashMap<CharacterKey, Vec<Recipe>>;

/// The auction house's cut of a sale, as a percentage.
///
/// Blizzard takes five percent of the sale price on the way out. The deposit is
/// returned when something sells, so it is not a cost of a successful flip and
/// is not modelled.
const CUT: u64 = 5;

/// One craft worth making, and who should make it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Making {
    pub recipe: u32,
    pub name: String,
    /// Who can make it. A recipe two characters both know appears once, under
    /// whichever of them the sort settled on — the answer to "what should I
    /// make" is a thing and a person, not a list of people.
    pub by: CharacterKey,
    pub by_name: String,
    pub realm: u32,
    pub realm_name: String,
    pub makes: u32,
    /// What the reagents cost for one craft, at the cheapest priced tier.
    pub cost: u64,
    /// What one of the output has been going for.
    pub each: u64,
    /// What one craft brings in after the auction house's cut.
    pub revenue: u64,
    /// Revenue minus cost. Always positive here — see [`worth_making`].
    pub margin: i64,
    /// Units of the output that left the listings across the window.
    ///
    /// The same inference as [`Resale::sold`], and the reason this list is
    /// ranked rather than sorted by margin: a four-hundred-gold margin on a
    /// thing nobody buys is worth nothing, and a calculator that cannot tell
    /// the two apart is how somebody ends up with forty unsold flasks.
    pub sold: u32,
    /// How many snapshots the figures rest on. Two is a rumour.
    pub samples: usize,
    /// The span those snapshots actually cover, in hours.
    pub span_hours: u32,
    /// Reagents already sitting in the Warband bank, as `(item, count)`.
    ///
    /// **Shown, and deliberately not counted.** The addon's Warband bag indices
    /// have never been confirmed against a stocked bank — the read came back
    /// empty, which is either an empty bank or the wrong index — so subtracting
    /// this from the cost would turn an unverified number into a silently
    /// inflated margin. As a line of its own a wrong index is visibly wrong
    /// instead.
    pub held: Vec<(u32, u64)>,
}

/// What the account's recipe books are worth, and what could not be answered.
///
/// One type rather than a pair, because the two halves are one answer: a page
/// that shows the ranking without the count of what could not be priced is a
/// page that quietly presents a subset as the whole.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Crafting {
    pub worth: Vec<Making>,
    pub unmeasured: Unmeasured,
}

/// What could not be answered, and why.
///
/// Returned beside the answer rather than folded into it, for the reason
/// `Evaluation::observable` exists: a recipe with one unpriced reagent is not a
/// cheap recipe, it is an unmeasured one, and silently ranking it against
/// recipes that *are* measured would put a made-up number at the top of the
/// page.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Unmeasured {
    /// Recipes where at least one required reagent has no price on any realm.
    pub missing_reagent: usize,
    /// Recipes whose output has never been seen listed.
    pub missing_output: usize,
}

/// Which of the account's recipes are worth making, and where to sell them.
///
/// Four things have to be true before a recipe appears here:
///
/// **Somebody knows it.** The books come from the addon, one profession window
/// at a time, so a character whose professions have never been opened
/// contributes nothing. That is silence and not a no.
///
/// **Every reagent has a price.** Costing a craft against the reagents that
/// happen to be listed and ignoring the one that is not would make the dearest
/// recipes look like the best ones. A recipe with an unpriced reagent is
/// counted in [`Unmeasured`] instead.
///
/// **The output has a price.** For the same reason, in the other direction.
///
/// **The margin is positive.** This answers "what is worth making"; a list of
/// things that lose money is a different question and nobody asked it.
///
/// The figures are quality-one throughout, and that is a floor rather than a
/// guess. What quality a craft lands at depends on skill, specialisation and
/// inspiration, and Armory reads none of them — quoting the three-star price at
/// somebody whose craft will land at one star would be inventing a number about
/// their own character, which is the same mistake [`Resale::floor`] exists to
/// avoid.
pub fn worth_making(
    books: &RecipeBooks,
    names: &HashMap<CharacterKey, String>,
    markets: &[Market],
    warband_bank: &HashMap<u32, u64>,
) -> Crafting {
    let mut best: HashMap<u32, Making> = HashMap::new();
    let mut unmeasured = Unmeasured::default();

    for (character, recipes) in books {
        for recipe in recipes {
            let mut answered = false;
            let mut short_reagent = false;
            let mut short_output = false;

            for (realm, realm_name, series) in markets {
                // The output first: a recipe whose product nobody lists cannot
                // be costed against anything, and there is no point pricing its
                // reagents to find that out.
                let Some((each, sold, samples, span_hours)) = quote(series, recipe.output) else {
                    short_output = true;
                    continue;
                };

                let mut cost = 0u64;
                let mut priced = true;
                for reagent in &recipe.reagents {
                    // The cheapest tier that has a price. A recipe can be made
                    // with any of them, and the cheapest is the one a flip is
                    // actually costed at.
                    let tier = reagent
                        .tiers
                        .iter()
                        .filter_map(|item| quote(series, *item).map(|(price, ..)| price))
                        .min();
                    match tier {
                        Some(price) => {
                            cost = cost.saturating_add(price * u64::from(reagent.quantity))
                        }
                        None => {
                            priced = false;
                            break;
                        }
                    }
                }
                if !priced {
                    short_reagent = true;
                    continue;
                }

                let revenue = each * u64::from(recipe.makes) * (100 - CUT) / 100;
                let margin = revenue as i64 - cost as i64;
                answered = true;
                if margin <= 0 {
                    continue;
                }

                let candidate = Making {
                    recipe: recipe.id,
                    name: recipe.name.clone(),
                    by: character.clone(),
                    by_name: names
                        .get(character)
                        .cloned()
                        .unwrap_or_else(|| character.display_name()),
                    realm: *realm,
                    realm_name: realm_name.clone(),
                    makes: recipe.makes,
                    cost,
                    each,
                    revenue,
                    margin,
                    sold,
                    samples,
                    span_hours,
                    held: recipe
                        .reagents
                        .iter()
                        .flat_map(|reagent| reagent.tiers.iter())
                        .filter_map(|item| warband_bank.get(item).map(|count| (*item, *count)))
                        .collect(),
                };

                // Which realm to make it on: the one where the craft is worth
                // most. Ties go to the realm actually moving them, because an
                // identical margin on a dead market is not the same offer.
                let better = match best.get(&recipe.id) {
                    None => true,
                    Some(held) => {
                        (candidate.margin, candidate.sold as i64) > (held.margin, held.sold as i64)
                    }
                };
                if better {
                    best.insert(recipe.id, candidate);
                }
            }

            // Counted once per recipe rather than once per realm, and only when
            // no realm could answer it at all.
            if !answered {
                if short_reagent {
                    unmeasured.missing_reagent += 1;
                } else if short_output {
                    unmeasured.missing_output += 1;
                }
            }
        }
    }

    let mut out: Vec<Making> = best.into_values().collect();
    // Realisable profit, not paper margin: what one craft is worth, times how
    // many of them the market has actually absorbed.
    out.sort_by(|a, b| {
        let worth = |making: &Making| making.margin.max(0) * i64::from(making.sold);
        worth(b)
            .cmp(&worth(a))
            .then_with(|| b.margin.cmp(&a.margin))
            .then_with(|| a.name.cmp(&b.name))
    });
    Crafting {
        worth: out,
        unmeasured,
    }
}

/// What one item is going for on one realm, and how much of it has moved.
///
/// `None` when nothing has ever been recorded for it, which is the difference
/// between "cheap" and "unknown".
fn quote(series: &Series, item: u32) -> Option<(u64, u32, usize, u32)> {
    // Commodities carry no variant, so a reagent's series key is its item id.
    // Anything with a variant is a piece of gear and is not what a reagent or a
    // stackable output is listed as.
    let samples = series.get(&item.to_string())?;
    if samples.is_empty() {
        return None;
    }
    Some((
        typical(samples),
        sold(samples),
        samples.len(),
        span_hours(samples),
    ))
}

/// How many of something move in a day, from a count and the span it covers.
///
/// Said as a rate because that is the question — "eighteen sold" is meaningless
/// without knowing whether that was an hour or a month. Still an inference all
/// the way down: Blizzard records no sale, so this is stock that stopped being
/// listed and some of it was cancelled.
pub fn per_day(sold: u32, span_hours: u32) -> f64 {
    f64::from(sold) * 24.0 / f64::from(span_hours.max(1))
}

// -- the other direction: what is worth selling -------------------------------

/// One snapshot of one market: what it cost, and what shape the book was in.
///
/// The clock *is* carried, and that is a change. It used to be dropped on the
/// grounds that the whole series is already the window — true, and enough for
/// "what is this worth", but it makes every question with a rate in it
/// unanswerable. "Eighteen sold" and "eighteen sold an hour" are the same
/// number over a different span, and only one of them is a market.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub at: DateTime<Utc>,
    /// The cheapest unit price. What it costs to buy exactly one.
    pub cheapest: u64,
    /// Units listed, across every auction of it.
    pub quantity: u32,
    /// How many separate auctions those units were spread across.
    pub listings: u32,
    /// The unit price a tenth of the way into the book, by quantity.
    pub tenth: u64,
    /// The unit price halfway into the book, by quantity.
    pub median: u64,
}

/// Every price series recorded for one item on one realm.
///
/// Keyed by [`Listing::series`] — which for item 82800 is a species and a
/// quality — oldest sample first.
pub type Series = HashMap<String, Vec<Sample>>;

/// One realm's market: its connected-realm id, its name, and its series.
pub type Market = (u32, String, Series);

/// One spare pet, and the realm to sell it on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resale {
    pub species: u32,
    pub name: String,
    /// Copies beyond the one worth keeping. Never zero — a pet you own once is
    /// not a thing you can sell, it is a thing you would be giving up.
    pub spare: u32,
    pub realm: u32,
    pub realm_name: String,
    /// What the *cheapest* quality of this pet has been going for, in copper.
    ///
    /// Deliberately the floor rather than the best price seen. Armory knows the
    /// quality of every pet *listed* — the auction house says so — and not the
    /// quality of the ones in your own journal, because the pet journal reports
    /// quality per pet rather than per species and the collector does not read
    /// it. Quoting the rare price at somebody whose spare is a common would be
    /// inventing a number about their own collection. The floor is the one
    /// figure that is true whatever quality the spare turns out to be.
    pub floor: u64,
    /// What the dearest quality has been going for. The spread, so a pet whose
    /// value is all in the rare version is visible as exactly that.
    pub ceiling: u64,
    /// Units that vanished from the listings between snapshots, summed across
    /// every quality.
    ///
    /// Inferred, and it has to be: Blizzard records no sale anywhere, so a
    /// quantity going down is either a sale or a cancelled auction and nothing
    /// distinguishes them. Still the only liquidity signal there is — a high
    /// price nothing moves at is a price nobody paid.
    pub sold: u32,
    /// How many snapshots the figures rest on. Two is a rumour.
    pub samples: usize,
    /// The span those snapshots actually cover, in hours.
    ///
    /// What turns `sold` into a rate. Thirty days is the most the store may
    /// keep, and a realm watched since Tuesday has four days of evidence — a
    /// page that divides by the former is quoting a number nobody measured.
    pub span_hours: u32,
}

/// Which spare pets are worth selling, and where.
///
/// Three things have to be true before a pet appears here, and each of them
/// removes most of the collection:
///
/// **It can be caged.** Most pets cannot, and only the in-game journal knows —
/// the web API's pet record does not say. A `None` is silence rather than a no,
/// so an account that has never run the collector gets an empty list and not a
/// wrong one.
///
/// **There is a spare.** Caging the only copy of a pet takes it out of the
/// collection, which is the opposite of what this application is for.
///
/// **Somebody has been selling it.** A price with no history behind it is one
/// hopeful listing, and recommending against it is how a collector ends up
/// undercutting a market that does not exist.
///
/// `markets` is one entry per realm: its id, its name, and the price series for
/// item 82800 on it, keyed by [`Listing::series`] and oldest sample first.
pub fn worth_selling(
    catalogue: &[Collectible],
    held: &HashMap<u32, u32>,
    markets: &[Market],
) -> Vec<Resale> {
    let sellable: HashMap<u32, &Collectible> = catalogue
        .iter()
        .filter(|entry| entry.kind == Kind::Pet)
        .filter(|entry| entry.tradeable == Some(true))
        .filter(|entry| held.get(&entry.id).copied().unwrap_or(0) > 1)
        .map(|entry| (entry.id, entry))
        .collect();

    if sellable.is_empty() {
        return Vec::new();
    }

    let mut best: HashMap<u32, Resale> = HashMap::new();

    for (realm, realm_name, series) in markets {
        // Every quality of one species, folded back together. The series are
        // stored apart because they are different goods; the answer to "what is
        // my spare worth" is the range across them.
        let mut per_species: HashMap<u32, (u64, u64, u32, usize, u32)> = HashMap::new();

        for (key, samples) in series {
            let Some((species, _quality)) = auctions::pet_series(key) else {
                continue;
            };
            if !sellable.contains_key(&species) || samples.is_empty() {
                continue;
            }

            let price = typical(samples);
            let entry = per_species
                .entry(species)
                .or_insert((price, price, 0, 0, 1));
            entry.0 = entry.0.min(price);
            entry.1 = entry.1.max(price);
            entry.2 = entry.2.saturating_add(sold(samples));
            entry.3 += samples.len();
            // The longest-watched quality, because the qualities are folded
            // together and the rate has to be over a span that covers them.
            entry.4 = entry.4.max(span_hours(samples));
        }

        for (species, (floor, ceiling, sold, samples, span_hours)) in per_species {
            let Some(entry) = sellable.get(&species) else {
                continue;
            };
            let candidate = Resale {
                species,
                name: entry.name.clone(),
                spare: held.get(&species).copied().unwrap_or(1).saturating_sub(1),
                realm: *realm,
                realm_name: realm_name.clone(),
                floor,
                ceiling,
                sold,
                samples,
                span_hours,
            };

            // Which realm makes sense: the one paying most for the same pet.
            // Ties go to the realm that has actually been moving them, because
            // an identical price on a dead market is not the same offer.
            let better = match best.get(&species) {
                None => true,
                Some(held) => (candidate.floor, candidate.sold) > (held.floor, held.sold),
            };
            if better {
                best.insert(species, candidate);
            }
        }
    }

    let mut out: Vec<Resale> = best.into_values().collect();
    out.sort_by(|a, b| {
        b.floor
            .cmp(&a.floor)
            .then_with(|| b.sold.cmp(&a.sold))
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

/// The going rate for a series: the median of the cheapest listing over time.
///
/// Not the latest, which is whatever one person happened to be asking when the
/// last snapshot ran, and not the mean, which one lowball drags down as far as
/// it likes. The median of thirty days of "what it cost to buy one" is the
/// number somebody can actually expect.
fn typical(samples: &[Sample]) -> u64 {
    let mut prices: Vec<u64> = samples.iter().map(|sample| sample.cheapest).collect();
    prices.sort_unstable();
    prices.get(prices.len() / 2).copied().unwrap_or(0)
}

/// How many units left the listings across a series.
///
/// Only the falls. A quantity going up is somebody listing more, which says
/// nothing about demand, and counting it would turn a stagnant market into a
/// busy one.
fn sold(samples: &[Sample]) -> u32 {
    samples
        .windows(2)
        .filter_map(|pair| pair[0].quantity.checked_sub(pair[1].quantity))
        .sum()
}

/// How long a series covers, in hours.
///
/// The span actually observed rather than the thirty days the store is allowed
/// to keep: a realm watched since Tuesday has four days of evidence and saying
/// otherwise turns a rate into a fiction. Never zero, so it can be divided by.
fn span_hours(samples: &[Sample]) -> u32 {
    let (Some(first), Some(last)) = (samples.first(), samples.last()) else {
        return 1;
    };
    (last.at - first.at).num_hours().max(1) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::blizzard::collections::Source;

    fn pet(id: u32, name: &str) -> Collectible {
        Collectible {
            kind: Kind::Pet,
            id,
            name: name.to_string(),
            source: Source::Drop,
            description: None,
            flavour: None,
            icon: None,
            display: None,
            faction: None,
            link_id: id * 10,
            tradeable: None,
        }
    }

    fn caged(species: u32, price: u64, quantity: u32) -> Listing {
        Listing {
            // Every caged pet in the game is this item.
            item_id: 82800,
            unit_price: price,
            quantity,
            variant: String::new(),
            pet_species: Some(species),
            pet_quality: Some(3),
        }
    }

    #[test]
    fn only_what_is_missing_is_offered() {
        let catalogue = vec![pet(1, "Sprite Darter"), pet(2, "Nether Faerie Dragon")];
        let owned = HashSet::from([1]);
        let listings = vec![caged(1, 500, 1), caged(2, 900, 1)];

        let offers = on_sale(&catalogue, &owned, &listings, 61);
        assert_eq!(offers.len(), 1, "the collected one is not an offer");
        assert_eq!(offers[0].collectible_id, 2);
        assert_eq!(offers[0].realm, 61);
    }

    #[test]
    fn a_caged_pet_is_matched_on_its_species_and_never_on_its_item() {
        // Every caged pet is item 82800. Joining on the item would make every
        // missing pet in the game look available the moment anybody listed one.
        let catalogue = vec![pet(1, "Sprite Darter"), pet(2, "Nether Faerie Dragon")];
        let offers = on_sale(&catalogue, &HashSet::new(), &[caged(2, 900, 1)], 0);

        assert_eq!(offers.len(), 1);
        assert_eq!(offers[0].name, "Nether Faerie Dragon");
    }

    #[test]
    fn several_listings_of_one_thing_are_one_offer_at_the_lowest_price() {
        // What it costs to buy one, and how many there are to buy. Two rows for
        // the same pet is the auction house's view, not a collector's.
        let catalogue = vec![pet(1, "Sprite Darter")];
        let listings = vec![caged(1, 900, 2), caged(1, 400, 1), caged(1, 1200, 5)];

        let offers = on_sale(&catalogue, &HashSet::new(), &listings, 0);
        assert_eq!(offers.len(), 1);
        assert_eq!(offers[0].unit_price, 400);
        assert_eq!(offers[0].quantity, 8);
    }

    #[test]
    fn offers_come_back_cheapest_first() {
        let catalogue = vec![pet(1, "A"), pet(2, "B"), pet(3, "C")];
        let listings = vec![caged(1, 900, 1), caged(2, 100, 1), caged(3, 500, 1)];

        let offers = on_sale(&catalogue, &HashSet::new(), &listings, 0);
        let prices: Vec<u64> = offers.iter().map(|offer| offer.unit_price).collect();
        assert_eq!(prices, [100, 500, 900]);
    }

    #[test]
    fn a_toy_joins_on_the_item_it_is() {
        let mut toy = pet(500, "Kang's Bindstone");
        toy.kind = Kind::Toy;
        toy.link_id = 86571;

        let listing = Listing {
            item_id: 86571,
            unit_price: 250_000,
            quantity: 1,
            variant: String::new(),
            pet_species: None,
            pet_quality: None,
        };

        let offers = on_sale(&[toy], &HashSet::new(), &[listing], 61);
        assert_eq!(offers.len(), 1);
        assert_eq!(offers[0].kind, Kind::Toy);
        assert_eq!(offers[0].unit_price, 250_000);
    }

    #[test]
    fn a_snapshot_with_nothing_of_interest_yields_nothing() {
        // The usual case by a wide margin: a realm's auction house is tens of
        // thousands of listings and almost none of them are collectibles.
        let catalogue = vec![pet(1, "Sprite Darter")];
        let noise = Listing {
            item_id: 197794,
            unit_price: 50,
            quantity: 900,
            variant: String::new(),
            pet_species: None,
            pet_quality: None,
        };
        assert!(on_sale(&catalogue, &HashSet::new(), &[noise], 0).is_empty());
    }

    // -- what is worth selling ------------------------------------------------

    fn spare(id: u32, name: &str) -> Collectible {
        let mut entry = pet(id, name);
        entry.tradeable = Some(true);
        entry
    }

    /// One species at one quality, and what it did over time.
    type Sampled<'a> = (u32, u32, &'a [(u64, u32)]);

    /// `(price, quantity)` pairs as a series, an hour apart.
    ///
    /// An hour because that is what a snapshot cycle is, so a test that writes
    /// four samples is writing four hours of market and any rate derived from
    /// it is the rate a real four hours would give.
    fn over_time(samples: &[(u64, u32)]) -> Vec<Sample> {
        let start = DateTime::parse_from_rfc3339("2026-08-01T00:00:00Z")
            .expect("a date")
            .to_utc();
        samples
            .iter()
            .enumerate()
            .map(|(hour, (price, quantity))| Sample {
                at: start + chrono::Duration::hours(hour as i64),
                cheapest: *price,
                quantity: *quantity,
                listings: 1,
                tenth: *price,
                median: *price,
            })
            .collect()
    }

    /// One realm's series for item 82800, keyed the way the store holds them.
    fn market(realm: u32, name: &str, series: &[Sampled]) -> Market {
        let built = series
            .iter()
            .map(|(species, quality, samples)| {
                (format!("pet{species}:{quality}"), over_time(samples))
            })
            .collect();
        (realm, name.to_string(), built)
    }

    /// A market of plain commodities, keyed by item id the way one is.
    fn goods(realm: u32, name: &str, items: &[(u32, &[(u64, u32)])]) -> Market {
        let series: Series = items
            .iter()
            .map(|(item, samples)| (item.to_string(), over_time(samples)))
            .collect();
        (realm, name.to_string(), series)
    }

    fn flask() -> Recipe {
        Recipe {
            id: 371_637,
            name: "Flask of Alchemical Chaos".into(),
            output: 191_318,
            makes: 1,
            reagents: vec![
                Reagent {
                    quantity: 3,
                    tiers: vec![210_796, 210_797, 210_798],
                },
                Reagent {
                    quantity: 1,
                    tiers: vec![212_263],
                },
            ],
        }
    }

    fn books(recipes: Vec<Recipe>) -> (RecipeBooks, HashMap<CharacterKey, String>) {
        let key = CharacterKey::new("emerald-dream", "Somechar");
        (
            HashMap::from([(key.clone(), recipes)]),
            HashMap::from([(key, "Somechar".to_string())]),
        )
    }

    fn listed(item_id: u32, name: Option<&str>, cheapest: u64, quantity: u32) -> Listed {
        Listed {
            item_id,
            name: name.map(str::to_string),
            cheapest,
            quantity,
            listings: 1,
            tenth: cheapest,
            median: cheapest,
            sold: 0,
            span_hours: 0,
        }
    }

    #[test]
    fn browsing_matches_a_name_where_there_is_one_and_an_id_where_there_is_not() {
        let market = vec![
            listed(1, Some("Mycobloom"), 37_400, 4_120),
            listed(2, Some("Crystalline Powder"), 21_000, 12_400),
            listed(219_873, None, 500, 4),
        ];

        assert_eq!(browse(&market, "bloom").len(), 1);
        // An id is what somebody pasting from a wiki has, and an unnamed item
        // is the only thing they *can* search for.
        let by_id = browse(&market, "219873");
        assert_eq!(by_id.len(), 1);
        assert_eq!(by_id[0].item_id, 219_873);
        assert_eq!(browse(&market, "").len(), 3);
    }

    #[test]
    fn the_items_worth_naming_first_are_the_ones_being_traded() {
        // The bug this replaces: names were fetched in display order, which is
        // alphabetical with unnamed rows last — so a one-listing shirt got a
        // name and Copper Ore, four hundred thousand units across a hundred and
        // fifty-three listings, stayed "Item 2770" and could not be searched.
        let mut ore = listed(2770, None, 21_100, 437_411);
        ore.listings = 153;
        let mut shirt = listed(10_042, None, 500, 1);
        shirt.listings = 1;
        let named = listed(1, Some("Mycobloom"), 37_400, 4_120);

        let wanted = worth_naming(&[shirt, named, ore], 2);
        assert_eq!(wanted, [2770, 10_042], "busiest market first");
        // A named item never needs another call.
        assert!(!wanted.contains(&1));
    }

    #[test]
    fn a_craft_is_costed_at_the_cheapest_reagent_quality_that_has_a_price() {
        // A recipe can be made with any tier, so the cheapest one that is
        // actually listed is what a flip costs — not the tier the game happens
        // to list first.
        let (book, names) = books(vec![flask()]);
        let markets = vec![goods(
            61,
            "Emerald Dream",
            &[
                (191_318, &[(120_000, 40), (120_000, 22)]),
                // The three-star tier is dearest and the one-star is absent, so
                // the two-star is what a craft is costed at: 3 × 900.
                (210_797, &[(900, 500), (900, 480)]),
                (210_798, &[(4_000, 90), (4_000, 88)]),
                (212_263, &[(1_500, 200), (1_500, 190)]),
            ],
        )];

        let Crafting {
            worth: making,
            unmeasured,
        } = worth_making(&book, &names, &markets, &HashMap::new());
        assert_eq!(unmeasured, Unmeasured::default());
        assert_eq!(making.len(), 1);

        let flip = &making[0];
        assert_eq!(flip.cost, 3 * 900 + 1_500);
        // Revenue is the output's going rate less the auction house's cut.
        assert_eq!(flip.each, 120_000);
        assert_eq!(flip.revenue, 114_000);
        assert_eq!(flip.margin, 114_000 - 4_200);
        assert_eq!(flip.by_name, "Somechar");
        // Only the falls count as sales, in both directions.
        assert_eq!(flip.sold, 18);
    }

    #[test]
    fn a_recipe_with_one_unpriced_reagent_is_unmeasured_and_not_cheap() {
        // The same rule as `Evaluation::observable`: a floor is not a
        // measurement. Costing a craft against the reagents that happen to be
        // listed would put the dearest recipes at the top of the page.
        let (book, names) = books(vec![flask()]);
        let markets = vec![goods(
            61,
            "Emerald Dream",
            &[
                (191_318, &[(120_000, 40), (120_000, 22)]),
                (210_797, &[(900, 500), (900, 480)]),
                // 212263 has never been seen listed.
            ],
        )];

        let Crafting {
            worth: making,
            unmeasured,
        } = worth_making(&book, &names, &markets, &HashMap::new());
        assert!(making.is_empty());
        assert_eq!(unmeasured.missing_reagent, 1);
        assert_eq!(unmeasured.missing_output, 0);
    }

    #[test]
    fn a_fat_margin_on_a_dead_market_loses_to_a_thin_one_that_moves() {
        // The whole reason this is ranked rather than sorted by margin. Forty
        // unsold flasks is what a paper margin buys.
        let dead = Recipe {
            id: 1,
            name: "Unsellable Draught".into(),
            output: 100,
            makes: 1,
            reagents: vec![Reagent {
                quantity: 1,
                tiers: vec![900],
            }],
        };
        let brisk = Recipe {
            id: 2,
            name: "Ordinary Potion".into(),
            output: 200,
            makes: 1,
            reagents: vec![Reagent {
                quantity: 1,
                tiers: vec![901],
            }],
        };
        let (book, names) = books(vec![dead, brisk]);
        let markets = vec![goods(
            61,
            "Emerald Dream",
            &[
                // Huge margin, one unit ever moved.
                (100, &[(500_000, 3), (500_000, 2)]),
                (900, &[(100, 900), (100, 900)]),
                // Small margin, four hundred moved.
                (200, &[(20_000, 900), (20_000, 500)]),
                (901, &[(100, 900), (100, 900)]),
            ],
        )];

        let making = worth_making(&book, &names, &markets, &HashMap::new()).worth;
        assert_eq!(making.len(), 2);
        assert_eq!(making[0].name, "Ordinary Potion");
        assert!(
            making[1].margin > making[0].margin,
            "the loser here has the better paper margin, which is the point"
        );
    }

    #[test]
    fn warband_stock_is_shown_and_never_subtracted() {
        // The addon's Warband bag indices have never been confirmed against a
        // stocked bank. A wrong index has to look wrong, not quietly inflate a
        // margin.
        let (book, names) = books(vec![flask()]);
        let markets = vec![goods(
            61,
            "Emerald Dream",
            &[
                (191_318, &[(120_000, 40), (120_000, 22)]),
                (210_797, &[(900, 500), (900, 480)]),
                (212_263, &[(1_500, 200), (1_500, 190)]),
            ],
        )];
        let bank = HashMap::from([(210_797u32, 600u64)]);

        let making = worth_making(&book, &names, &markets, &bank).worth;
        assert_eq!(making[0].held, [(210_797, 600)]);
        // Unchanged by the bank holding every reagent the craft needs.
        assert_eq!(making[0].cost, 3 * 900 + 1_500);
    }

    #[test]
    fn a_craft_that_loses_money_is_not_something_worth_making() {
        let (book, names) = books(vec![flask()]);
        let markets = vec![goods(
            61,
            "Emerald Dream",
            &[
                (191_318, &[(1_000, 40), (1_000, 22)]),
                (210_797, &[(900, 500), (900, 480)]),
                (212_263, &[(1_500, 200), (1_500, 190)]),
            ],
        )];

        let Crafting {
            worth: making,
            unmeasured,
        } = worth_making(&book, &names, &markets, &HashMap::new());
        assert!(making.is_empty());
        // Measured, and the answer was no. Not the same as unmeasured.
        assert_eq!(unmeasured, Unmeasured::default());
    }

    #[test]
    fn a_pet_you_own_once_is_not_a_thing_you_can_sell() {
        // Caging it takes it out of the collection, which is the opposite of
        // what the application is for.
        let catalogue = vec![spare(1, "Sprite Darter")];
        let market = market(61, "Emerald Dream", &[(1, 3, &[(5000, 4), (5000, 2)])]);

        let only_one = worth_selling(
            &catalogue,
            &HashMap::from([(1, 1)]),
            std::slice::from_ref(&market),
        );
        assert!(only_one.is_empty(), "one copy is not a spare");

        let two = worth_selling(&catalogue, &HashMap::from([(1, 3)]), &[market]);
        assert_eq!(two.len(), 1);
        assert_eq!(two[0].spare, 2, "the one being kept is not for sale");
    }

    #[test]
    fn a_pet_that_cannot_be_caged_is_never_offered() {
        // Most pets cannot be, and the journal is the only source that says so.
        let mut bound = pet(1, "Sprite Darter");
        bound.tradeable = Some(false);
        let market = market(61, "Emerald Dream", &[(1, 3, &[(5000, 4), (5000, 2)])]);

        assert!(worth_selling(
            &[bound],
            &HashMap::from([(1, 4)]),
            std::slice::from_ref(&market)
        )
        .is_empty());

        // And silence is not a no. An account that has never run the collector
        // knows nothing about tradeability, and guessing either way is worse
        // than an empty list that explains itself.
        let unknown = pet(1, "Sprite Darter");
        assert_eq!(unknown.tradeable, None);
        assert!(worth_selling(&[unknown], &HashMap::from([(1, 4)]), &[market]).is_empty());
    }

    #[test]
    fn the_realm_that_makes_sense_is_the_one_paying_most() {
        let catalogue = vec![spare(1, "Sprite Darter")];
        let held = HashMap::from([(1, 2)]);
        let markets = vec![
            market(61, "Emerald Dream", &[(1, 3, &[(1000, 9), (1000, 8)])]),
            market(11, "Tichondrius", &[(1, 3, &[(9000, 9), (9000, 8)])]),
        ];

        let offers = worth_selling(&catalogue, &held, &markets);
        assert_eq!(offers.len(), 1, "one pet is one recommendation, not two");
        assert_eq!(offers[0].realm_name, "Tichondrius");
        assert_eq!(offers[0].floor, 9000);
    }

    #[test]
    fn the_quoted_price_is_the_floor_across_qualities_and_the_spread_is_shown() {
        // Armory knows the quality of every pet listed and not the quality of
        // the one in your journal. Quoting the rare price at somebody holding a
        // common would be inventing a fact about their own collection.
        let catalogue = vec![spare(1, "Sprite Darter")];
        let markets = vec![market(
            61,
            "Emerald Dream",
            &[
                (1, 1, &[(500, 5), (500, 4)]),
                (1, 3, &[(40_000, 5), (40_000, 4)]),
            ],
        )];

        let offers = worth_selling(&catalogue, &HashMap::from([(1, 2)]), &markets);
        assert_eq!(
            offers[0].floor, 500,
            "true whatever the spare turns out to be"
        );
        assert_eq!(offers[0].ceiling, 40_000);
    }

    #[test]
    fn only_quantities_that_fell_count_as_sales() {
        // A quantity going up is somebody listing more, which says nothing
        // about demand. Counting it would make a stagnant market look busy.
        assert_eq!(
            sold(&over_time(&[(100, 10), (100, 6), (100, 9), (100, 7)])),
            6
        );
        assert_eq!(sold(&over_time(&[(100, 1), (100, 5)])), 0);
        assert_eq!(
            sold(&over_time(&[(100, 4)])),
            0,
            "one sample is no evidence at all"
        );
    }

    #[test]
    fn a_count_only_becomes_a_rate_over_the_span_actually_watched() {
        // Six units in four hours is thirty-six a day. Six units in thirty days
        // is not, and the store keeping thirty days does not mean this realm
        // has been watched for thirty.
        let four_hours = over_time(&[(100, 10), (100, 6), (100, 9), (100, 7)]);
        assert_eq!(span_hours(&four_hours), 3);
        assert!((per_day(sold(&four_hours), span_hours(&four_hours)) - 48.0).abs() < 0.01);

        // A single sample has no span at all, and dividing by nothing has to
        // give a number rather than a panic.
        assert_eq!(span_hours(&over_time(&[(100, 4)])), 1);
        assert_eq!(per_day(0, 1), 0.0);
    }

    #[test]
    fn the_going_rate_is_the_median_and_not_the_last_thing_seen() {
        // The latest is whatever one person was asking when the snapshot ran.
        // One lowball should not reprice a pet.
        assert_eq!(
            typical(&over_time(&[(1000, 1), (1100, 1), (1050, 1), (5, 1)])),
            1050
        );
    }

    #[test]
    fn a_pet_nobody_has_listed_is_not_a_recommendation() {
        // No series, no evidence, no suggestion. Recommending a price with no
        // history behind it is how somebody undercuts a market that is not
        // there.
        let catalogue = vec![spare(1, "Sprite Darter")];
        let markets = vec![market(61, "Emerald Dream", &[(999, 3, &[(5000, 2)])])];
        assert!(worth_selling(&catalogue, &HashMap::from([(1, 5)]), &markets).is_empty());
    }

    #[test]
    fn an_empty_collection_asks_nothing_of_the_snapshot() {
        // Every entry collected means no join to do at all, which is worth
        // returning early for: this runs against every listing on a realm.
        let catalogue = vec![pet(1, "Sprite Darter")];
        let owned = HashSet::from([1]);
        assert!(on_sale(&catalogue, &owned, &[caged(1, 100, 1)], 0).is_empty());
    }
}
