using System.Globalization;
using System.Text.Json.Nodes;
using Armory.Zones;

namespace Armory.Blizzard;

/// <summary>What one item response says about the item: the name a browser needs, and the binding that decides whether it can be sold at all.</summary>
public sealed record Item
{
    public string Name { get; init; } = "";

    /// <summary>False for Bind-on-Pickup. Absent binding means freely tradeable, so silence is a yes here, unusually.</summary>
    public bool Sellable { get; init; } = true;

    public string? Quality { get; init; }
}

/// <summary>
/// <c>/data/...</c>: the catalogue. Deliberately not where a criterion's
/// meaning comes from: the public achievement endpoint returns the tree's
/// shape and never the asset each node measures.
/// </summary>
public static class GameData
{
    private const SourceId Source = SourceId.BlizzardGameData;

    private static Request Static(Region region, string path) => Request.Get(Source, Api.Url(region, Namespace.Static, path));

    public static Request AchievementIndex(Region region) => Static(region, "/data/wow/achievement/index");

    public static Request AchievementOf(Region region, long id) => Static(region, string.Create(CultureInfo.InvariantCulture, $"/data/wow/achievement/{id}"));

    public static Request InstanceIndex(Region region) => Static(region, "/data/wow/journal-instance/index");

    public static Request InstanceOf(Region region, long id) => Static(region, string.Create(CultureInfo.InvariantCulture, $"/data/wow/journal-instance/{id}"));

    public static Request EncounterOf(Region region, long id) => Static(region, string.Create(CultureInfo.InvariantCulture, $"/data/wow/journal-encounter/{id}"));

    /// <summary>One item, for its name. A listing carries an item id and nothing else, and no endpoint turns ids into names in bulk.</summary>
    public static Request ItemOf(Region region, long id) => Static(region, string.Create(CultureInfo.InvariantCulture, $"/data/wow/item/{id}"));

    /// <summary>Look an item up by name. The field is locale-suffixed, because an item has a name per locale.</summary>
    public static Request ItemSearch(Region region, string name) =>
        Request.Get(Source, Api.Url(region, Namespace.Static, "/data/wow/search/item",
            ($"name.{region.DefaultLocale()}", name),
            ("orderby", "id"),
            // A name fragment matches hundreds of items; twenty-five is plenty.
            ("_pageSize", "25")));

    public static Outcome<List<(long Id, string Name)>> ParseInstanceIndex(byte[] body)
    {
        var parsed = Outcomes.ParseJson<List<(long, string)>>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        if (parsed.Value.At("instances") is not JsonArray list)
        {
            return new Outcome<List<(long, string)>>.Stale(new Reason.Malformed("the instance index carried no instances"));
        }
        var instances = new List<(long, string)>();
        foreach (var entry in list)
        {
            if (entry.At("id").Int() is { } id && entry.At("name").Str() is { } name)
            {
                instances.Add((id, name));
            }
        }
        return Outcomes.OfCollection(instances);
    }

    /// <summary>One instance, with the encounters it contains. <c>map</c> is a UiMapID, the key a zone and a session join on.</summary>
    public static Outcome<Instance> ParseInstance(byte[] body)
    {
        var parsed = Outcomes.ParseJson<Instance>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        var value = parsed.Value;
        if (value.At("id").Int() is not { } id || value.At("name").Str() is not { } name)
        {
            return new Outcome<Instance>.Stale(new Reason.Malformed("the instance had no id or name"));
        }
        return new Outcome<Instance>.Found(new Instance
        {
            Id = id,
            Name = name,
            Map = value.At("map").At("id").Int(),
            Description = value.At("description").Str() ?? "",
            Expansion = value.At("category").At("type").Str(),
            // Duplicates are real: a raid with a faction-split wing lists the same boss twice.
            Encounters = value.At("encounters").Items().Select(entry => entry.At("id").Int()).OfType<long>().ToList(),
        });
    }

