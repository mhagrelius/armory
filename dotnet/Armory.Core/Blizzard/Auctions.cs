using System.Globalization;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;

namespace Armory.Blizzard;

/// <summary>A connected realm: one shared auction house for everything but commodities.</summary>
public sealed record ConnectedRealm
{
    [JsonPropertyName("id")]
    public long Id { get; init; }

    /// <summary>Every realm that shares this auction house.</summary>
    [JsonPropertyName("realms")]
    public List<string> Realms { get; init; } = [];

    [JsonPropertyName("slugs")]
    public List<string> Slugs { get; init; } = [];
}

/// <summary>
/// One listing, flattened to what a price history needs. Bonus ids and
/// modifiers are kept as an opaque fingerprint for grouping rather than
/// interpreted into stats we would be guessing at.
/// </summary>
public sealed record Listing
{
    /// <summary>Every caged battle pet in the game, listed under one item id.</summary>
    public const long CagedPet = 82800;

    public long ItemId { get; init; }

    /// <summary>Copper per unit. Commodities give this directly; realm auctions divide buyout by quantity.</summary>
    public long UnitPrice { get; init; }

    public long Quantity { get; init; } = 1;

    /// <summary>Bonus ids and modifiers, joined. Empty for commodities, which have no variance.</summary>
    public string Variant { get; init; } = "";

    /// <summary>Battle pets are listed under one item id with the species in the item.</summary>
    public long? PetSpecies { get; init; }

    /// <summary>A caged pet's quality: 1 poor through 4 rare. The biggest thing about a pet's price after its species.</summary>
    public long? PetQuality { get; init; }

    /// <summary>
    /// What this is, as far as a price history is concerned: the fingerprint
    /// for anything but a pet, and the species and quality for a pet, because
    /// every caged pet is item 82800 with no bonuses. Level is deliberately not
    /// in the key.
    /// </summary>
    public string Series() => PetSpecies is { } species
        ? string.Create(CultureInfo.InvariantCulture, $"pet{species}:{PetQuality ?? 0}")
        : Variant;
}

/// <summary>One realm as a person names it, distinct from the connected realm they trade in.</summary>
public sealed record Realm
{
    [JsonPropertyName("id")]
    public long Id { get; init; }

    [JsonPropertyName("name")]
    public string Name { get; init; } = "";

    [JsonPropertyName("slug")]
    public string Slug { get; init; } = "";
}

/// <summary>
/// One item's market on one realm, at one moment: the shape of the book in
/// six numbers, because the cheapest price alone cannot tell one lowball at a
/// hundred gold from four hundred units at a hundred gold.
/// </summary>
public sealed record Depth
{
    public long ItemId { get; init; }

    public string Variant { get; init; } = "";

    /// <summary>The cheapest unit price. What it costs to buy exactly one.</summary>
    public long Cheapest { get; init; }

    /// <summary>Units listed, across every auction of it.</summary>
    public long Quantity { get; init; }

    /// <summary>How many separate auctions those units are spread across: forty sellers or one.</summary>
    public long Listings { get; init; }

    /// <summary>The unit price a tenth of the way into the book, by quantity.</summary>
    public long Tenth { get; init; }

    /// <summary>The unit price halfway into the book, by quantity. The real middle.</summary>
    public long Median { get; init; }
}

/// <summary>
/// The auction house. Commodities are region-wide and priced by unit alone;
/// everything else is locked to a connected realm and carries a buyout and
/// bonus ids. Blizzard publishes no price history and no sale signal, so
/// history is accumulated locally and sales are inferred by diffing.
/// </summary>
public static class Auctions
{
    private const SourceId Source = SourceId.BlizzardGameData;

    private static Request Dynamic(Region region, string path) => Request.Get(Source, Api.Url(region, Namespace.Dynamic, path));

    /// <summary>Every realm in the region, by name. What a realm picker is built from.</summary>
    public static Request RealmIndex(Region region) => Dynamic(region, "/data/wow/realm/index");

    /// <summary>One realm, which is how its connected realm is found.</summary>
    public static Request RealmOf(Region region, string slug) => Dynamic(region, $"/data/wow/realm/{slug}");

    public static Request ConnectedRealmIndex(Region region) => Dynamic(region, "/data/wow/connected-realm/index");

    public static Request ConnectedRealmOf(Region region, long id) => Dynamic(region, string.Create(CultureInfo.InvariantCulture, $"/data/wow/connected-realm/{id}"));

    /// <summary>A connected realm's non-commodity auctions.</summary>
    public static Request AuctionsOf(Region region, long connectedRealmId) => Dynamic(region, string.Create(CultureInfo.InvariantCulture, $"/data/wow/connected-realm/{connectedRealmId}/auctions"));

