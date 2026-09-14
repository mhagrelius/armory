using Armory.Blizzard;
using Microsoft.EntityFrameworkCore;

namespace Armory.Store;

/// <summary>
/// What items are called. The auction house never says: a listing is an id,
/// and names arrive one call at a time.
/// </summary>
public sealed partial class Store
{
    /// <summary>Item names already known.</summary>
    public Result<Dictionary<long, string>, StoreError> ItemNames() => Work(() =>
        Context.Items.AsNoTracking().ToDictionary(row => row.ItemId, row => row.Name));

    /// <summary>Remember what one item is.</summary>
    public Result<Unit, StoreError> NameItem(long itemId, Item item) => Work(() =>
    {
        var held = Context.Items.Find(itemId);
        if (held is null)
        {
            Insert(itemId, item.Name, item.Sellable ? 1 : 0, item.Quality ?? "");
        }
        else
        {
            held.Name = item.Name;
            held.Sellable = item.Sellable ? 1 : 0;
            held.Quality = item.Quality ?? "";
        }
    });

    /// <summary>
    /// Record an item's name, and nothing else about it. The name comes from
    /// the item search, which carries no binding, so <c>sellable</c> is left
    /// at unknown on insert and left alone on conflict: a row that already
    /// carries a real binding must not have it overwritten by a search that
    /// never saw one. Unknown is the honest state and also the safe one.
    /// </summary>
    public Result<Unit, StoreError> NameFoundItem(long itemId, string name) => Work(() =>
    {
        var held = Context.Items.Find(itemId);
        if (held is null)
        {
            Insert(itemId, name, 0, "");
        }
        else
        {
            held.Name = name;
        }
    });

    /// <summary>
    /// A new item row. Zero is the CLR default for <c>sellable</c>, and a
    /// column with a database default is left out of the insert while it
    /// still holds its CLR default, so the database's 1 would win over an
    /// honest 0. Insert, then set the value as an update.
    /// </summary>
    private void Insert(long itemId, string name, long sellable, string quality)
    {
        var row = new ItemRow { ItemId = itemId, Name = name, Quality = quality };
        Context.Items.Add(row);
        if (sellable == 0)
        {
            Context.SaveChanges();
        }
        row.Sellable = sellable;
    }

    /// <summary>Everything known about the items named so far.</summary>
    public Result<Dictionary<long, Item>, StoreError> Items() => Work(() =>
        Context.Items.AsNoTracking().AsEnumerable().ToDictionary(
            row => row.ItemId,
            row => new Item { Name = row.Name, Sellable = row.Sellable != 0, Quality = row.Quality.Length == 0 ? null : row.Quality }));
}
