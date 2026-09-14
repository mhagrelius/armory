using Armory.Blizzard;
using Armory.Collections;
using Armory.Roster;

namespace Armory.Market;

/// <summary>One thing the account has not collected, currently for sale.</summary>
public sealed record Offer
{
    public Kind Kind { get; init; }

    /// <summary>The collectible's own id: a species for a pet, a decor id for decor.</summary>
    public long CollectibleId { get; init; }

    public string Name { get; init; } = "";

    /// <summary>The connected realm it is listed on, or zero for region-wide commodities.</summary>
    public long Realm { get; init; }

    /// <summary>Copper, for one.</summary>
    public long UnitPrice { get; init; }

    /// <summary>How many are up. A single listing is somebody's hopeful price; ten is a market.</summary>
    public long Quantity { get; init; }
}

/// <summary>
/// One item as the market currently has it. The whole snapshot rather than
/// the watch list, affordable because the response it comes from is already
/// downloaded in full every sync.
/// </summary>
public sealed record Listed
{
    public long ItemId { get; init; }

    /// <summary>What it is called, once a name has been fetched for it. Null is normal: names arrive one at a time.</summary>
    public string? Name { get; init; }

    public long Cheapest { get; init; }

    public long Quantity { get; init; }

    public long Listings { get; init; }

    public long Tenth { get; init; }

    public long Median { get; init; }

    /// <summary>Units that left the listings across the watched window, where there is one. Zero for an item nobody is watching.</summary>
    public long Sold { get; init; }

    public long SpanHours { get; init; }

    /// <summary>What a person would call it: the name, or the id until one arrives.</summary>
    public string Title() => Name ?? $"Item {ItemId}";

    /// <summary>What the whole listed stock is worth at the cheapest price: the rough measure of how much market there is here.</summary>
    public long Depth()
    {
        try
        {
            return checked(Cheapest * Quantity);
        }
        catch (OverflowException)
        {
            return long.MaxValue;
        }
    }
}

/// <summary>One craft worth making, and who should make it.</summary>
public sealed record Making
{
    public long Recipe { get; init; }

    public string Name { get; init; } = "";

    /// <summary>Who can make it. A recipe two characters both know appears once.</summary>
    public required CharacterKey By { get; init; }

    public string ByName { get; init; } = "";

    public long Realm { get; init; }

    public string RealmName { get; init; } = "";

    public long Makes { get; init; }

    /// <summary>What the reagents cost for one craft, at the cheapest priced tier.</summary>
    public long Cost { get; init; }

    /// <summary>What one of the output has been going for.</summary>
    public long Each { get; init; }

    /// <summary>What one craft brings in after the auction house's cut.</summary>
    public long Revenue { get; init; }

    /// <summary>Revenue minus cost. Always positive here.</summary>
    public long Margin { get; init; }

    /// <summary>Units of the output that left the listings across the window.</summary>
    public long Sold { get; init; }

    /// <summary>How many snapshots the figures rest on. Two is a rumour.</summary>
    public int Samples { get; init; }

    public long SpanHours { get; init; }

    /// <summary>Reagents already sitting in the Warband bank, as (item, count). Shown, and deliberately not counted.</summary>
    public List<(long Item, long Count)> Held { get; init; } = [];
}

/// <summary>What could not be answered, and why.</summary>
public readonly record struct Unmeasured(int MissingReagent, int MissingOutput);

/// <summary>What the account's recipe books are worth, and what could not be answered.</summary>
public sealed record Crafting
{
    public List<Making> Worth { get; init; } = [];

    public Unmeasured Unmeasured { get; init; }
}

/// <summary>One snapshot of one market: what it cost, and what shape the book was in. The clock is carried so a rate can be said.</summary>
public readonly record struct Sample(DateTime At, long Cheapest, long Quantity, long Listings, long Tenth, long Median);

/// <summary>Every price series recorded for one item on one realm, keyed by <see cref="Listing.Series"/>, oldest sample first.</summary>
public sealed class Series : Dictionary<string, List<Sample>>
{
}

/// <summary>One realm's market: its connected-realm id, its name, and its series.</summary>
public sealed record RealmMarket(long Realm, string Name, Series Series);

