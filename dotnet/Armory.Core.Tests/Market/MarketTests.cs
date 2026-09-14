using Armory.Blizzard;
using Armory.Collections;
using Armory.Market;
using Armory.Roster;
using Xunit;

namespace Armory.Tests.Market;

/// <summary>Ported from <c>core/src/market.rs</c>.</summary>
public sealed class MarketTests
{
    private static Collectible Pet(long id, string name) => new()
    {
        Kind = Kind.Pet,
        Id = id,
        Name = name,
        Source = Source.Drop,
        LinkId = id * 10,
    };

    private static Listing Caged(long species, long price, long quantity) => new()
    {
        // Every caged pet in the game is this item.
        ItemId = 82800,
        UnitPrice = price,
        Quantity = quantity,
        Variant = "",
        PetSpecies = species,
        PetQuality = 3,
    };

    private static Collectible Spare(long id, string name) => Pet(id, name) with { Tradeable = true };

    /// <summary>(price, quantity) pairs as a series, an hour apart, because that is what a snapshot cycle is.</summary>
    private static List<Sample> OverTime(params (long Price, long Quantity)[] samples)
    {
        var start = new DateTime(2026, 8, 1, 0, 0, 0, DateTimeKind.Utc);
        return samples.Select((sample, hour) => new Sample(start.AddHours(hour), sample.Price, sample.Quantity, 1, sample.Price, sample.Price)).ToList();
    }

    /// <summary>One realm's series for item 82800, keyed the way the store holds them.</summary>
    private static RealmMarket Market(long realm, string name, params (long Species, long Quality, (long, long)[] Samples)[] series)
    {
        var built = new Series();
        foreach (var (species, quality, samples) in series)
        {
            built[$"pet{species}:{quality}"] = OverTime(samples);
        }
        return new RealmMarket(realm, name, built);
    }

    /// <summary>A market of plain commodities, keyed by item id the way one is.</summary>
    private static RealmMarket Goods(long realm, string name, params (long Item, (long, long)[] Samples)[] items)
    {
        var series = new Series();
        foreach (var (item, samples) in items)
        {
            series[item.ToString()] = OverTime(samples);
        }
        return new RealmMarket(realm, name, series);
    }

    private static Recipe Flask() => new()
    {
        Id = 371_637,
        Name = "Flask of Alchemical Chaos",
        Output = 191_318,
        Makes = 1,
        Reagents =
        [
            new Reagent { Quantity = 3, Tiers = [210_796, 210_797, 210_798] },
            new Reagent { Quantity = 1, Tiers = [212_263] },
        ],
    };

    private static (RecipeBooks Books, Dictionary<CharacterKey, string> Names) Books(params Recipe[] recipes)
    {
        var key = new CharacterKey("emerald-dream", "Somechar");
        var books = new RecipeBooks { [key] = recipes.ToList() };
        return (books, new Dictionary<CharacterKey, string> { [key] = "Somechar" });
    }

    private static Listed ListedAs(long itemId, string? name, long cheapest, long quantity) => new()
    {
        ItemId = itemId,
        Name = name,
        Cheapest = cheapest,
        Quantity = quantity,
        Listings = 1,
        Tenth = cheapest,
        Median = cheapest,
        Sold = 0,
        SpanHours = 0,
    };

    private static readonly HashSet<long> Nobody = [];

    [Fact(DisplayName = "only_what_is_missing_is_offered")]
    public void Only_what_is_missing_is_offered()
    {
        var catalogue = new[] { Pet(1, "Sprite Darter"), Pet(2, "Nether Faerie Dragon") };
        var offers = Markets.OnSale(catalogue, new HashSet<long> { 1 }, [Caged(1, 500, 1), Caged(2, 900, 1)], 61);
        Assert.True(offers.Count == 1, "the collected one is not an offer");
        Assert.Equal(2, offers[0].CollectibleId);
        Assert.Equal(61, offers[0].Realm);
    }

    [Fact(DisplayName = "a_caged_pet_is_matched_on_its_species_and_never_on_its_item")]
    public void A_caged_pet_is_matched_on_its_species_and_never_on_its_item()
    {
        var catalogue = new[] { Pet(1, "Sprite Darter"), Pet(2, "Nether Faerie Dragon") };
        var offers = Markets.OnSale(catalogue, Nobody, [Caged(2, 900, 1)], 0);
        Assert.Single(offers);
        Assert.Equal("Nether Faerie Dragon", offers[0].Name);
    }

