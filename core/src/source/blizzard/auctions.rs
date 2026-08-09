//! The auction house.
//!
//! Two payloads with different shapes, for a reason worth knowing. Since the
//! 2022 overhaul, **commodities** — stackable materials, consumables, gems —
//! are region-wide and priced by `unit_price` alone, because a stack of herbs
//! has no variance. **Everything else** — gear, pets, recipes, bags — is locked
//! to a connected realm and carries `bid`, `buyout`, and an item with bonus
//! ids, modifiers and pet fields, because two copies of the same item id can be
//! very different things.
//!
//! Blizzard publishes no price history and no sale signal at all. A quantity
//! simply disappears between snapshots and whether it sold or was cancelled is
//! not recorded anywhere. So history is accumulated locally and sales are
//! inferred by diffing — and labelled as inferred, because they are.
//!
//! Snapshots refresh hourly. `If-Modified-Since` earns a free `304` everywhere
//! except commodities, which costs 25x quota per call and is charged that even
//! for a 304 — so commodities is polled on a schedule rather than on
//! speculation.

use serde::{Deserialize, Serialize};

use super::super::{parse_json, Outcome, Reason, Request, SourceId};
use super::{url, Namespace, Region};

const SOURCE: SourceId = SourceId::BlizzardGameData;

/// A connected realm: one shared auction house for everything but commodities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectedRealm {
    pub id: u32,
    /// Every realm that shares this auction house. Several of a person's
    /// characters can land in one group without them realising, which is
    /// exactly the thing worth showing.
    pub realms: Vec<String>,
    pub slugs: Vec<String>,
}

/// One listing, flattened to what a price history needs.
///
/// Deliberately not the whole record. Bonus ids and modifiers decide what an
/// item actually *is*, and Blizzard publishes no dictionary for either — so
/// they are kept as an opaque fingerprint for grouping rather than interpreted
/// into stats we would be guessing at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub item_id: u32,
    /// Copper per unit. Commodities give this directly; realm auctions divide
    /// buyout by quantity.
    pub unit_price: u64,
    pub quantity: u32,
    /// Bonus ids and modifiers, joined. Empty for commodities, which have no
    /// variance — that emptiness is the point, not a gap.
    pub variant: String,
    /// Battle pets are listed under one item id with the species in the item,
    /// so without this every pet on the realm would price as one thing.
    pub pet_species: Option<u32>,
    /// A caged pet's quality: 1 poor through 4 rare.
    ///
    /// The single biggest thing about a pet's price after its species. A rare
    /// and a common of the same pet are different goods sold at different
    /// prices to different buyers, and a history that averages them describes
    /// neither.
    pub pet_quality: Option<u32>,
}

/// One realm as a person names it.
///
/// Distinct from [`ConnectedRealm`], and the distinction is the whole reason
/// picking a realm to watch is not a one-liner. A person has a character on
/// "Emerald Dream"; the auction house they trade in is connected realm 61,
/// which is Emerald Dream *and* Terenas together. The name is what to offer in
/// a list and the connected id is what to fetch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Realm {
    pub id: u32,
    pub name: String,
    pub slug: String,
}

/// Every realm in the region, by name.
///
/// One call, and the answer only changes when Blizzard opens or merges a realm.
/// This is what a realm picker is built from — the connected-realm index gives
/// numbers with no names at all, so offering that to somebody would be a list
/// of several hundred integers.
pub fn realm_index(region: Region) -> Request {
    Request::get(
        SOURCE,
        url(region, Namespace::Dynamic, "/data/wow/realm/index", &[]),
    )
}

/// One realm, which is how its connected realm is found.
pub fn realm(region: Region, slug: &str) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Dynamic,
            &format!("/data/wow/realm/{slug}"),
            &[],
        ),
    )
}

