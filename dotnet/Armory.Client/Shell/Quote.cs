using System.Globalization;
using Armory.Market;

namespace Armory.Client.Shell;

/// <summary>One reading of a watched item's price: when, the cheapest unit price, and how many units were listed.</summary>
public readonly record struct PriceReading(DateTimeOffset At, long Price, long Quantity);

/// <summary>What is known about one watched item on one realm. The port of the GTK market page's <c>Quote</c>.</summary>
public sealed record Quote
{
    /// <summary>The 30-day ceiling, in days. A term of the API licence, not a cache policy.</summary>
    public const int Term = 30;

    public long ItemId { get; init; }

    public string Name { get; init; } = "";

    /// <summary>Realm id, or zero for region-wide commodities.</summary>
    public long Realm { get; init; }

    public string RealmName { get; init; } = "";

    public List<PriceReading> History { get; init; } = [];

    public PriceReading? Latest => History.Count > 0 ? History[^1] : null;

    /// <summary>
    /// How the price has moved across what we hold. Null when there is only
    /// one observation: a change needs two, and showing 0% for a single
    /// reading would claim a stable market we have not watched long enough
    /// to have seen.
    /// </summary>
    public double? Change()
    {
        if (History.Count < 2 || History[0].Price == 0)
        {
            return null;
        }
        double first = History[0].Price;
        double last = History[^1].Price;
        return (last - first) / first;
    }

    /// <summary>The span actually observed, in whole days, never the thirty the store may keep.</summary>
    public long Days() => History.Count == 0 ? 1 : Math.Max((long)(History[^1].At - History[0].At).TotalDays, 1);

    /// <summary>The same span in hours, which is what turns a count into a rate.</summary>
    public long Hours() => History.Count == 0 ? 1 : Math.Max((long)(History[^1].At - History[0].At).TotalHours, 1);

    public DateTimeOffset? Since => History.Count > 0 ? History[0].At : null;

    /// <summary>
    /// Units that left the listings across the window. Only the falls, which
    /// is the same inference <c>RecordPrices</c> makes and the only one
    /// available: a quantity going up is somebody listing more and says
    /// nothing about demand.
    /// </summary>
    public long Moved()
    {
        long moved = 0;
        for (var i = 1; i < History.Count; i++)
        {
            var fell = History[i - 1].Quantity - History[i].Quantity;
            if (fell > 0)
            {
                moved += fell;
            }
        }
        return moved;
    }

    public List<double> Prices() => History.Select(reading => (double)reading.Price).ToList();
}

/// <summary>The market page's sentences and figures, written once so the browser, the watch list and the crafting cards agree.</summary>
public static class MarketProse
{
    /// <summary>Copper as gold, the way the game writes it.</summary>
    public static string Gold(long copper)
    {
        var gold = copper / 10_000;
        var silver = (copper % 10_000) / 100;
        return gold > 0
            ? string.Create(CultureInfo.InvariantCulture, $"{Thousands(gold)}g {silver}s")
            : string.Create(CultureInfo.InvariantCulture, $"{silver}s {copper % 100}c");
    }

    public static string Thousands(long n) => n.ToString("N0", CultureInfo.InvariantCulture);

    /// <summary>
    /// How fast something has been moving, said as a rate. Below one a day
    /// the count is given instead: "0.3 a day" is a precision the evidence
    /// does not support, and "4 sold in 9 days" is what was actually seen.
    /// </summary>
    public static string Moving(long sold, long spanHours)
    {
        if (sold == 0)
        {
            return "nothing seen to move";
        }
        var perDay = Markets.PerDay(sold, spanHours);
        if (perDay >= 1.0)
        {
            return $"{Thousands((long)Math.Round(perDay))} a day moving";
        }
        var days = Math.Max((long)Math.Round(Math.Max(spanHours, 1) / 24.0), 1);
        return $"{Thousands(sold)} sold in {Chronicle.Prose.Plural(days, "day", "days")}";
    }

    /// <summary>What the browser's last column says. "not watched" is the honest state and the whole argument for watching.</summary>
    public static string Selling(Listed listed)
    {
        if (listed.SpanHours == 0)
        {
            return "not watched";
        }
        var perDay = Markets.PerDay(listed.Sold, listed.SpanHours);
        if (perDay >= 1.0)
        {
            return $"{Thousands((long)Math.Round(perDay))}/day";
        }
        if (listed.Sold > 0)
        {
            return string.Create(CultureInfo.InvariantCulture, $"{Thousands(listed.Sold)} in {Math.Max((long)Math.Round(listed.SpanHours / 24.0), 1)}d");
        }
        return "none moved";
    }

    /// <summary>Where a recipe landed in the ranking, in words. A sentence, not a readout.</summary>
    public static string Ordinal(int index)
    {
        string[] words = ["FIRST", "SECOND", "THIRD", "FOURTH", "FIFTH", "SIXTH", "SEVENTH", "EIGHTH", "NINTH", "TENTH", "ELEVENTH", "TWELFTH"];
        return index < words.Length ? words[index] : string.Create(CultureInfo.InvariantCulture, $"{index + 1}TH");
    }

    /// <summary>The market in the one order there is: market size descending, then name. Written once and read by the table and the name backfill alike.</summary>
    public static List<Listed> Ordered(IReadOnlyList<Listed> market, string needle) =>
        Markets.Browse(market, needle)
            .OrderByDescending(listed => listed.Median * listed.Quantity)
            .ThenBy(listed => listed.Title(), StringComparer.OrdinalIgnoreCase)
            .ToList();

    /// <summary>Which items on the page still have no name, in the page's order, so the budget is spent on what somebody is looking at.</summary>
    public static List<long> WantsNames(IReadOnlyList<Listed> market, string needle, int budget) =>
        Ordered(market, needle).Where(listed => listed.Name is null).Take(budget).Select(listed => listed.ItemId).ToList();
}