    /// <summary>The region's commodities: one document, 25x quota per call, charged even for a 304.</summary>
    public static Request Commodities(Region region) => Dynamic(region, "/data/wow/auctions/commodities");

    /// <summary>The WoW Token's current price, in copper.</summary>
    public static Request Token(Region region) => Dynamic(region, "/data/wow/token/index");

    /// <summary>Read the realm index into names and slugs, alphabetical rather than in the order they opened.</summary>
    public static Outcome<List<Realm>> ParseRealmIndex(byte[] body)
    {
        var parsed = Outcomes.ParseJson<List<Realm>>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        if (parsed.Value.At("realms") is not JsonArray list)
        {
            return new Outcome<List<Realm>>.Stale(new Reason.Malformed("the realm index carried no realms"));
        }
        var realms = new List<Realm>();
        foreach (var entry in list)
        {
            if (entry.At("id").Int() is { } id && entry.At("name").Str() is { } name && entry.At("slug").Str() is { } slug)
            {
                realms.Add(new Realm { Id = id, Name = name, Slug = slug });
            }
        }
        realms.Sort((a, b) => string.CompareOrdinal(a.Name, b.Name));
        return Outcomes.OfCollection(realms);
    }

    /// <summary>Read which connected realm a realm trades in. An href rather than an id.</summary>
    public static Outcome<long> ParseRealmConnection(byte[] body)
    {
        var parsed = Outcomes.ParseJson<long>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        return parsed.Value.At("connected_realm").At("href").Str() is { } href && IdFromHref(href) is { } id
            ? new Outcome<long>.Found(id)
            : new Outcome<long>.Stale(new Reason.Malformed("a realm with no connected realm"));
    }

    /// <summary>Read the connected-realm index into ids, read off the end of each href.</summary>
    public static Outcome<List<long>> ParseConnectedRealmIndex(byte[] body)
    {
        var parsed = Outcomes.ParseJson<List<long>>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        if (parsed.Value.At("connected_realms") is not JsonArray list)
        {
            return new Outcome<List<long>>.Stale(new Reason.Malformed("the connected-realm index carried no connected_realms"));
        }
        return Outcomes.OfCollection(list.Select(entry => entry.At("href").Str()).OfType<string>().Select(IdFromHref).OfType<long>().ToList());
    }

    private static long? IdFromHref(string href)
    {
        var tail = href.Split('/').LastOrDefault(segment => segment.Length > 0);
        if (tail is null)
        {
            return null;
        }
        var query = tail.IndexOf('?', StringComparison.Ordinal);
        var digits = query < 0 ? tail : tail[..query];
        return long.TryParse(digits, NumberStyles.Integer, CultureInfo.InvariantCulture, out var id) ? id : null;
    }

    /// <summary>Read a connected realm's member realms.</summary>
    public static Outcome<ConnectedRealm> ParseConnectedRealm(byte[] body)
    {
        var parsed = Outcomes.ParseJson<ConnectedRealm>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        if (parsed.Value.At("id").Int() is not { } id)
        {
            return new Outcome<ConnectedRealm>.Stale(new Reason.Malformed("a connected realm with no id"));
        }
        var realms = parsed.Value.At("realms").Items().ToList();
        return new Outcome<ConnectedRealm>.Found(new ConnectedRealm
        {
            Id = id,
            Realms = realms.Select(realm => realm.At("name").Str()).OfType<string>().ToList(),
            Slugs = realms.Select(realm => realm.At("slug").Str()).OfType<string>().ToList(),
        });
    }

    /// <summary>Read the region's commodity listings.</summary>
    public static Outcome<List<Listing>> ParseCommodities(byte[] body)
    {
        var parsed = Outcomes.ParseJson<List<Listing>>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        if (parsed.Value.At("auctions") is not JsonArray list)
        {
            return new Outcome<List<Listing>>.Stale(new Reason.Malformed("the commodities response carried no auctions"));
        }
        var listings = new List<Listing>();
        foreach (var entry in list)
        {
            if (entry.At("item").At("id").Int() is { } itemId && entry.At("unit_price").Int() is { } unitPrice)
            {
                listings.Add(new Listing { ItemId = itemId, UnitPrice = unitPrice, Quantity = entry.At("quantity").Int() ?? 1 });
            }
        }
        return Outcomes.OfCollection(listings);
    }