/// Read the realm index into names and slugs.
pub fn parse_realm_index(body: &[u8]) -> Outcome<Vec<Realm>> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let Some(list) = value.get("realms").and_then(|list| list.as_array()) else {
        return Outcome::Stale(Reason::Malformed(
            "the realm index carried no realms".into(),
        ));
    };

    let mut realms: Vec<Realm> = list
        .iter()
        .filter_map(|entry| {
            Some(Realm {
                id: entry.get("id")?.as_u64()? as u32,
                name: entry.get("name")?.as_str()?.to_string(),
                slug: entry.get("slug")?.as_str()?.to_string(),
            })
        })
        .collect();

    // Blizzard's order is by id, which is the order they were opened in. A
    // person looking for their realm wants it alphabetical.
    realms.sort_by(|a, b| a.name.cmp(&b.name));
    Outcome::of_collection(realms)
}

/// Read which connected realm a realm trades in.
///
/// The response gives an href rather than an id — the same shape as the
/// connected-realm index, and read the same way.
pub fn parse_realm_connection(body: &[u8]) -> Outcome<u32> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let found = value
        .get("connected_realm")
        .and_then(|connected| connected.get("href"))
        .and_then(|href| href.as_str())
        .and_then(id_from_href);

    match found {
        Some(id) => Outcome::Found(id),
        None => Outcome::Stale(Reason::Malformed("a realm with no connected realm".into())),
    }
}

/// The index of connected realms.
pub fn connected_realm_index(region: Region) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Dynamic,
            "/data/wow/connected-realm/index",
            &[],
        ),
    )
}

pub fn connected_realm(region: Region, id: u32) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Dynamic,
            &format!("/data/wow/connected-realm/{id}"),
            &[],
        ),
    )
}

/// A connected realm's non-commodity auctions.
pub fn auctions(region: Region, connected_realm_id: u32) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Dynamic,
            &format!("/data/wow/connected-realm/{connected_realm_id}/auctions"),
            &[],
        ),
    )
}

/// The region's commodities.
///
/// One document for the whole region, and the expensive one: 25x quota per
/// call, charged even when the answer is `304`.
pub fn commodities(region: Region) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Dynamic,
            "/data/wow/auctions/commodities",
            &[],
        ),
    )
}

/// The WoW Token's current price, in copper.
pub fn token(region: Region) -> Request {
    Request::get(
        SOURCE,
        url(region, Namespace::Dynamic, "/data/wow/token/index", &[]),
    )
}

/// Read the connected-realm index into ids.
pub fn parse_connected_realm_index(body: &[u8]) -> Outcome<Vec<u32>> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let Some(list) = value
        .get("connected_realms")
        .and_then(|list| list.as_array())
    else {
        return Outcome::Stale(Reason::Malformed(
            "the connected-realm index carried no connected_realms".into(),
        ));
    };

    // The index gives hrefs rather than ids, so the id is read off the end of
    // the URL. Blizzard has never given this index a plain id field.
    Outcome::of_collection(
        list.iter()
            .filter_map(|entry| entry.get("href").and_then(|href| href.as_str()))
            .filter_map(id_from_href)
            .collect(),
    )
}

fn id_from_href(href: &str) -> Option<u32> {
    href.rsplit('/')
        .find(|segment| !segment.is_empty())
        .and_then(|tail| tail.split('?').next())
        .and_then(|id| id.parse().ok())
}

