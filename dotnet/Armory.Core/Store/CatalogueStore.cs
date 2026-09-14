using System.Text.Json;
using Armory.Blizzard;
using Armory.Collections;
using Microsoft.EntityFrameworkCore;

namespace Armory.Store;

/// <summary>The achievement catalogue, and the collections catalogue with what of it is owned.</summary>
public sealed partial class Store
{
    public Result<Unit, StoreError> SaveAchievements(IEnumerable<Achievement> achievements) => Work(() =>
    {
        foreach (var achievement in achievements)
        {
            var held = Context.Achievements.Find(achievement.Id);
            if (held is null)
            {
                Context.Achievements.Add(new AchievementRow
                {
                    Id = achievement.Id,
                    Name = achievement.Name,
                    Category = achievement.Category,
                    Points = achievement.Points,
                    Description = achievement.Description,
                    Unrepeatable = achievement.IsUnrepeatable ? 1 : 0,
                });
            }
            else
            {
                held.Name = achievement.Name;
                held.Category = achievement.Category;
                held.Points = achievement.Points;
                held.Description = achievement.Description;
                held.Unrepeatable = achievement.IsUnrepeatable ? 1 : 0;
            }
        }
    });

    public Result<Dictionary<long, Achievement>, StoreError> AchievementsHeld() => Work(() =>
        Context.Achievements.AsNoTracking().AsEnumerable().ToDictionary(row => row.Id, row => new Achievement
        {
            Id = row.Id,
            Name = row.Name,
            Category = row.Category,
            Points = row.Points,
            Description = row.Description,
            IsUnrepeatable = row.Unrepeatable != 0,
        }));

    /// <summary>
    /// Add or update catalogue entries, leaving ownership alone. Merged rather
    /// than replaced: the journal knows the sentence, the artwork and the
    /// faction lock; the web API knows a name. Overwriting one record with
    /// the other loses whichever half arrived first, and the half most often
    /// lost is the artwork.
    /// </summary>
    public Result<Unit, StoreError> SaveCollectibles(IEnumerable<Collectible> entries) => Work(() =>
    {
        foreach (var entry in entries)
        {
            var kind = entry.Kind.ToString();
            var held = Context.Collectibles.Find(kind, entry.Id);
            var merged = entry;
            if (held is not null)
            {
                try
                {
                    if (JsonSerializer.Deserialize<Collectible>(held.Json, Collectible.Json) is { } mine)
                    {
                        merged = entry.Merge(mine);
                    }
                }
                catch (JsonException)
                {
                    // What is held is unreadable; the arriving half stands alone.
                }
            }
            var json = JsonSerializer.Serialize(merged, Collectible.Json);
            if (held is null)
            {
                Context.Collectibles.Add(new CollectibleRow { Kind = kind, Id = entry.Id, Json = json });
            }
            else
            {
                held.Json = json;
            }
        }
    });

    /// <summary>
    /// Replace what is owned for one kind. What is owned goes in first, then
    /// what is no longer owned comes out, rather than clearing the column and
    /// setting it again: the old order wrote every collectible twice on every
    /// sync, which is a thousand rows to send to every other machine to say
    /// nothing changed. An owned id the catalogue has not reached yet is still
    /// recorded, or a slow catalogue sync would look like a lost collection.
    /// </summary>
    public Result<Unit, StoreError> SaveOwned(Kind kind, IReadOnlySet<long> owned) => Work(() =>
    {
        var kindName = kind.ToString();
        foreach (var id in owned)
        {
            var held = Context.Collectibles.Find(kindName, id);
            if (held is null)
            {
                Context.Collectibles.Add(new CollectibleRow { Kind = kindName, Id = id, Json = "{}", Owned = 1 });
            }
            else
            {
                held.Owned = 1;
            }
        }
        Context.SaveChanges();
        var keeping = owned.ToList();
        Context.Collectibles
            .Where(row => row.Kind == kindName && row.Owned != 0 && !keeping.Contains(row.Id))
            .ExecuteUpdate(set => set.SetProperty(row => row.Owned, 0L));
    });

    /// <summary>The catalogue for one kind, and what of it is owned.</summary>
    public Result<(List<Collectible> Catalogue, HashSet<long> Owned), StoreError> CollectiblesHeld(Kind kind) => Work(() =>
    {
        var kindName = kind.ToString();
        var catalogue = new List<Collectible>();
        var owned = new HashSet<long>();
        foreach (var row in Context.Collectibles.AsNoTracking().Where(row => row.Kind == kindName).OrderBy(row => row.Id))
        {
            if (row.Owned != 0)
            {
                owned.Add(row.Id);
            }
            try
            {
                if (JsonSerializer.Deserialize<Collectible>(row.Json, Collectible.Json) is { } entry)
                {
                    catalogue.Add(entry);
                }
            }
            catch (JsonException)
            {
                // An ownership-only row, or one a newer build wrote.
            }
        }
        if (kind == Kind.Toy)
        {
            CollapseToys(catalogue, owned);
        }
        return (catalogue, owned);
    });

    /// <summary>
    /// Fold together the two id spaces a toy lives in.
    /// </summary>
    /// <remarks>
    /// The in-game toy box knows an item and the web API knows a toy, a
    /// separate id space nothing in the client exposes, so an account with
    /// both sources holds most of its toys twice. Two joins, in order of
    /// trust: by item, where a web-API row's link names another row's id; and
    /// by name for what that cannot reach, which is safe for toys in a way it
    /// would not be for mounts, because Blizzard ships several distinct mount
    /// ids called White Stallion and no two toys share a name. The surviving
    /// row is the richer one, and owning either counts as owning it.
    /// </remarks>
    internal static void CollapseToys(List<Collectible> catalogue, HashSet<long> owned)
    {
        var keep = new List<Collectible>(catalogue.Count);
        var byId = new Dictionary<long, int>();
        var byName = new Dictionary<string, int>(StringComparer.Ordinal);

        // Richest first, so the row that survives a join is the one worth
        // keeping. Item ids run to six figures where toy ids are in the low
        // thousands, so the larger is the item when nothing else separates them.
        var ordered = catalogue
            .OrderBy(entry => entry.Icon is null)
            .ThenBy(entry => entry.Description is null)
            .ThenBy(entry => entry.Name.Length == 0)
            .ThenByDescending(entry => entry.Id)
            .ToList();
        catalogue.Clear();

        foreach (var entry in ordered)
        {
            int? existing = byId.TryGetValue(entry.LinkId, out var byLink) ? byLink
                : byId.TryGetValue(entry.Id, out var byOwn) ? byOwn
                // A nameless row cannot be matched by name. Two of them share
                // nothing but their emptiness.
                : entry.Name.Length > 0 && byName.TryGetValue(entry.Name, out var named) ? named
                : null;
            if (existing is not { } at)
            {
                at = keep.Count;
                byId[entry.Id] = at;
                if (entry.LinkId != entry.Id)
                {
                    byId[entry.LinkId] = at;
                }
                if (entry.Name.Length > 0)
                {
                    byName[entry.Name] = at;
                }
                keep.Add(entry);
                continue;
            }
            // Owning it under either id is owning it.
            if (owned.Remove(entry.Id))
            {
                owned.Add(keep[at].Id);
            }
            keep[at] = keep[at].Merge(entry);
            if (keep[at].Name.Length > 0)
            {
                byName[keep[at].Name] = at;
            }
            byId[entry.Id] = at;
        }
        catalogue.AddRange(keep);
    }
}