    [Fact(DisplayName = "several_listings_of_one_thing_are_one_offer_at_the_lowest_price")]
    public void Several_listings_of_one_thing_are_one_offer_at_the_lowest_price()
    {
        var offers = Markets.OnSale([Pet(1, "Sprite Darter")], Nobody, [Caged(1, 900, 2), Caged(1, 400, 1), Caged(1, 1200, 5)], 0);
        Assert.Single(offers);
        Assert.Equal(400, offers[0].UnitPrice);
        Assert.Equal(8, offers[0].Quantity);
    }

    [Fact(DisplayName = "offers_come_back_cheapest_first")]
    public void Offers_come_back_cheapest_first()
    {
        var catalogue = new[] { Pet(1, "A"), Pet(2, "B"), Pet(3, "C") };
        var offers = Markets.OnSale(catalogue, Nobody, [Caged(1, 900, 1), Caged(2, 100, 1), Caged(3, 500, 1)], 0);
        Assert.Equal([100L, 500L, 900L], offers.Select(offer => offer.UnitPrice));
    }

    [Fact(DisplayName = "a_toy_joins_on_the_item_it_is")]
    public void A_toy_joins_on_the_item_it_is()
    {
        var toy = Pet(500, "Kang's Bindstone") with { Kind = Kind.Toy, LinkId = 86571 };
        var listing = new Listing { ItemId = 86571, UnitPrice = 250_000, Quantity = 1 };
        var offers = Markets.OnSale([toy], Nobody, [listing], 61);
        Assert.Single(offers);
        Assert.Equal(Kind.Toy, offers[0].Kind);
        Assert.Equal(250_000, offers[0].UnitPrice);
    }

    [Fact(DisplayName = "a_snapshot_with_nothing_of_interest_yields_nothing")]
    public void A_snapshot_with_nothing_of_interest_yields_nothing()
    {
        var noise = new Listing { ItemId = 197794, UnitPrice = 50, Quantity = 900 };
        Assert.Empty(Markets.OnSale([Pet(1, "Sprite Darter")], Nobody, [noise], 0));
    }

    [Fact(DisplayName = "browsing_matches_a_name_where_there_is_one_and_an_id_where_there_is_not")]
    public void Browsing_matches_a_name_where_there_is_one_and_an_id_where_there_is_not()
    {
        var market = new[]
        {
            ListedAs(1, "Mycobloom", 37_400, 4_120),
            ListedAs(2, "Crystalline Powder", 21_000, 12_400),
            ListedAs(219_873, null, 500, 4),
        };
        Assert.Single(Markets.Browse(market, "bloom"));
        var byId = Markets.Browse(market, "219873");
        Assert.Single(byId);
        Assert.Equal(219_873, byId[0].ItemId);
        Assert.Equal(3, Markets.Browse(market, "").Count);
    }

    [Fact(DisplayName = "the_items_worth_naming_first_are_the_ones_being_traded")]
    public void The_items_worth_naming_first_are_the_ones_being_traded()
    {
        var ore = ListedAs(2770, null, 21_100, 437_411) with { Listings = 153 };
        var shirt = ListedAs(10_042, null, 500, 1) with { Listings = 1 };
        var named = ListedAs(1, "Mycobloom", 37_400, 4_120);

        var wanted = Markets.WorthNaming([shirt, named, ore], 2);
        Assert.True(wanted.SequenceEqual([2770L, 10_042L]), "busiest market first");
        Assert.DoesNotContain(1L, wanted);
    }

    [Fact(DisplayName = "a_craft_is_costed_at_the_cheapest_reagent_quality_that_has_a_price")]
    public void A_craft_is_costed_at_the_cheapest_reagent_quality_that_has_a_price()
    {
        var (book, names) = Books(Flask());
        var markets = new[]
        {
            Goods(61, "Emerald Dream",
                (191_318, [(120_000, 40), (120_000, 22)]),
                // The three-star tier is dearest and the one-star is absent, so
                // the two-star is what a craft is costed at: 3 × 900.
                (210_797, [(900, 500), (900, 480)]),
                (210_798, [(4_000, 90), (4_000, 88)]),
                (212_263, [(1_500, 200), (1_500, 190)])),
        };

        var crafting = Markets.WorthMaking(book, names, markets, new Dictionary<long, long>());
        Assert.Equal(default, crafting.Unmeasured);
        var flip = Assert.Single(crafting.Worth);
        Assert.Equal(3 * 900 + 1_500, flip.Cost);
        Assert.Equal(120_000, flip.Each);
        Assert.Equal(114_000, flip.Revenue);
        Assert.Equal(114_000 - 4_200, flip.Margin);
        Assert.Equal("Somechar", flip.ByName);
        // Only the falls count as sales, in both directions.
        Assert.Equal(18, flip.Sold);
    }