    /// <summary>One encounter, with its lore and the items it drops. The nested item is the real item; the outer id is the journal's row number.</summary>
    public static Outcome<Encounter> ParseEncounter(byte[] body)
    {
        var parsed = Outcomes.ParseJson<Encounter>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        var value = parsed.Value;
        if (value.At("id").Int() is not { } id || value.At("name").Str() is not { } name)
        {
            return new Outcome<Encounter>.Stale(new Reason.Malformed("the encounter had no id or name"));
        }
        return new Outcome<Encounter>.Found(new Encounter
        {
            Id = id,
            Name = name,
            Description = value.At("description").Str() ?? "",
            Loot = value.At("items").Items().Select(entry => entry.At("item").At("id").Int()).OfType<long>().ToList(),
        });
    }

    /// <summary>One item's name, binding and quality.</summary>
    public static Outcome<Item> ParseItem(byte[] body)
    {
        var parsed = Outcomes.ParseJson<Item>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        var value = parsed.Value;
        if (value.At("name").Str() is not { Length: > 0 } name)
        {
            return new Outcome<Item>.Stale(new Reason.Malformed("the item response carried no name"));
        }
        return new Outcome<Item>.Found(new Item
        {
            Name = name,
            Sellable = value.At("preview_item").At("binding").At("type").Str() != "ON_ACQUIRE",
            Quality = value.At("quality").At("type").Str(),
        });
    }

    /// <summary>The name off an item response. An item with no name is a shape that changed, not an item nobody named.</summary>
    public static Outcome<string> ParseItemName(byte[] body)
    {
        var parsed = Outcomes.ParseJson<string>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        return parsed.Value.At("name").Str() is { Length: > 0 } name
            ? new Outcome<string>.Found(name)
            : new Outcome<string>.Stale(new Reason.Malformed("the item response carried no name"));
    }

    /// <summary>Read a search response into ids and names. Search results carry every locale's name in one object.</summary>
    public static Outcome<List<(long Id, string Name)>> ParseItemSearch(byte[] body, string locale)
    {
        var parsed = Outcomes.ParseJson<List<(long, string)>>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        if (parsed.Value.At("results") is not JsonArray results)
        {
            return new Outcome<List<(long, string)>>.Stale(new Reason.Malformed("the item search carried no results"));
        }
        var found = new List<(long, string)>();
        foreach (var result in results)
        {
            var data = result.At("data");
            if (data.At("id").Int() is not { } id || data.At("name") is not { } names)
            {
                continue;
            }
            if ((names.At(locale) ?? names.At("en_US")).Str() is { } name)
            {
                found.Add((id, name));
            }
        }
        return Outcomes.OfCollection(found);
    }

    /// <summary>Read one achievement out of the catalogue. Blizzard flags the category, not the achievement, as unrepeatable.</summary>
    public static Outcome<Achievement> ParseAchievement(byte[] body)
    {
        var parsed = Outcomes.ParseJson<Achievement>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        var value = parsed.Value;
        if (value.At("id").Int() is not { } id)
        {
            return new Outcome<Achievement>.Stale(new Reason.Malformed("an achievement with no id"));
        }
        var category = value.At("category");
        var categoryName = category.Named();
        var isUnrepeatable = (category.At("is_guild_category").Flag() ?? false)
            || categoryName.Contains("Feats of Strength", StringComparison.Ordinal)
            || categoryName.Contains("Legacy", StringComparison.Ordinal);
        return new Outcome<Achievement>.Found(new Achievement
        {
            Id = id,
            Name = value.At("name").Str() ?? "",
            Category = categoryName,
            Points = value.At("points").Int() ?? 0,
            Description = value.At("description").Str() ?? "",
            IsUnrepeatable = isUnrepeatable,
        });
    }

    public static Outcome<List<long>> ParseAchievementIndex(byte[] body)
    {
        var parsed = Outcomes.ParseJson<List<long>>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        if (parsed.Value.At("achievements") is not JsonArray list)
        {
            return new Outcome<List<long>>.Stale(new Reason.Malformed("the achievement index carried no achievements"));
        }
        return Outcomes.OfCollection(list.Select(entry => entry.At("id").Int()).OfType<long>().ToList());
    }
}