/// Read a connected realm's member realms.
pub fn parse_connected_realm(body: &[u8]) -> Outcome<ConnectedRealm> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let Some(id) = value.get("id").and_then(|id| id.as_u64()) else {
        return Outcome::Stale(Reason::Malformed("a connected realm with no id".into()));
    };

    let realms = value.get("realms").and_then(|list| list.as_array());
    Outcome::Found(ConnectedRealm {
        id: id as u32,
        realms: realms
            .map(|list| {
                list.iter()
                    .filter_map(|realm| realm.get("name").and_then(|name| name.as_str()))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        slugs: realms
            .map(|list| {
                list.iter()
                    .filter_map(|realm| realm.get("slug").and_then(|slug| slug.as_str()))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// Read the region's commodity listings.
pub fn parse_commodities(body: &[u8]) -> Outcome<Vec<Listing>> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let Some(list) = value.get("auctions").and_then(|list| list.as_array()) else {
        return Outcome::Stale(Reason::Malformed(
            "the commodities response carried no auctions".into(),
        ));
    };

    Outcome::of_collection(
        list.iter()
            .filter_map(|entry| {
                Some(Listing {
                    item_id: entry.get("item")?.get("id")?.as_u64()? as u32,
                    unit_price: entry.get("unit_price")?.as_u64()?,
                    quantity: entry.get("quantity").and_then(|q| q.as_u64()).unwrap_or(1) as u32,
                    variant: String::new(),
                    pet_species: None,
                    pet_quality: None,
                })
            })
            .collect(),
    )
}

/// Read a connected realm's non-commodity listings.
///
/// A bid-only auction is skipped. Its price is what somebody hopes to get
/// rather than what the item costs, and mixing the two into one history makes
/// both meaningless.
pub fn parse_auctions(body: &[u8]) -> Outcome<Vec<Listing>> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let Some(list) = value.get("auctions").and_then(|list| list.as_array()) else {
        return Outcome::Stale(Reason::Malformed(
            "the auctions response carried no auctions".into(),
        ));
    };

    Outcome::of_collection(
        list.iter()
            .filter_map(|entry| {
                let item = entry.get("item")?;
                let quantity = entry.get("quantity").and_then(|q| q.as_u64()).unwrap_or(1);
                let buyout = entry.get("buyout").and_then(|price| price.as_u64())?;

                Some(Listing {
                    item_id: item.get("id")?.as_u64()? as u32,
                    unit_price: buyout / quantity.max(1),
                    quantity: quantity as u32,
                    variant: variant_of(item),
                    pet_species: item
                        .get("pet_species_id")
                        .and_then(|id| id.as_u64())
                        .map(|id| id as u32),
                    pet_quality: item
                        .get("pet_quality_id")
                        .and_then(|id| id.as_u64())
                        .map(|id| id as u32),
                })
            })
            .collect(),
    )
}

/// A fingerprint for the bonus ids and modifiers on an item.
///
/// Blizzard publishes no dictionary for either, so these are not interpreted —
/// two listings with the same fingerprint are the same thing and two with
/// different fingerprints are not, which is all a price history needs and all
/// that can be honestly claimed.
fn variant_of(item: &serde_json::Value) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(bonuses) = item.get("bonus_lists").and_then(|list| list.as_array()) {
        let mut ids: Vec<u64> = bonuses.iter().filter_map(|id| id.as_u64()).collect();
        // Sorted, because Blizzard's order is not stable between snapshots and
        // an unsorted join would make one item look like two.
        ids.sort_unstable();
        parts.extend(ids.into_iter().map(|id| format!("b{id}")));
    }

    if let Some(modifiers) = item.get("modifiers").and_then(|list| list.as_array()) {
        let mut pairs: Vec<(u64, u64)> = modifiers
            .iter()
            .filter_map(|modifier| {
                Some((
                    modifier.get("type")?.as_u64()?,
                    modifier.get("value")?.as_u64()?,
                ))
            })
            .collect();
        pairs.sort_unstable();
        parts.extend(
            pairs
                .into_iter()
                .map(|(kind, value)| format!("m{kind}:{value}")),
        );
    }

    parts.join(",")
}

/// Read the WoW Token price, in copper.
pub fn parse_token(body: &[u8]) -> Outcome<u64> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };
    match value.get("price").and_then(|price| price.as_u64()) {
        Some(price) => Outcome::Found(price),
        None => Outcome::Stale(Reason::Malformed(
            "the token response carried no price".into(),
        )),
    }
}

/// Every caged battle pet in the game, listed under one item id.
pub const CAGED_PET: u32 = 82800;

impl Listing {
    /// What this is, as far as a price history is concerned.
    ///
    /// Stored in the `variant` column, which is exactly the question that
    /// column answers: what distinguishes two listings of the same item id.
    /// For anything but a pet this is the bonus-and-modifier fingerprint and
    /// nothing has changed.
    ///
    /// For a pet it has to be the species and the quality, because every caged
    /// pet in the game is item 82800 with no bonuses and no modifiers. Without
    /// this they all key alike and a realm's entire pet market records as one
    /// series: the price of the cheapest pet on the realm, against the summed
    /// quantity of every pet listing on it. That series is not a price for
    /// anything.
    ///
    /// Level is deliberately *not* in the key. It would multiply the series
    /// twenty-five-fold to separate markets that barely exist — almost every
    /// caged pet is sold at 1 or at 25 — and since a history keeps the cheapest
    /// listing, folding it in makes the recorded price a floor rather than a
    /// guess, which is the safe direction for deciding whether to sell.
    pub fn series(&self) -> String {
        match self.pet_species {
            Some(species) => format!("pet{species}:{}", self.pet_quality.unwrap_or(0)),
            None => self.variant.clone(),
        }
    }
}

/// Read a species and quality back out of a stored series key.
///
/// The inverse of [`Listing::series`]. A price row knows only its item and its
/// variant, so this is what turns thirty days of rows back into "Sprite Darter,
/// rare".
pub fn pet_series(series: &str) -> Option<(u32, u32)> {
    let (species, quality) = series.strip_prefix("pet")?.split_once(':')?;
    Some((species.parse().ok()?, quality.parse().ok()?))
}

/// One item's market on one realm, at one moment.
///
/// The cheapest price alone cannot tell one lowball at a hundred gold from four
/// hundred units at a hundred gold, and every interesting question about a
/// market — how deep is it, what would it cost to buy twenty, is that floor a
/// real price or one goblin — is a question about the shape of the book rather
/// than about its first row. So a snapshot keeps the shape, in six numbers that
/// still cost one row per item per hour.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Depth {
    pub item_id: u32,
    pub variant: String,
    /// The cheapest unit price. What it costs to buy exactly one.
    pub cheapest: u64,
    /// Units listed, across every auction of it.
    pub quantity: u32,
    /// How many separate auctions those units are spread across.
    ///
    /// The difference between forty sellers and one. It is also what tells a
    /// sale from a cancellation later: stock and listings falling together is
    /// people buying, and one big listing vanishing on its own is not.
    pub listings: u32,
    /// The unit price a tenth of the way into the book, by quantity.
    ///
    /// What somebody clearing the cheap end would actually pay, rather than
    /// what the first row advertises.
    pub tenth: u64,
    /// The unit price halfway into the book, by quantity. The real middle.
    pub median: u64,
}

