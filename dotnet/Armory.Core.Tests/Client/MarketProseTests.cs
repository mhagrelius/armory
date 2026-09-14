using Armory.Client.Shell;
using Armory.Market;
using Xunit;

namespace Armory.Tests.Client;

/// <summary>Ported from the tests in <c>src/ui/market_page.rs</c>.</summary>
public sealed class MarketProseTests
{
    private static Quote QuoteOf(long[] prices, long[]? quantities = null)
    {
        var at = DateTimeOffset.UtcNow;
        var count = Math.Max(prices.Length, quantities?.Length ?? 0);
        return new Quote
        {
            ItemId = 1,
            Name = "Mycobloom",
            RealmName = "Region-wide",
            History = Enumerable.Range(0, count)
                .Select(index => new PriceReading(at.AddHours(index), index < prices.Length ? prices[index] : 100, quantities is null ? 10 : quantities[index]))
                .ToList(),
        };
    }

    [Fact(DisplayName = "copper_reads_as_gold_and_silver")]
    public void Copper_reads_as_gold_and_silver()
    {
        Assert.Equal("123g 45s", MarketProse.Gold(1_234_500));
        Assert.Equal("45s 0c", MarketProse.Gold(4_500));
    }

    [Fact(DisplayName = "one_observation_is_not_a_trend")]
    public void One_observation_is_not_a_trend()
    {
        // Showing 0% would claim a stable market we have not watched long enough to have seen.
        Assert.Null(QuoteOf([100]).Change());
        Assert.Null(QuoteOf([]).Change());
    }

    [Fact(DisplayName = "a_change_is_measured_across_what_is_held")]
    public void A_change_is_measured_across_what_is_held()
    {
        Assert.Equal(0.5, QuoteOf([100, 150]).Change()!.Value, 9);
        Assert.Equal(-0.5, QuoteOf([200, 100]).Change()!.Value, 9);
    }

    [Fact(DisplayName = "only_stock_that_disappeared_counts_as_having_moved")]
    public void Only_stock_that_disappeared_counts_as_having_moved()
    {
        // A quantity going up is somebody listing more and says nothing about demand.
        Assert.Equal(30, QuoteOf([100, 100, 100, 100], [50, 30, 60, 50]).Moved());
        Assert.Equal(0, QuoteOf([100, 100], [10, 40]).Moved());
    }

    [Fact(DisplayName = "movement is a rate above a day and a count below it")]
    public void Movement_is_a_rate_above_a_day_and_a_count_below_it()
    {
        Assert.Equal("nothing seen to move", MarketProse.Moving(0, 48));
        Assert.Equal("12 a day moving", MarketProse.Moving(24, 48));
        Assert.Equal("4 sold in 9 days", MarketProse.Moving(4, 9 * 24));
        Assert.Equal("not watched", MarketProse.Selling(new Listed { ItemId = 1 }));
        Assert.Equal("none moved", MarketProse.Selling(new Listed { ItemId = 1, SpanHours = 48 }));
        Assert.Equal("2/day", MarketProse.Selling(new Listed { ItemId = 1, Sold = 4, SpanHours = 48 }));
    }

    [Fact(DisplayName = "the browser is ordered by market size and names are wanted in that order")]
    public void The_browser_is_ordered_by_market_size()
    {
        List<Listed> market =
        [
            new() { ItemId = 1, Name = "Small", Median = 10, Quantity = 10 },
            new() { ItemId = 2, Median = 100, Quantity = 100 },
            new() { ItemId = 3, Name = "Also small", Median = 10, Quantity = 10 },
        ];
        Assert.Equal([2L, 3L, 1L], MarketProse.Ordered(market, "").Select(listed => listed.ItemId));
        Assert.Equal([2L], MarketProse.WantsNames(market, "", 5));
        Assert.Equal("FOURTH", MarketProse.Ordinal(3));
        Assert.Equal("13TH", MarketProse.Ordinal(12));
    }
}
