using Armory.Blizzard;
using Armory.Collections;
using Armory.Market;
using Armory.Roster;

namespace Armory.Client.Shell;

/// <summary>Everything the Market page reads on one draw.</summary>
public sealed record MarketView
{
    /// <summary>The auction houses being fetched, by connected-realm id.</summary>
    public List<(long Id, string Name)> Realms { get; init; } = [];

    /// <summary>A quote per watched item per market that holds a history for it.</summary>
    public List<Quote> Quotes { get; init; } = [];

    /// <summary>Which market the browser is reading: a watched realm, or zero for the region-wide commodities.</summary>
    public long Browsing { get; init; }

    /// <summary>The browsed market as it stands now.</summary>
    public List<Listed> Market { get; init; } = [];

    /// <summary>The items whose history is being kept.</summary>
    public HashSet<long> Watching { get; init; } = [];

    public long? TokenPrice { get; init; }

    public List<Offer> Offers { get; init; } = [];

    public List<Resale> Resale { get; init; } = [];

    public Crafting Crafting { get; init; } = new();
}

/// <summary>
/// The auction house: what to fetch, what to keep, and what the page is
/// handed. The joins live in <see cref="Markets"/>; this supplies their
/// halves. The port of the GTK application's market half.
/// </summary>
public sealed partial class Account
{
    /// <summary>Bargains found in the last snapshot of each watched realm. Kept in memory, not stored: a listing is gone within the hour.</summary>
    private readonly List<Offer> offers = [];

    /// <summary>The WoW Token's price, the one price Blizzard sets rather than players.</summary>
    public long? TokenPrice { get; private set; }

    /// <summary>Which market the browser was last pointed at. Checked against the watch list on every draw rather than trusted.</summary>
    public long? Browsing { get; set; }

    /// <summary>
    /// Fetch the token price, the region-wide commodities, and each watched
    /// realm's auctions. No early return on an empty watch list: the
    /// region-wide commodity market is what the browser shows, and it has to
    /// arrive before anybody has asked for anything.
    /// </summary>
    public async Task SyncMarket(Token token, long generation)
    {
        var region = Settings.Region;
        var (watched, realms) = await Store.On(store => (
            store.WatchedItems().Match(items => items.Select(item => item.ItemId).ToHashSet(), _ => []),
            store.WatchedRealmList().Match(list => list, _ => [])));

        if (await FetchBare(token, generation, Auctions.Token(region)) is { } tokenBody
            && Auctions.ParseToken(tokenBody) is Outcome<long>.Found price)
        {
            TokenPrice = price.Value;
        }

        // Commodities: region-wide, realm zero. The expensive call, 25x quota
        // and charged even for a 304, made every sync rather than only for a
        // watch list because browsing the market is a question about the
        // whole of it.
        if (await FetchBare(token, generation, Auctions.Commodities(region)) is { } commodities
            && Auctions.ParseCommodities(commodities) is Outcome<List<Listing>>.Found region_wide)
        {
            await Record(0, region_wide.Value, watched);
        }

        foreach (var (realmId, _) in realms)
        {
            if (await FetchBare(token, generation, Auctions.AuctionsOf(region, realmId)) is { } body
                && Auctions.ParseAuctions(body) is Outcome<List<Listing>>.Found listings)
            {
                await Record(realmId, listings.Value, watched);
                await MatchCollection(realmId, listings.Value);
            }
        }
        Changed?.Invoke();
    }

    /// <summary>
    /// Keep one snapshot: the whole market for <i>now</i>, which costs
    /// nothing because the body was downloaded in full anyway, and a price
    /// history for the items somebody asked about plus the two exceptions
    /// to opting in. Caged pets, because every pet is item 82800 and the
    /// watch list cannot name one; reagents and crafted outputs, because
    /// <see cref="Markets.WorthMaking"/> cannot cost a craft against a price
    /// that was thrown away. Bounded by the account's own recipe books.
    /// </summary>
    public async Task Record(long realm, IReadOnlyList<Listing> listings, IReadOnlySet<long> watched)
    {
        var now = clock.GetUtcNow();
        var whole = Auctions.DepthOf(listings);
        await Store.On(store =>
        {
            store.RecordSnapshot(realm, whole, now);
            var recipeItems = store.RecipeItems().Match(items => items, _ => []);
            var mine = listings
                .Where(listing => watched.Contains(listing.ItemId) || recipeItems.Contains(listing.ItemId) || listing.PetSpecies is not null)
                .ToList();
            if (mine.Count > 0)
            {
                store.RecordPrices(realm, Auctions.DepthOf(mine), now);
            }
        });
    }

