using System.Globalization;
using Armory.Blizzard;
using Armory.Market;
using Microsoft.EntityFrameworkCore;

namespace Armory.Store;

/// <summary>
/// The market half: the snapshot that is <i>now</i>, and the price history
/// that is opt-in. Two different questions, and only one is expensive.
/// </summary>
public sealed partial class Store
{
    /// <summary>
    /// Replace one realm's snapshot with this hour's book. Replaced rather
    /// than merged: an item that has left the auction house entirely has to
    /// disappear from the browser, and a merge leaves last hour's price
    /// sitting there looking current.
    /// </summary>
    public Result<Unit, StoreError> RecordSnapshot(long realm, IReadOnlyList<Depth> book, DateTimeOffset at) => Work(() =>
    {
        var stamp = Stamp(at);
        Reconcile(
            Context.Snapshots.Where(row => row.Realm == realm),
            book.Select(entry => new SnapshotRow
            {
                Realm = realm,
                ItemId = entry.ItemId,
                Variant = entry.Variant,
                Cheapest = entry.Cheapest,
                Quantity = entry.Quantity,
                Listings = entry.Listings,
                Tenth = entry.Tenth,
                Median = entry.Median,
                SeenAt = stamp,
            }).ToList(),
            row => (row.Realm, row.ItemId, row.Variant),
            (held, fresh) =>
            {
                held.Cheapest = fresh.Cheapest;
                held.Quantity = fresh.Quantity;
                held.Listings = fresh.Listings;
                held.Tenth = fresh.Tenth;
                held.Median = fresh.Median;
                held.SeenAt = fresh.SeenAt;
            });
    });

    /// <summary>
    /// What is for sale on one realm: its own listings and the region-wide
    /// commodities, because that is what the auction house in the game is.
    /// Realm 0 is where the commodities are recorded, so it is always in the
    /// query. Only rows with no variant: gear listed with bonus ids is a
    /// different kind of question.
    /// </summary>
    public Result<List<Listed>, StoreError> Snapshot(long realm) => Work(() =>
        (from snapshot in Context.Snapshots.AsNoTracking()
         where (snapshot.Realm == realm || snapshot.Realm == 0) && snapshot.Variant == ""
         join item in Context.Items.AsNoTracking() on snapshot.ItemId equals item.ItemId into named
         from item in named.DefaultIfEmpty()
         select new { snapshot.ItemId, snapshot.Cheapest, snapshot.Quantity, snapshot.Listings, snapshot.Tenth, snapshot.Median, Name = item == null ? null : item.Name })
        .AsEnumerable()
        .Select(row => new Listed
        {
            ItemId = row.ItemId,
            Cheapest = row.Cheapest,
            Quantity = row.Quantity,
            Listings = row.Listings,
            Tenth = row.Tenth,
            Median = row.Median,
            Name = row.Name,
            Sold = 0,
            SpanHours = 0,
        })
        .ToList());

    /// <summary>Every row of one realm's snapshot as (item, cheapest, quantity), variants included and folded by the caller.</summary>
    public Result<List<(long ItemId, long Cheapest, long Quantity)>, StoreError> SnapshotAll(long realm) => Work(() =>
        Context.Snapshots.AsNoTracking()
            .Where(row => row.Realm == realm)
            .Select(row => new { row.ItemId, row.Cheapest, row.Quantity })
            .AsEnumerable()
            .Select(row => (row.ItemId, row.Cheapest, row.Quantity))
            .ToList());

    /// <summary>
    /// Every commodity price series on one realm, for the items asked for.
    /// Keyed by item id rather than by <see cref="Listing.Series"/>, and only
    /// rows with no variant: a reagent is a commodity, and a commodity carries
    /// no bonus ids. Region-wide commodities are realm 0.
    /// </summary>
    public Result<Series, StoreError> CommoditySeries(long realm, IReadOnlySet<long> items) => Work(() =>
    {
        var series = new Series();
        if (items.Count == 0)
        {
            return series;
        }
        var rows = Context.Prices.AsNoTracking()
            .Where(row => row.Realm == realm && row.Variant == "")
            .OrderBy(row => row.ItemId).ThenBy(row => row.SeenAt)
            .ToList();
        foreach (var row in rows)
        {
            if (!items.Contains(row.ItemId))
            {
                continue;
            }
            var key = row.ItemId.ToString(CultureInfo.InvariantCulture);
            if (!series.TryGetValue(key, out var samples))
            {
                samples = [];
                series[key] = samples;
            }
            samples.Add(SampleOf(row));
        }
        return series;
    });

