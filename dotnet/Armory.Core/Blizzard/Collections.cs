using System.Globalization;
using System.Text.Json.Nodes;
using Armory.Collections;

namespace Armory.Blizzard;

/// <summary>
/// Mounts, pets, toys and decor: what the account has, and what exists. The
/// profile side says what is collected, account-wide; the game data side says
/// what exists at all; missing is the difference. What Blizzard will not say
/// is where anything comes from beyond one word.
/// </summary>
public static class CollectionsApi
{
    private const SourceId ProfileSource = SourceId.BlizzardProfile;
    private const SourceId DataSource = SourceId.BlizzardGameData;

    public static Request Collected(Region region, Kind kind) => Request.Get(ProfileSource, Api.Url(region, Namespace.Profile, kind switch
    {
        Kind.Mount => "/profile/user/wow/collections/mounts",
        Kind.Pet => "/profile/user/wow/collections/pets",
        Kind.Toy => "/profile/user/wow/collections/toys",
        _ => "/profile/user/wow/collections/decor",
    }));

    /// <summary>
    /// Read the ids the account already has. The responses nest differently:
    /// Blizzard names the list after the collection and the id after the
    /// thing, so each is picked apart on its own. Decor accepts both the
    /// nested and the flat form, because no fixture existed to record.
    /// </summary>
    public static Outcome<HashSet<long>> ParseCollected(byte[] body, Kind kind)
    {
        var parsed = Outcomes.ParseJson<HashSet<long>>(ProfileSource, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        var (listKeys, itemKey) = kind switch
        {
            Kind.Mount => (new[] { "mounts" }, "mount"),
            Kind.Pet => (new[] { "pets" }, "species"),
            Kind.Toy => (new[] { "toys" }, "toy"),
            _ => (new[] { "decor_items", "decor" }, "decor"),
        };
        JsonArray? list = null;
        foreach (var key in listKeys)
        {
            if (parsed.Value.At(key) is JsonArray found)
            {
                list = found;
                break;
            }
        }
        if (list is null)
        {
            return new Outcome<HashSet<long>>.Stale(new Reason.Malformed($"the {listKeys[0]} response carried no {listKeys[0]} list"));
        }
        var ids = new HashSet<long>();
        foreach (var entry in list)
        {
            if ((entry.At(itemKey).At("id").Int() ?? entry.At("id").Int()) is { } id)
            {
                ids.Add(id);
            }
        }
        return ids.Count == 0 ? new Outcome<HashSet<long>>.Empty() : new Outcome<HashSet<long>>.Found(ids);
    }

    public static Request Index(Region region, Kind kind) => Request.Get(DataSource, Api.Url(region, Namespace.Static, kind switch
    {
        Kind.Mount => "/data/wow/mount/index",
        Kind.Pet => "/data/wow/pet/index",
        Kind.Toy => "/data/wow/toy/index",
        _ => "/data/wow/decor/index",
    }));

    public static Request Detail(Region region, Kind kind, long id) => Request.Get(DataSource, Api.Url(region, Namespace.Static, kind switch
    {
        Kind.Mount => string.Create(CultureInfo.InvariantCulture, $"/data/wow/mount/{id}"),
        Kind.Pet => string.Create(CultureInfo.InvariantCulture, $"/data/wow/pet/{id}"),
        Kind.Toy => string.Create(CultureInfo.InvariantCulture, $"/data/wow/toy/{id}"),
        _ => string.Create(CultureInfo.InvariantCulture, $"/data/wow/decor/{id}"),
    }));

    /// <summary>
    /// Read an index into ids and names. The index gives no source at all;
    /// sources arrive one call at a time from <see cref="ParseDetail"/>. The
    /// decor index's list is <c>decor_items</c>, the one list key in this API
    /// that is not simply the plural.
    /// </summary>
    public static Outcome<List<Collectible>> ParseIndex(byte[] body, Kind kind)
    {
        var parsed = Outcomes.ParseJson<List<Collectible>>(DataSource, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        var listKey = kind switch
        {
            Kind.Mount => "mounts",
            Kind.Pet => "pets",
            Kind.Toy => "toys",
            _ => "decor_items",
        };
        if (parsed.Value.At(listKey) is not JsonArray list)
        {
            return new Outcome<List<Collectible>>.Stale(new Reason.Malformed($"the {listKey} index carried no {listKey}"));
        }
        var entries = new List<Collectible>();
        foreach (var entry in list)
        {
            if (entry.At("id").Int() is not { } id)
            {
                continue;
            }
            entries.Add(new Collectible
            {
                Kind = kind,
                Id = id,
                Name = entry.At("name").Str() ?? "",
                Source = Source.Unknown,
                // Until the detail call lands, the collection id is the best
                // link available. The web API does not say whether a pet is
                // tradeable, and silence is not "no".
                LinkId = id,
                Tradeable = null,
            });
        }
        return Outcomes.OfCollection(entries);
    }

    /// <summary>Read one collectible's detail, including whatever source Blizzard admits to.</summary>
    public static Outcome<Collectible> ParseDetail(byte[] body, Kind kind)
    {
        var parsed = Outcomes.ParseJson<Collectible>(DataSource, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        var value = parsed.Value;
        if (value.At("id").Int() is not { } id)
        {
            return new Outcome<Collectible>.Stale(new Reason.Malformed("a collectible with no id"));
        }
        // Mounts are indexed on Wowhead by the spell that summons them, pets
        // by their creature, and toys and decor by the item they wrap.
        var linkId = kind switch
        {
            Kind.Mount => (value.At("source_spell") ?? value.At("spell")).At("id").Int() ?? id,
            Kind.Pet => value.At("creature_display").At("id").Int() ?? id,
            _ => value.At("item").At("id").Int() ?? id,
        };
        return new Outcome<Collectible>.Found(new Collectible
        {
            Kind = kind,
            Id = id,
            // A toy has no name of its own: it wraps an item, and the name is the item's.
            Name = value.At("name").Str() ?? value.At("item").At("name").Str() ?? "",
            Source = value.At("source").At("type").Str() is { } code ? Sources.FromType(code) : Source.Unknown,
            Display = value.At("creature_display").At("id").Int(),
            LinkId = linkId,
            Tradeable = null,
        });
    }
}