    /// <summary>Find what the account is missing in one realm's snapshot. This realm's answer replaces this realm's previous answer and leaves the others alone.</summary>
    private async Task MatchCollection(long realm, IReadOnlyList<Listing> listings)
    {
        var found = await Store.On(store =>
        {
            var offers = new List<Offer>();
            foreach (var kind in Links.AllKinds)
            {
                var (catalogue, owned) = store.CollectiblesHeld(kind).Match(held => held, _ => ([], []));
                if (catalogue.Count > 0)
                {
                    offers.AddRange(Markets.OnSale(catalogue, owned, listings, realm));
                }
            }
            return offers;
        });
        offers.RemoveAll(offer => offer.Realm == realm);
        offers.AddRange(found);
        offers.Sort((a, b) => a.UnitPrice != b.UnitPrice ? a.UnitPrice.CompareTo(b.UnitPrice) : string.CompareOrdinal(a.Name, b.Name));
    }

    // -- choosing what to watch ------------------------------------------------

    /// <summary>
    /// The region's realms, with the account's own first. The index is one
    /// call whose answer changes only when Blizzard opens or merges a
    /// realm, so it goes through the ordinary response cache and is almost
    /// always already there. The error is the sentence to say out loud.
    /// </summary>
    public async Task<Result<(List<Realm> Realms, List<string> Mine), string>> RealmChoices()
    {
        var request = Auctions.RealmIndex(Settings.Region);
        var mine = Roster.Realms().Select(realm => realm.Slug).ToList();
        var held = await Store.On(store => store.Response(request.Url, TimeSpan.FromDays(Armory.Store.Store.MaxTtlDays)).Match(body => body, _ => null));
        if (held is not null && Auctions.ParseRealmIndex(held) is Outcome<List<Realm>>.Found cached)
        {
            return Result<(List<Realm>, List<string>), string>.Ok((cached.Value, mine));
        }
        if (Token is not { } token)
        {
            return Result<(List<Realm>, List<string>), string>.Err("Sign in to fetch the realm list — it is the only way to find a realm's auction house.");
        }
        var body = await FetchBare(token, Generation, request);
        return body is not null && Auctions.ParseRealmIndex(body) is Outcome<List<Realm>>.Found fetched
            ? Result<(List<Realm>, List<string>), string>.Ok((fetched.Value, mine))
            : Result<(List<Realm>, List<string>), string>.Err("Blizzard did not answer with a realm list.");
    }

    /// <summary>
    /// Turn a realm into the auction house it trades in, and watch that. A
    /// realm and its market are different numbers: Terenas is realm 1567
    /// and trades in connected realm 61, so the id chosen in the list is
    /// never the id to fetch with.
    /// </summary>
    public async Task WatchRealm(Realm realm)
    {
        if (Token is not { } token)
        {
            Toasted?.Invoke("Sign in to add a realm — finding its auction house is a call to Blizzard.");
            return;
        }
        var body = await FetchBare(token, Generation, Auctions.RealmOf(Settings.Region, realm.Slug));
        var outcome = body is null ? new Outcome<long>.Unusable(new Reason.Network("no answer")) : Auctions.ParseRealmConnection(body);
        if (outcome is Outcome<long>.Found connected)
        {
            await Store.On(store => store.WatchRealm(connected.Value, realm.Name));
            Toasted?.Invoke($"Watching {realm.Name}. Prices start accumulating from the next sync.");
            Changed?.Invoke();
        }
        else
        {
            var reason = outcome.Gap() ?? new Reason.Malformed("Blizzard did not say which auction house that realm uses");
            Toasted?.Invoke($"Could not add {realm.Name}: {reason}");
        }
    }

    /// <summary>Search Blizzard's catalogue for an item by name. Empty when signed out, and the toast says so.</summary>
    public async Task<List<(long Id, string Name)>> SearchItems(string text)
    {
        if (Token is not { } token)
        {
            Toasted?.Invoke("Sign in to search — the catalogue is Blizzard's.");
            return [];
        }
        var region = Settings.Region;
        var body = await FetchBare(token, Generation, GameData.ItemSearch(region, text));
        return body is not null && GameData.ParseItemSearch(body, region.DefaultLocale()) is Outcome<List<(long, string)>>.Found found ? found.Value : [];
    }

    public async Task WatchItem(long itemId, string name)
    {
        await Store.On(store => store.WatchItem(itemId, name));
        Toasted?.Invoke($"Watching {name}. Blizzard publishes no history, so the first snapshot is where yours starts.");
        Changed?.Invoke();
    }

    public async Task UnwatchItem(long itemId)
    {
        await Store.On(store => store.UnwatchItem(itemId));
        Changed?.Invoke();
    }

    public async Task UnwatchRealm(long realmId)
    {
        await Store.On(store => store.UnwatchRealm(realmId));
        Changed?.Invoke();
    }

    // -- what the page reads ----------------------------------------------------