    [Fact(DisplayName = "a_recipe_with_one_unpriced_reagent_is_unmeasured_and_not_cheap")]
    public void A_recipe_with_one_unpriced_reagent_is_unmeasured_and_not_cheap()
    {
        var (book, names) = Books(Flask());
        var markets = new[]
        {
            Goods(61, "Emerald Dream",
                (191_318, [(120_000, 40), (120_000, 22)]),
                (210_797, [(900, 500), (900, 480)])),
            // 212263 has never been seen listed.
        };

        var crafting = Markets.WorthMaking(book, names, markets, new Dictionary<long, long>());
        Assert.Empty(crafting.Worth);
        Assert.Equal(1, crafting.Unmeasured.MissingReagent);
        Assert.Equal(0, crafting.Unmeasured.MissingOutput);
    }

    [Fact(DisplayName = "a_fat_margin_on_a_dead_market_loses_to_a_thin_one_that_moves")]
    public void A_fat_margin_on_a_dead_market_loses_to_a_thin_one_that_moves()
    {
        var dead = new Recipe { Id = 1, Name = "Unsellable Draught", Output = 100, Makes = 1, Reagents = [new Reagent { Quantity = 1, Tiers = [900] }] };
        var brisk = new Recipe { Id = 2, Name = "Ordinary Potion", Output = 200, Makes = 1, Reagents = [new Reagent { Quantity = 1, Tiers = [901] }] };
        var (book, names) = Books(dead, brisk);
        var markets = new[]
        {
            Goods(61, "Emerald Dream",
                // Huge margin, one unit ever moved.
                (100, [(500_000, 3), (500_000, 2)]),
                (900, [(100, 900), (100, 900)]),
                // Small margin, four hundred moved.
                (200, [(20_000, 900), (20_000, 500)]),
                (901, [(100, 900), (100, 900)])),
        };

        var making = Markets.WorthMaking(book, names, markets, new Dictionary<long, long>()).Worth;
        Assert.Equal(2, making.Count);
        Assert.Equal("Ordinary Potion", making[0].Name);
        Assert.True(making[1].Margin > making[0].Margin, "the loser here has the better paper margin, which is the point");
    }

    [Fact(DisplayName = "warband_stock_is_shown_and_never_subtracted")]
    public void Warband_stock_is_shown_and_never_subtracted()
    {
        var (book, names) = Books(Flask());
        var markets = new[]
        {
            Goods(61, "Emerald Dream",
                (191_318, [(120_000, 40), (120_000, 22)]),
                (210_797, [(900, 500), (900, 480)]),
                (212_263, [(1_500, 200), (1_500, 190)])),
        };
        var bank = new Dictionary<long, long> { [210_797] = 600 };

        var making = Markets.WorthMaking(book, names, markets, bank).Worth;
        Assert.Equal([(210_797L, 600L)], making[0].Held);
        // Unchanged by the bank holding every reagent the craft needs.
        Assert.Equal(3 * 900 + 1_500, making[0].Cost);
    }

    [Fact(DisplayName = "a_craft_that_loses_money_is_not_something_worth_making")]
    public void A_craft_that_loses_money_is_not_something_worth_making()
    {
        var (book, names) = Books(Flask());
        var markets = new[]
        {
            Goods(61, "Emerald Dream",
                (191_318, [(1_000, 40), (1_000, 22)]),
                (210_797, [(900, 500), (900, 480)]),
                (212_263, [(1_500, 200), (1_500, 190)])),
        };

        var crafting = Markets.WorthMaking(book, names, markets, new Dictionary<long, long>());
        Assert.Empty(crafting.Worth);
        // Measured, and the answer was no. Not the same as unmeasured.
        Assert.Equal(default, crafting.Unmeasured);
    }

    [Fact(DisplayName = "a_pet_you_own_once_is_not_a_thing_you_can_sell")]
    public void A_pet_you_own_once_is_not_a_thing_you_can_sell()
    {
        var catalogue = new[] { Spare(1, "Sprite Darter") };
        var market = Market(61, "Emerald Dream", (1, 3, [(5000, 4), (5000, 2)]));

        var onlyOne = Markets.WorthSelling(catalogue, new Dictionary<long, long> { [1] = 1 }, [market]);
        Assert.True(onlyOne.Count == 0, "one copy is not a spare");

        var two = Markets.WorthSelling(catalogue, new Dictionary<long, long> { [1] = 3 }, [market]);
        Assert.Single(two);
        Assert.True(two[0].Spare == 2, "the one being kept is not for sale");
    }

