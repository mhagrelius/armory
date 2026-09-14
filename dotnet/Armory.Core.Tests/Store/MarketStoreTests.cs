using Armory.Blizzard;
using Armory.Store;
using Xunit;

namespace Armory.Tests.Store;

/// <summary>The price-history tests from <c>core/src/store.rs</c>, plus the snapshot and item readers that had none.</summary>
public sealed class MarketStoreTests
{
    private static readonly DateTimeOffset At = new(2026, 9, 14, 12, 0, 0, TimeSpan.Zero);

    private static Depth DepthOf(long itemId, long price, long quantity) => new()
    {
        ItemId = itemId,
        Variant = "",
        Cheapest = price,
        Quantity = quantity,
        Listings = 1,
        Tenth = price,
        Median = price,
    };

    [Fact(DisplayName = "the_shape_of_a_book_changing_is_worth_a_row_on_its_own")]
    public void The_shape_of_a_book_changing_is_worth_a_row_on_its_own()
    {
        using var store = Armory.Store.Store.InMemory();
        var before = DepthOf(1, 100, 500) with { Listings = 40 };
        store.RecordPrices(0, [before], At);

        var after = DepthOf(1, 100, 500) with { Listings = 1 };
        Assert.Equal(1, store.RecordPrices(0, [after], At.AddHours(1)).Value);
    }

    [Fact(DisplayName = "a_stable_market_writes_almost_nothing")]
    public void A_stable_market_writes_almost_nothing()
    {
        using var store = Armory.Store.Store.InMemory();
        var snapshot = new[] { DepthOf(197_794, 56_523, 400) };

        Assert.Equal(1, store.RecordPrices(0, snapshot, At).Value);
        // Same price, same quantity, an hour later: nothing to say.
        Assert.Equal(0, store.RecordPrices(0, snapshot, At.AddHours(1)).Value);
    }

    [Fact(DisplayName = "a_quantity_moving_is_worth_a_row_even_when_the_price_does_not")]
    public void A_quantity_moving_is_worth_a_row_even_when_the_price_does_not()
    {
        using var store = Armory.Store.Store.InMemory();
        store.RecordPrices(0, [DepthOf(1, 100, 50)], At);
        Assert.Equal(1, store.RecordPrices(0, [DepthOf(1, 100, 30)], At.AddHours(1)).Value);
    }

    [Fact(DisplayName = "a_variant_is_priced_as_a_different_thing")]
    public void A_variant_is_priced_as_a_different_thing()
    {
        using var store = Armory.Store.Store.InMemory();
        var gear = DepthOf(1, 90_000, 1) with { Variant = "b1532" };
        Assert.Equal(2, store.RecordPrices(0, [DepthOf(1, 100, 1), gear], At).Value);
    }

    [Fact(DisplayName = "a_history_comes_back_oldest_first")]
    public void A_history_comes_back_oldest_first()
    {
        using var store = Armory.Store.Store.InMemory();
        store.RecordPrices(0, [DepthOf(1, 300, 1)], At);
        store.RecordPrices(0, [DepthOf(1, 100, 1)], At.AddHours(1));
        store.RecordPrices(0, [DepthOf(1, 200, 1)], At.AddHours(2));

        Assert.Equal([300L, 100L, 200L], store.PriceHistory(0, 1).Value.Select(row => row.Price));
    }

    [Fact(DisplayName = "realms_and_commodities_keep_separate_histories")]
    public void Realms_and_commodities_keep_separate_histories()
    {
        using var store = Armory.Store.Store.InMemory();
        store.RecordPrices(0, [DepthOf(1, 100, 1)], At);
        store.RecordPrices(61, [DepthOf(1, 900, 1)], At);

        Assert.Equal(100, store.PriceHistory(0, 1).Value[0].Price);
        Assert.Equal(900, store.PriceHistory(61, 1).Value[0].Price);
    }

    [Fact(DisplayName = "purging_covers_price_history_because_the_terms_do")]
    public void Purging_covers_price_history_because_the_terms_do()
    {
        var clock = new FixedClock();
        using var store = Armory.Store.Store.InMemory(clock);
        var ancient = clock.Now - TimeSpan.FromDays(Armory.Store.Store.MaxTtlDays + 1);
        store.RecordPrices(0, [DepthOf(1, 100, 1)], ancient);
        store.RecordPrices(0, [DepthOf(2, 100, 1)], clock.Now);

        Assert.Equal(1, store.Purge().Value);
        Assert.Empty(store.PriceHistory(0, 1).Value);
        Assert.Single(store.PriceHistory(0, 2).Value);
    }