/// Each item's market shape out of one snapshot.
///
/// Not the mean anywhere: the market price of a thing is what it costs to buy
/// one, and an average is dragged upward by the hopeful listing nobody will
/// ever take. The percentiles are weighted by quantity for the same reason —
/// a stack of four hundred and a stack of one are not one vote each.
pub fn depth(listings: &[Listing]) -> Vec<Depth> {
    use std::collections::HashMap;

    let mut books: HashMap<(u32, String), Vec<(u64, u32)>> = HashMap::new();
    for listing in listings {
        books
            .entry((listing.item_id, listing.series()))
            .or_default()
            .push((listing.unit_price, listing.quantity));
    }

    let mut out: Vec<Depth> = books
        .into_iter()
        .filter_map(|((item_id, variant), mut book)| {
            book.sort_unstable();
            let cheapest = book.first()?.0;
            let quantity: u32 = book.iter().map(|(_, count)| *count).sum();
            Some(Depth {
                item_id,
                variant,
                cheapest,
                quantity,
                listings: book.len() as u32,
                tenth: percentile(&book, quantity, 10),
                median: percentile(&book, quantity, 50),
            })
        })
        .collect();
    out.sort_unstable();
    out
}

/// The unit price you reach after buying `share` percent of what is listed.
///
/// The book is sorted cheapest first, so this walks it accumulating quantity
/// and answers with the price of the listing the target lands in — which is
/// the price somebody would actually be paying by then, not an average of the
/// ones they walked past.
fn percentile(book: &[(u64, u32)], quantity: u32, share: u32) -> u64 {
    if quantity == 0 {
        return book.first().map(|(price, _)| *price).unwrap_or(0);
    }
    let target = (u64::from(quantity) * u64::from(share))
        .div_ceil(100)
        .max(1);
    let mut seen = 0u64;
    for (price, count) in book {
        seen += u64::from(*count);
        if seen >= target {
            return *price;
        }
    }
    book.last().map(|(price, _)| *price).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_index_gives_hrefs_and_the_id_is_read_off_the_end() {
        // Blizzard has never given this index a plain id field.
        let body = br#"{"connected_realms":[
            {"href":"https://us.api.blizzard.com/data/wow/connected-realm/61?namespace=dynamic-us"},
            {"href":"https://us.api.blizzard.com/data/wow/connected-realm/3684?namespace=dynamic-us"}
        ]}"#;
        assert_eq!(
            parse_connected_realm_index(body).found(),
            Some(vec![61, 3684])
        );
    }

    #[test]
    fn the_realm_index_comes_back_alphabetical_not_in_the_order_they_opened() {
        // Blizzard orders by id, which is the order realms were opened in. A
        // person looking for theirs in a list of several hundred wants it
        // where the alphabet says.
        let body = br#"{"realms":[
            {"id":61,"name":"Mannoroth","slug":"mannoroth"},
            {"id":1567,"name":"Emerald Dream","slug":"emerald-dream"}]}"#;
        let realms = parse_realm_index(body).found().expect("realms");
        assert_eq!(realms[0].name, "Emerald Dream");
        assert_eq!(realms[0].slug, "emerald-dream");
        assert_eq!(realms[1].name, "Mannoroth");
    }

    #[test]
    fn a_realm_says_which_auction_house_it_trades_in() {
        // The realm a character is on and the auction house they trade in are
        // different numbers, and only this call joins them.
        let body = br#"{"id":1567,"name":"Terenas","connected_realm":
            {"href":"https://us.api.blizzard.com/data/wow/connected-realm/61?namespace=dynamic-us"}}"#;
        assert_eq!(parse_realm_connection(body), Outcome::Found(61));
    }

    #[test]
    fn a_realm_with_no_connection_is_stale_rather_than_realm_zero() {
        // Realm zero is region-wide commodities. Defaulting to it would file a
        // realm's prices under the wrong market entirely.
        assert!(matches!(
            parse_realm_connection(br#"{"id":1567,"name":"Terenas"}"#),
            Outcome::Stale(_)
        ));
    }

    #[test]
    fn a_connected_realm_names_every_realm_that_shares_its_auction_house() {
        // Several of a person's characters can land in one group without them
        // realising, which is the thing worth showing.
        let body = br#"{"id":61,"realms":[
            {"id":61,"name":"Emerald Dream","slug":"emerald-dream"},
            {"id":1567,"name":"Terenas","slug":"terenas"}]}"#;
        let realm = parse_connected_realm(body).found().expect("a realm");
        assert_eq!(realm.id, 61);
        assert_eq!(realm.realms, ["Emerald Dream", "Terenas"]);
        assert_eq!(realm.slugs, ["emerald-dream", "terenas"]);
    }

    #[test]
    fn commodities_carry_a_unit_price_and_nothing_else() {
        // No bid, no bonus ids, no variance — a stack of herbs is a stack of
        // herbs. That emptiness is the point rather than a gap.
        let body = br#"{"auctions":[
            {"id":1,"item":{"id":197794},"quantity":20,"unit_price":56523,"time_left":"SHORT"}]}"#;
        let listings = parse_commodities(body).found().expect("listings");
        assert_eq!(listings[0].item_id, 197794);
        assert_eq!(listings[0].unit_price, 56523);
        assert_eq!(listings[0].variant, "");
        assert_eq!(listings[0].pet_species, None);
    }

    #[test]
    fn a_realm_auction_prices_per_unit_from_its_buyout() {
        let body = br#"{"auctions":[
            {"id":1,"item":{"id":6513},"quantity":4,"buyout":4000,"time_left":"LONG"}]}"#;
        let listings = parse_auctions(body).found().expect("listings");
        assert_eq!(listings[0].unit_price, 1000);
        assert_eq!(listings[0].quantity, 4);
    }

    #[test]
    fn a_bid_only_auction_is_skipped_rather_than_priced() {
        // Its price is what somebody hopes to get, not what the item costs.
        // Mixing the two into one history makes both meaningless.
        let body = br#"{"auctions":[
            {"id":1,"item":{"id":6513},"quantity":1,"bid":300,"time_left":"LONG"}]}"#;
        assert_eq!(parse_auctions(body), Outcome::Empty);
    }

    #[test]
    fn a_variant_fingerprint_is_stable_whatever_order_blizzard_sends() {
        // Blizzard's order is not stable between snapshots, and an unsorted
        // join would make one item look like two.
        let one = br#"{"auctions":[{"item":{"id":1,"bonus_lists":[4279,1532],
            "modifiers":[{"type":28,"value":1},{"type":9,"value":70}]},
            "quantity":1,"buyout":100}]}"#;
        let two = br#"{"auctions":[{"item":{"id":1,"bonus_lists":[1532,4279],
            "modifiers":[{"type":9,"value":70},{"type":28,"value":1}]},
            "quantity":1,"buyout":100}]}"#;

        let a = parse_auctions(one).found().expect("one");
        let b = parse_auctions(two).found().expect("two");
        assert_eq!(a[0].variant, b[0].variant);
        assert_eq!(a[0].variant, "b1532,b4279,m9:70,m28:1");
    }

    #[test]
    fn a_pet_keeps_its_species_or_every_pet_prices_as_one_thing() {
        let body = br#"{"auctions":[{"item":{"id":82800,"pet_species_id":1442,
            "pet_level":1,"pet_quality_id":3},"quantity":1,"buyout":50000}]}"#;
        let listings = parse_auctions(body).found().expect("listings");
        assert_eq!(listings[0].pet_species, Some(1442));
    }

    #[test]
    fn the_cheapest_listing_is_the_price_and_the_quantities_add_up() {
        // The market price of a thing is what it costs to buy one. An average
        // is dragged upward by the hopeful listing nobody will ever take.
        let listings = vec![
            Listing {
                item_id: 1,
                unit_price: 900,
                quantity: 5,
                variant: String::new(),
                pet_species: None,
                pet_quality: None,
            },
            Listing {
                item_id: 1,
                unit_price: 100,
                quantity: 2,
                variant: String::new(),
                pet_species: None,
                pet_quality: None,
            },
            Listing {
                item_id: 1,
                unit_price: 5000,
                quantity: 1,
                variant: "b1532".into(),
                pet_species: None,
                pet_quality: None,
            },
        ];

        let book = depth(&listings);
        assert_eq!(book.len(), 2, "a variant is a different thing");
        assert_eq!(book[0].cheapest, 100);
        assert_eq!(book[0].quantity, 7);
        assert_eq!(book[1].variant, "b1532");
        assert_eq!(book[1].cheapest, 5000);
    }

    #[test]
    fn the_shape_of_the_book_survives_the_snapshot() {
        // One lowball at a hundred and four hundred units at nine hundred is
        // not a hundred-gold market, and the cheapest price on its own cannot
        // say which of the two you are looking at.
        let listed = |price, quantity| Listing {
            item_id: 1,
            unit_price: price,
            quantity,
            variant: String::new(),
            pet_species: None,
            pet_quality: None,
        };
        let book = depth(&[listed(900, 400), listed(100, 1), listed(950, 99)]);

        assert_eq!(book.len(), 1);
        assert_eq!(book[0].cheapest, 100, "what one costs");
        assert_eq!(book[0].quantity, 500);
        assert_eq!(book[0].listings, 3, "one goblin or forty sellers");
        // A tenth of five hundred is fifty units, which is well past the single
        // cheap one and into the wall at nine hundred.
        assert_eq!(book[0].tenth, 900);
        assert_eq!(book[0].median, 900);

        // The same total stock, actually cheap.
        let real = depth(&[listed(100, 500)]);
        assert_eq!(real[0].cheapest, 100);
        assert_eq!(real[0].tenth, 100);
        assert_eq!(real[0].median, 100);
    }

    #[test]
    fn the_token_price_is_copper() {
        assert_eq!(
            parse_token(br#"{"last_updated_timestamp":1,"price":2500000000}"#),
            Outcome::Found(2_500_000_000)
        );
    }

    #[test]
    fn auctions_are_asked_for_in_the_dynamic_namespace() {
        // Static would 404, and the 404 reads like a realm that does not exist.
        assert!(auctions(Region::Us, 61)
            .url
            .contains("namespace=dynamic-us"));
        assert!(commodities(Region::Us).url.contains("namespace=dynamic-us"));
    }
}