    /// <summary>Read a connected realm's non-commodity listings. A bid-only auction is skipped: its price is what somebody hopes to get.</summary>
    public static Outcome<List<Listing>> ParseAuctions(byte[] body)
    {
        var parsed = Outcomes.ParseJson<List<Listing>>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        if (parsed.Value.At("auctions") is not JsonArray list)
        {
            return new Outcome<List<Listing>>.Stale(new Reason.Malformed("the auctions response carried no auctions"));
        }
        var listings = new List<Listing>();
        foreach (var entry in list)
        {
            var item = entry.At("item");
            if (item.At("id").Int() is not { } itemId || entry.At("buyout").Int() is not { } buyout)
            {
                continue;
            }
            var quantity = entry.At("quantity").Int() ?? 1;
            listings.Add(new Listing
            {
                ItemId = itemId,
                UnitPrice = buyout / Math.Max(quantity, 1),
                Quantity = quantity,
                Variant = VariantOf(item),
                PetSpecies = item.At("pet_species_id").Int(),
                PetQuality = item.At("pet_quality_id").Int(),
            });
        }
        return Outcomes.OfCollection(listings);
    }

    /// <summary>A fingerprint for the bonus ids and modifiers on an item, sorted because Blizzard's order is not stable between snapshots.</summary>
    private static string VariantOf(JsonNode? item)
    {
        var parts = new List<string>();
        var bonuses = item.At("bonus_lists").Items().Select(id => id.Int()).OfType<long>().OrderBy(id => id);
        parts.AddRange(bonuses.Select(id => string.Create(CultureInfo.InvariantCulture, $"b{id}")));
        var modifiers = item.At("modifiers").Items()
            .Select(modifier => (Type: modifier.At("type").Int(), Value: modifier.At("value").Int()))
            .Where(pair => pair.Type is not null && pair.Value is not null)
            .OrderBy(pair => pair.Type).ThenBy(pair => pair.Value);
        parts.AddRange(modifiers.Select(pair => string.Create(CultureInfo.InvariantCulture, $"m{pair.Type}:{pair.Value}")));
        return string.Join(",", parts);
    }

    /// <summary>Read the WoW Token price, in copper.</summary>
    public static Outcome<long> ParseToken(byte[] body)
    {
        var parsed = Outcomes.ParseJson<long>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        return parsed.Value.At("price").Int() is { } price
            ? new Outcome<long>.Found(price)
            : new Outcome<long>.Stale(new Reason.Malformed("the token response carried no price"));
    }

    /// <summary>Read a species and quality back out of a stored series key: the inverse of <see cref="Listing.Series"/>.</summary>
    public static (long Species, long Quality)? PetSeries(string series)
    {
        if (!series.StartsWith("pet", StringComparison.Ordinal))
        {
            return null;
        }
        var colon = series.IndexOf(':', StringComparison.Ordinal);
        if (colon < 0)
        {
            return null;
        }
        return long.TryParse(series[3..colon], NumberStyles.Integer, CultureInfo.InvariantCulture, out var species)
            && long.TryParse(series[(colon + 1)..], NumberStyles.Integer, CultureInfo.InvariantCulture, out var quality)
            ? (species, quality)
            : null;
    }

    /// <summary>
    /// Each item's market shape out of one snapshot. Not the mean anywhere:
    /// the market price of a thing is what it costs to buy one, and the
    /// percentiles are weighted by quantity because a stack of four hundred
    /// and a stack of one are not one vote each.
    /// </summary>
    public static List<Depth> DepthOf(IEnumerable<Listing> listings)
    {
        var books = new Dictionary<(long, string), List<(long Price, long Count)>>();
        foreach (var listing in listings)
        {
            var key = (listing.ItemId, listing.Series());
            if (!books.TryGetValue(key, out var book))
            {
                book = [];
                books[key] = book;
            }
            book.Add((listing.UnitPrice, listing.Quantity));
        }
        var depths = new List<Depth>();
        foreach (var ((itemId, variant), book) in books)
        {
            if (book.Count == 0)
            {
                continue;
            }
            book.Sort();
            var quantity = book.Sum(entry => entry.Count);
            depths.Add(new Depth
            {
                ItemId = itemId,
                Variant = variant,
                Cheapest = book[0].Price,
                Quantity = quantity,
                Listings = book.Count,
                Tenth = Percentile(book, quantity, 10),
                Median = Percentile(book, quantity, 50),
            });
        }
        return depths.OrderBy(depth => depth.ItemId).ThenBy(depth => depth.Variant, StringComparer.Ordinal).ThenBy(depth => depth.Cheapest).ToList();
    }

    /// <summary>The unit price you reach after buying a share of what is listed, walking the book cheapest first.</summary>
    private static long Percentile(List<(long Price, long Count)> book, long quantity, long share)
    {
        if (quantity == 0)
        {
            return book.Count > 0 ? book[0].Price : 0;
        }
        var target = Math.Max((quantity * share + 99) / 100, 1);
        long seen = 0;
        foreach (var (price, count) in book)
        {
            seen += count;
            if (seen >= target)
            {
                return price;
            }
        }
        return book.Count > 0 ? book[^1].Price : 0;
    }
}