    [Fact(DisplayName = "a snapshot replaces a realm's rows and leaves the other realms alone")]
    public void A_snapshot_replaces_a_realms_rows_and_leaves_the_other_realms_alone()
    {
        using var store = Armory.Store.Store.InMemory();
        store.RecordSnapshot(61, [DepthOf(1, 100, 5), DepthOf(2, 200, 1)], At);
        store.RecordSnapshot(11, [DepthOf(3, 300, 1)], At);

        // Item 2 left the auction house; item 1 was repriced.
        store.RecordSnapshot(61, [DepthOf(1, 90, 4)], At.AddHours(1));

        var realm = store.SnapshotAll(61).Value;
        Assert.Equal([(1L, 90L, 4L)], realm);
        Assert.Single(store.SnapshotAll(11).Value);
    }

    [Fact(DisplayName = "browsing a realm shows the region-wide commodities too, named where a name is known")]
    public void Browsing_a_realm_shows_the_region_wide_commodities_too()
    {
        using var store = Armory.Store.Store.InMemory();
        store.RecordSnapshot(0, [DepthOf(2770, 21_100, 437_411)], At);
        store.RecordSnapshot(61, [DepthOf(6513, 1000, 4), DepthOf(6513, 5000, 1) with { Variant = "b1532" }], At);
        store.NameFoundItem(2770, "Copper Ore");

        var listed = store.Snapshot(61).Value.OrderBy(row => row.ItemId).ToList();
        Assert.Equal(2, listed.Count);
        Assert.Equal("Copper Ore", listed[0].Name);
        Assert.Equal("Copper Ore", listed[0].Title());
        Assert.Null(listed[1].Name);
        Assert.Equal("Item 6513", listed[1].Title());
        // Asking for the commodities alone asks for realm 0.
        Assert.Single(store.Snapshot(0).Value);
    }

    [Fact(DisplayName = "a name found by search never overwrites a binding the item call supplied")]
    public void A_name_found_by_search_never_overwrites_a_binding()
    {
        using var store = Armory.Store.Store.InMemory();
        store.NameFoundItem(1, "Helm");
        Assert.False(store.Items().Value[1].Sellable, "unknown is read as not sellable");

        store.NameItem(1, new Item { Name = "Helm of Might", Sellable = true, Quality = "EPIC" });
        store.NameFoundItem(1, "Helm of Might");
        var item = store.Items().Value[1];
        Assert.True(item.Sellable);
        Assert.Equal("EPIC", item.Quality);
        Assert.Equal("Helm of Might", store.ItemNames().Value[1]);

        store.NameItem(2, new Item { Name = "Mycobloom" });
        Assert.Null(store.Items().Value[2].Quality);
    }

    [Fact(DisplayName = "a commodity series is keyed by item and reads only the rows with no variant")]
    public void A_commodity_series_is_keyed_by_item_and_reads_only_the_rows_with_no_variant()
    {
        using var store = Armory.Store.Store.InMemory();
        store.RecordPrices(0, [DepthOf(210_797, 900, 500), DepthOf(210_797, 4000, 1) with { Variant = "b1" }, DepthOf(999, 1, 1)], At);
        store.RecordPrices(0, [DepthOf(210_797, 900, 480)], At.AddHours(1));

        var series = store.CommoditySeries(0, new HashSet<long> { 210_797 }).Value;
        var samples = Assert.Single(series).Value;
        Assert.Equal([500L, 480L], samples.Select(sample => sample.Quantity));
        Assert.Equal(At.UtcDateTime, samples[0].At);
        Assert.Empty(store.CommoditySeries(0, new HashSet<long>()).Value);
    }

    [Fact(DisplayName = "a price series answers per variant, which for a caged pet is per pet")]
    public void A_price_series_answers_per_variant()
    {
        using var store = Armory.Store.Store.InMemory();
        var common = DepthOf(Listing.CagedPet, 500, 5) with { Variant = "pet1:1" };
        var rare = DepthOf(Listing.CagedPet, 40_000, 5) with { Variant = "pet1:3" };
        store.RecordPrices(61, [common, rare], At);
        store.RecordPrices(61, [common with { Quantity = 4 }, rare with { Quantity = 4 }], At.AddHours(1));

        var series = store.PriceSeries(61, Listing.CagedPet).Value;
        Assert.Equal(2, series.Count);
        Assert.Equal([500L, 500L], series["pet1:1"].Select(sample => sample.Cheapest));
        Assert.Equal(4, series["pet1:3"][1].Quantity);
    }
}