    /// <summary>
    /// Record what moved. Blizzard records no sale at all: quantity just
    /// disappears between snapshots, so the whole inference of "what sold"
    /// is these deltas. All six numbers count as movement: a floor that holds
    /// while forty listings become one has changed in the way that matters
    /// most. Returns how many rows were written.
    /// </summary>
    public Result<int, StoreError> RecordPrices(long realm, IReadOnlyList<Depth> book, DateTimeOffset at) => Work(() =>
    {
        var stamp = Stamp(at);
        var written = 0;
        foreach (var entry in book)
        {
            var previous = Context.Prices.AsNoTracking()
                .Where(row => row.Realm == realm && row.ItemId == entry.ItemId && row.Variant == entry.Variant)
                .OrderByDescending(row => row.SeenAt)
                .Select(row => new { row.UnitPrice, row.Quantity, row.Listings, row.Tenth, row.Median })
                .FirstOrDefault();
            var moved = previous is null
                || (previous.UnitPrice, previous.Quantity, previous.Listings, previous.Tenth, previous.Median)
                    != (entry.Cheapest, entry.Quantity, entry.Listings, entry.Tenth, entry.Median);
            if (!moved)
            {
                continue;
            }
            var held = Context.Prices.Find(realm, entry.ItemId, entry.Variant, stamp);
            if (held is null)
            {
                Context.Prices.Add(new PriceRow
                {
                    Realm = realm,
                    ItemId = entry.ItemId,
                    Variant = entry.Variant,
                    UnitPrice = entry.Cheapest,
                    Quantity = entry.Quantity,
                    SeenAt = stamp,
                    Listings = entry.Listings,
                    Tenth = entry.Tenth,
                    Median = entry.Median,
                });
            }
            else
            {
                held.UnitPrice = entry.Cheapest;
                held.Quantity = entry.Quantity;
                held.Listings = entry.Listings;
                held.Tenth = entry.Tenth;
                held.Median = entry.Median;
            }
            written++;
        }
        return written;
    });

    /// <summary>One item's price history on one realm, oldest first.</summary>
    public Result<List<(DateTimeOffset At, long Price, long Quantity)>, StoreError> PriceHistory(long realm, long itemId) => Work(() =>
        Context.Prices.AsNoTracking()
            .Where(row => row.Realm == realm && row.ItemId == itemId)
            .OrderBy(row => row.SeenAt)
            .Select(row => new { row.SeenAt, row.UnitPrice, row.Quantity })
            .AsEnumerable()
            .Select(row => (At: ParseStamp(row.SeenAt), row.UnitPrice, row.Quantity))
            .Where(row => row.At is not null)
            .Select(row => (row.At!.Value, row.UnitPrice, row.Quantity))
            .ToList());

    /// <summary>
    /// Every series recorded for one item on one realm, oldest first. Per
    /// variant, which for item 82800 is per pet: a caged pet has no item id
    /// of its own, so reading the item whole would interleave fifteen hundred
    /// different pets into one line.
    /// </summary>
    public Result<Series, StoreError> PriceSeries(long realm, long itemId) => Work(() =>
    {
        var series = new Series();
        var rows = Context.Prices.AsNoTracking()
            .Where(row => row.Realm == realm && row.ItemId == itemId)
            .OrderBy(row => row.Variant).ThenBy(row => row.SeenAt)
            .ToList();
        foreach (var row in rows)
        {
            if (!series.TryGetValue(row.Variant, out var samples))
            {
                samples = [];
                series[row.Variant] = samples;
            }
            samples.Add(SampleOf(row));
        }
        return series;
    });

    /// <summary>A price row as a sample. A stamp that will not parse is read as now, as the Rust store does.</summary>
    private Sample SampleOf(PriceRow row) => new(
        (ParseStamp(row.SeenAt) ?? Clock.GetUtcNow()).UtcDateTime,
        row.UnitPrice,
        row.Quantity,
        row.Listings,
        row.Tenth,
        row.Median);
}