    /// <summary>
    /// What is worth crafting, and which character should craft it. Region-
    /// wide commodities and each watched realm, because a reagent is a
    /// commodity under realm zero while a crafted piece of gear is a realm
    /// auction, and reading both is what lets one recipe be costed at all.
    /// </summary>
    public Task<Crafting> Making(IReadOnlyList<(long Id, string Name)> realms)
    {
        var names = Roster.Characters.ToDictionary(character => character.Key, character => character.DisplayName);
        return Store.On(store =>
        {
            var books = store.RecipesHeld().Match(held => held, _ => new RecipeBooks());
            if (books.Count == 0)
            {
                // Nobody has opened a profession window. The page says so
                // itself; there is no point querying a market to find out.
                return new Crafting();
            }
            var items = store.RecipeItems().Match(held => held, _ => []);
            var markets = new List<(long, string)> { (0, "Region-wide") }
                .Concat(realms)
                .Select(realm => new RealmMarket(realm.Item1, realm.Item2, store.CommoditySeries(realm.Item1, items).Match(series => series, _ => new Series())))
                .Where(market => market.Series.Count > 0)
                .ToList();
            var bank = store.WarbandBank().Match(held => held, _ => []);
            return Markets.WorthMaking(books, names, markets, bank);
        });
    }

    /// <summary>Which spare pets are worth selling, and where.</summary>
    public Task<List<Resale>> Resale(IReadOnlyList<(long Id, string Name)> realms) => Store.On(store =>
    {
        var held = store.PetsHeldCounts().Match(counts => counts, _ => []);
        if (held.Count == 0)
        {
            return [];
        }
        var (catalogue, _) = store.CollectiblesHeld(Kind.Pet).Match(pets => pets, _ => ([], []));
        var markets = realms
            .Select(realm => new RealmMarket(realm.Id, realm.Name, store.PriceSeries(realm.Id, Listing.CagedPet).Match(series => series, _ => new Series())))
            .ToList();
        return Markets.WorthSelling(catalogue, held, markets);
    });

    /// <summary>Everything the market page draws, read out of storage on each draw rather than held.</summary>
    public async Task<MarketView> MarketNow()
    {
        var (realms, quotes, watching) = await Store.On(store =>
        {
            var realms = store.WatchedRealmList().Match(list => list, _ => []);
            var watched = store.WatchedItems().Match(items => items, _ => []);
            var quotes = new List<Quote>();
            foreach (var (itemId, name) in watched)
            {
                // Region-wide commodities plus each watched realm. An item
                // that is a commodity has no realm history and vice versa,
                // so the empty ones are dropped rather than shown as blank.
                foreach (var (realm, realmName) in new List<(long, string)> { (0, "Region-wide") }.Concat(realms))
                {
                    var history = store.PriceHistory(realm, itemId).Match(rows => rows, _ => []);
                    if (history.Count > 0)
                    {
                        quotes.Add(new Quote { ItemId = itemId, Name = name, Realm = realm, RealmName = realmName, History = history.Select(row => new PriceReading(row.At, row.Price, row.Quantity)).ToList() });
                    }
                }
            }
            return (realms, quotes, watched.Select(item => item.ItemId).ToHashSet());
        });

        // Whichever realm was picked, falling back to the first watched one,
        // or to the region-wide commodity market when nothing is watched.
        var browsing = Browsing is { } chosen && (chosen == 0 || realms.Any(realm => realm.RealmId == chosen))
            ? chosen
            : realms.Count > 0 ? realms[0].RealmId : 0;
        var market = await Store.On(store => store.Snapshot(browsing).Match(rows => rows, _ => []));

        return new MarketView
        {
            Realms = realms,
            Quotes = quotes,
            Browsing = browsing,
            Market = market,
            Watching = watching,
            TokenPrice = TokenPrice,
            Offers = offers.ToList(),
            Resale = await Resale(realms),
            Crafting = await Making(realms),
        };
    }

    /// <summary>What the addon has reported for the Roster page's Warband group.</summary>
    public async Task<Warband> Warband()
    {
        var (bankItems, currencies) = await Store.On(store => (
            store.WarbandBank().Match(bank => bank.Count, _ => 0),
            store.CurrenciesHeld().Match(held => held.Values.Sum(amounts => amounts.Count), _ => 0)));
        // Counted across every character the addon has watched. A currency
        // that arrived three different ways on three characters is three
        // answers, which is what the page shows.
        int earned = 0, transferred = 0, unclear = 0;
        foreach (var held in Inputs.Provenance.Values)
        {
            foreach (var currency in held.Currency.Values)
            {
                switch (currency.Origin)
                {
                    case Armory.Provenance.Origin.Earned:
                        earned++;
                        break;
                    case Armory.Provenance.Origin.Transferred:
                        transferred++;
                        break;
                    case Armory.Provenance.Origin.Unclear:
                        unclear++;
                        break;
                    default:
                        break;
                }
            }
        }
        return new Warband(watch is not null, bankItems, currencies, earned, transferred, unclear, CollectedAt);
    }
}

/// <summary>The Warband group on the Roster page: what the addon has reported.</summary>
public sealed record Warband(bool Installed, int BankItems, int Currencies, int EarnedCurrencies, int TransferredCurrencies, int UnclearCurrencies, DateTimeOffset? WrittenAt);