/// <summary>One spare pet, and the realm to sell it on.</summary>
public sealed record Resale
{
    public long Species { get; init; }

    public string Name { get; init; } = "";

    /// <summary>Copies beyond the one worth keeping. Never zero.</summary>
    public long Spare { get; init; }

    public long Realm { get; init; }

    public string RealmName { get; init; } = "";

    /// <summary>What the cheapest quality of this pet has been going for. The floor, because the journal's per-pet quality is not read.</summary>
    public long Floor { get; init; }

    /// <summary>What the dearest quality has been going for.</summary>
    public long Ceiling { get; init; }

    /// <summary>Units that vanished from the listings between snapshots, summed across every quality. Inferred, and it has to be.</summary>
    public long Sold { get; init; }

    public int Samples { get; init; }

    public long SpanHours { get; init; }
}

/// <summary>
/// What is missing, and on sale: the intersection of the catalogue's owned set
/// and the listings. Only pets join cleanly, and that is a fact about the game:
/// every caged pet is item 82800 with the species beside it, a mount's record
/// names its spell rather than its item, and toys and decor are untradeable.
/// </summary>
public static class Markets
{
    /// <summary>The auction house's cut of a sale, as a percentage. The deposit comes back on a sale and is not modelled.</summary>
    public const long Cut = 5;

    /// <summary>Which of the missing entries are on sale right now, cheapest first.</summary>
    public static List<Offer> OnSale(IReadOnlyList<Collectible> catalogue, IReadOnlySet<long> owned, IReadOnlyList<Listing> listings, long realm)
    {
        var missing = new Dictionary<long, Collectible>();
        foreach (var entry in catalogue)
        {
            if (!owned.Contains(entry.Id))
            {
                missing[JoinKey(entry)] = entry;
            }
        }
        if (missing.Count == 0)
        {
            return [];
        }

        // Cheapest per collectible, and the quantities added up. Two listings
        // of the same pet are one answer with a count, not two rows.
        var best = new Dictionary<long, Offer>();
        foreach (var listing in listings)
        {
            if (!missing.TryGetValue(ListingKey(listing), out var entry))
            {
                continue;
            }
            if (!best.TryGetValue(entry.Id, out var offer))
            {
                offer = new Offer { Kind = entry.Kind, CollectibleId = entry.Id, Name = entry.Name, Realm = realm, UnitPrice = listing.UnitPrice, Quantity = 0 };
            }
            best[entry.Id] = offer with
            {
                UnitPrice = Math.Min(offer.UnitPrice, listing.UnitPrice),
                Quantity = offer.Quantity + listing.Quantity,
            };
        }

        return best.Values.OrderBy(offer => offer.UnitPrice).ThenBy(offer => offer.Name, StringComparer.Ordinal).ToList();
    }

    /// <summary>What a catalogue entry would be listed under: a pet by species, everything else by item.</summary>
    private static long JoinKey(Collectible entry) => entry.Kind == Kind.Pet ? entry.Id : entry.LinkId;

    /// <summary>What a listing is offering, in the same space as <see cref="JoinKey"/>. A caged pet is answered by its species and never by its item.</summary>
    private static long ListingKey(Listing listing) => listing.PetSpecies ?? listing.ItemId;

    /// <summary>Which unnamed items are worth a name first: the ones being traded in volume, not the display order.</summary>
    public static List<long> WorthNaming(IReadOnlyList<Listed> market, int budget) =>
        market.Where(listed => listed.Name is null)
            .OrderByDescending(listed => listed.Listings)
            .ThenByDescending(listed => listed.Quantity)
            .Take(budget)
            .Select(listed => listed.ItemId)
            .ToList();

    /// <summary>Search one realm's snapshot. Filtering only; ordering is the column view's. Matches the name where there is one and the id otherwise.</summary>
    public static List<Listed> Browse(IReadOnlyList<Listed> market, string needle)
    {
        var wanted = needle.Trim().ToLowerInvariant();
        return market.Where(listed => wanted.Length == 0
                || (listed.Name is { } name
                    ? name.ToLowerInvariant().Contains(wanted, StringComparison.Ordinal)
                    : listed.ItemId.ToString(System.Globalization.CultureInfo.InvariantCulture).Contains(wanted, StringComparison.Ordinal)))
            .ToList();
    }