    [Fact(DisplayName = "a_pet_that_cannot_be_caged_is_never_offered")]
    public void A_pet_that_cannot_be_caged_is_never_offered()
    {
        var bound = Pet(1, "Sprite Darter") with { Tradeable = false };
        var market = Market(61, "Emerald Dream", (1, 3, [(5000, 4), (5000, 2)]));
        Assert.Empty(Markets.WorthSelling([bound], new Dictionary<long, long> { [1] = 4 }, [market]));

        // And silence is not a no.
        var unknown = Pet(1, "Sprite Darter");
        Assert.Null(unknown.Tradeable);
        Assert.Empty(Markets.WorthSelling([unknown], new Dictionary<long, long> { [1] = 4 }, [market]));
    }

    [Fact(DisplayName = "the_realm_that_makes_sense_is_the_one_paying_most")]
    public void The_realm_that_makes_sense_is_the_one_paying_most()
    {
        var catalogue = new[] { Spare(1, "Sprite Darter") };
        var held = new Dictionary<long, long> { [1] = 2 };
        var markets = new[]
        {
            Market(61, "Emerald Dream", (1, 3, [(1000, 9), (1000, 8)])),
            Market(11, "Tichondrius", (1, 3, [(9000, 9), (9000, 8)])),
        };

        var offers = Markets.WorthSelling(catalogue, held, markets);
        Assert.True(offers.Count == 1, "one pet is one recommendation, not two");
        Assert.Equal("Tichondrius", offers[0].RealmName);
        Assert.Equal(9000, offers[0].Floor);
    }

    [Fact(DisplayName = "the_quoted_price_is_the_floor_across_qualities_and_the_spread_is_shown")]
    public void The_quoted_price_is_the_floor_across_qualities_and_the_spread_is_shown()
    {
        var catalogue = new[] { Spare(1, "Sprite Darter") };
        var markets = new[]
        {
            Market(61, "Emerald Dream",
                (1, 1, [(500, 5), (500, 4)]),
                (1, 3, [(40_000, 5), (40_000, 4)])),
        };

        var offers = Markets.WorthSelling(catalogue, new Dictionary<long, long> { [1] = 2 }, markets);
        Assert.True(offers[0].Floor == 500, "true whatever the spare turns out to be");
        Assert.Equal(40_000, offers[0].Ceiling);
    }

    [Fact(DisplayName = "only_quantities_that_fell_count_as_sales")]
    public void Only_quantities_that_fell_count_as_sales()
    {
        Assert.Equal(6, Markets.Sold(OverTime((100, 10), (100, 6), (100, 9), (100, 7))));
        Assert.Equal(0, Markets.Sold(OverTime((100, 1), (100, 5))));
        Assert.True(Markets.Sold(OverTime((100, 4))) == 0, "one sample is no evidence at all");
    }

    [Fact(DisplayName = "a_count_only_becomes_a_rate_over_the_span_actually_watched")]
    public void A_count_only_becomes_a_rate_over_the_span_actually_watched()
    {
        var fourHours = OverTime((100, 10), (100, 6), (100, 9), (100, 7));
        Assert.Equal(3, Markets.SpanHours(fourHours));
        Assert.True(Math.Abs(Markets.PerDay(Markets.Sold(fourHours), Markets.SpanHours(fourHours)) - 48.0) < 0.01);

        Assert.Equal(1, Markets.SpanHours(OverTime((100, 4))));
        Assert.Equal(0.0, Markets.PerDay(0, 1));
    }

    [Fact(DisplayName = "the_going_rate_is_the_median_and_not_the_last_thing_seen")]
    public void The_going_rate_is_the_median_and_not_the_last_thing_seen()
    {
        Assert.Equal(1050, Markets.Typical(OverTime((1000, 1), (1100, 1), (1050, 1), (5, 1))));
    }

    [Fact(DisplayName = "a_pet_nobody_has_listed_is_not_a_recommendation")]
    public void A_pet_nobody_has_listed_is_not_a_recommendation()
    {
        var catalogue = new[] { Spare(1, "Sprite Darter") };
        var markets = new[] { Market(61, "Emerald Dream", (999, 3, [(5000, 2)])) };
        Assert.Empty(Markets.WorthSelling(catalogue, new Dictionary<long, long> { [1] = 5 }, markets));
    }

    [Fact(DisplayName = "an_empty_collection_asks_nothing_of_the_snapshot")]
    public void An_empty_collection_asks_nothing_of_the_snapshot()
    {
        Assert.Empty(Markets.OnSale([Pet(1, "Sprite Darter")], new HashSet<long> { 1 }, [Caged(1, 100, 1)], 0));
    }
}