    /// <summary>
    /// Which of the account's recipes are worth making, and where to sell
    /// them. Somebody knows it, every reagent has a price, the output has a
    /// price, and the margin is positive. Quality one throughout, as a floor.
    /// </summary>
    public static Crafting WorthMaking(RecipeBooks books, IReadOnlyDictionary<CharacterKey, string> names, IReadOnlyList<RealmMarket> markets, IReadOnlyDictionary<long, long> warbandBank)
    {
        var best = new Dictionary<long, Making>();
        var missingReagent = 0;
        var missingOutput = 0;

        foreach (var (character, recipes) in books)
        {
            foreach (var recipe in recipes)
            {
                var answered = false;
                var shortReagent = false;
                var shortOutput = false;

                foreach (var (realm, realmName, series) in markets)
                {
                    // The output first: a recipe whose product nobody lists
                    // cannot be costed against anything.
                    if (Quote(series, recipe.Output) is not { } output)
                    {
                        shortOutput = true;
                        continue;
                    }

                    long cost = 0;
                    var priced = true;
                    foreach (var reagent in recipe.Reagents)
                    {
                        // The cheapest tier that has a price: a recipe can be
                        // made with any of them.
                        var tier = reagent.Tiers.Select(item => Quote(series, item)?.Price).Where(price => price is not null).Min();
                        if (tier is { } price)
                        {
                            cost += price * reagent.Quantity;
                        }
                        else
                        {
                            priced = false;
                            break;
                        }
                    }
                    if (!priced)
                    {
                        shortReagent = true;
                        continue;
                    }

                    var revenue = output.Price * recipe.Makes * (100 - Cut) / 100;
                    var margin = revenue - cost;
                    answered = true;
                    if (margin <= 0)
                    {
                        continue;
                    }

                    var candidate = new Making
                    {
                        Recipe = recipe.Id,
                        Name = recipe.Name,
                        By = character,
                        ByName = names.TryGetValue(character, out var known) ? known : character.DisplayName(),
                        Realm = realm,
                        RealmName = realmName,
                        Makes = recipe.Makes,
                        Cost = cost,
                        Each = output.Price,
                        Revenue = revenue,
                        Margin = margin,
                        Sold = output.Sold,
                        Samples = output.Samples,
                        SpanHours = output.SpanHours,
                        Held = recipe.Reagents.SelectMany(reagent => reagent.Tiers)
                            .Where(warbandBank.ContainsKey)
                            .Select(item => (item, warbandBank[item]))
                            .ToList(),
                    };

                    // Which realm to make it on: the one where the craft is
                    // worth most. Ties go to the realm actually moving them.
                    var better = !best.TryGetValue(recipe.Id, out var held)
                        || (candidate.Margin, candidate.Sold).CompareTo((held.Margin, held.Sold)) > 0;
                    if (better)
                    {
                        best[recipe.Id] = candidate;
                    }
                }

                // Counted once per recipe rather than once per realm, and only
                // when no realm could answer it at all.
                if (!answered)
                {
                    if (shortReagent)
                    {
                        missingReagent++;
                    }
                    else if (shortOutput)
                    {
                        missingOutput++;
                    }
                }
            }
        }

        // Realisable profit, not paper margin: what one craft is worth, times
        // how many of them the market has actually absorbed.
        static long Worth(Making making) => Math.Max(making.Margin, 0) * making.Sold;
        var worth = best.Values
            .OrderByDescending(Worth)
            .ThenByDescending(making => making.Margin)
            .ThenBy(making => making.Name, StringComparer.Ordinal)
            .ToList();
        return new Crafting { Worth = worth, Unmeasured = new Unmeasured(missingReagent, missingOutput) };
    }

    /// <summary>What one item is going for on one realm, and how much of it has moved. Null when nothing has ever been recorded for it.</summary>
    private static (long Price, long Sold, int Samples, long SpanHours)? Quote(Series series, long item)
    {
        // Commodities carry no variant, so a reagent's series key is its item id.
        if (!series.TryGetValue(item.ToString(System.Globalization.CultureInfo.InvariantCulture), out var samples) || samples.Count == 0)
        {
            return null;
        }
        return (Typical(samples), Sold(samples), samples.Count, SpanHours(samples));
    }

    /// <summary>How many of something move in a day, from a count and the span it covers.</summary>
    public static double PerDay(long sold, long spanHours) => sold * 24.0 / Math.Max(spanHours, 1);

    /// <summary>
    /// Which spare pets are worth selling, and where. It can be caged, there
    /// is a spare, and somebody has been selling it.
    /// </summary>
    public static List<Resale> WorthSelling(IReadOnlyList<Collectible> catalogue, IReadOnlyDictionary<long, long> held, IReadOnlyList<RealmMarket> markets)
    {
        var sellable = catalogue
            .Where(entry => entry.Kind == Kind.Pet && entry.Tradeable == true && held.GetValueOrDefault(entry.Id) > 1)
            .ToDictionary(entry => entry.Id);
        if (sellable.Count == 0)
        {
            return [];
        }

        var best = new Dictionary<long, Resale>();
        foreach (var (realm, realmName, series) in markets)
        {
            // Every quality of one species, folded back together. The series
            // are stored apart because they are different goods.
            var perSpecies = new Dictionary<long, (long Floor, long Ceiling, long Sold, int Samples, long SpanHours)>();
            foreach (var (key, samples) in series)
            {
                if (Auctions.PetSeries(key) is not { } pet || !sellable.ContainsKey(pet.Species) || samples.Count == 0)
                {
                    continue;
                }
                var price = Typical(samples);
                var entry = perSpecies.TryGetValue(pet.Species, out var seen) ? seen : (price, price, 0, 0, 1);
                perSpecies[pet.Species] = (
                    Math.Min(entry.Floor, price),
                    Math.Max(entry.Ceiling, price),
                    entry.Sold + Sold(samples),
                    entry.Samples + samples.Count,
                    // The longest-watched quality, because the rate has to be
                    // over a span that covers the folded qualities.
                    Math.Max(entry.SpanHours, SpanHours(samples)));
            }

            foreach (var (species, figures) in perSpecies)
            {
                var entry = sellable[species];
                var candidate = new Resale
                {
                    Species = species,
                    Name = entry.Name,
                    Spare = Math.Max(held.GetValueOrDefault(species, 1) - 1, 0),
                    Realm = realm,
                    RealmName = realmName,
                    Floor = figures.Floor,
                    Ceiling = figures.Ceiling,
                    Sold = figures.Sold,
                    Samples = figures.Samples,
                    SpanHours = figures.SpanHours,
                };
                // Which realm makes sense: the one paying most for the same
                // pet, ties to the realm that has been moving them.
                var better = !best.TryGetValue(species, out var standing)
                    || (candidate.Floor, candidate.Sold).CompareTo((standing.Floor, standing.Sold)) > 0;
                if (better)
                {
                    best[species] = candidate;
                }
            }
        }

        return best.Values
            .OrderByDescending(resale => resale.Floor)
            .ThenByDescending(resale => resale.Sold)
            .ThenBy(resale => resale.Name, StringComparer.Ordinal)
            .ToList();
    }

    /// <summary>The going rate for a series: the median of the cheapest listing over time. Not the latest, and not the mean.</summary>
    public static long Typical(IReadOnlyList<Sample> samples)
    {
        if (samples.Count == 0)
        {
            return 0;
        }
        var prices = samples.Select(sample => sample.Cheapest).Order().ToList();
        return prices[prices.Count / 2];
    }

    /// <summary>How many units left the listings across a series. Only the falls: a rise is somebody listing more.</summary>
    public static long Sold(IReadOnlyList<Sample> samples)
    {
        long fell = 0;
        for (var i = 1; i < samples.Count; i++)
        {
            var drop = samples[i - 1].Quantity - samples[i].Quantity;
            if (drop > 0)
            {
                fell += drop;
            }
        }
        return fell;
    }

    /// <summary>How long a series covers, in hours. The span actually observed, never zero.</summary>
    public static long SpanHours(IReadOnlyList<Sample> samples)
    {
        if (samples.Count == 0)
        {
            return 1;
        }
        var hours = (long)Math.Floor((samples[^1].At - samples[0].At).TotalHours);
        return Math.Max(hours, 1);
    }
}
