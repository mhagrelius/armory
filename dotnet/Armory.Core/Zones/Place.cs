using System.Globalization;
using System.Reflection;
using System.Text.Json;
using System.Text.Json.Serialization;
using Armory.Chronicle;
using Armory.Tally;

namespace Armory.Zones;

/// <summary>One zone's lore, as written in <c>data/zones.json</c>.</summary>
public sealed record Lore
{
    [JsonPropertyName("zone")]
    public string Zone { get; init; } = "";

    /// <summary>Null for the handful the wiki's own map table never listed, which the addon supplies instead.</summary>
    [JsonPropertyName("map")]
    public long? Map { get; init; }

    [JsonPropertyName("expansion")]
    public string Expansion { get; init; } = "";

    [JsonPropertyName("summary")]
    public string Summary { get; init; } = "";

    [JsonPropertyName("history")]
    public string History { get; init; } = "";

    [JsonPropertyName("factions")]
    public List<string> Factions { get; init; } = [];

    [JsonPropertyName("notable")]
    public List<Named> Notable { get; init; } = [];

    [JsonPropertyName("sources")]
    public List<Citation> Sources { get; init; } = [];

    [JsonPropertyName("licence")]
    public string Licence { get; init; } = "";
}

/// <summary>One instance's lore, for the raids Blizzard never wrote up.</summary>
public sealed record Written
{
    [JsonPropertyName("instance")]
    public string Instance { get; init; } = "";

    /// <summary>The Adventure Guide's own instance id, which is what this joins on.</summary>
    [JsonPropertyName("journal")]
    public long Journal { get; init; }

    [JsonPropertyName("summary")]
    public string Summary { get; init; } = "";

    [JsonPropertyName("history")]
    public string History { get; init; } = "";

    /// <summary>What the place takes for granted that the game never tells you. Karazhan assumes you know who Medivh was.</summary>
    [JsonPropertyName("assumes")]
    public string? Assumes { get; init; }

    /// <summary>Where the sources conflict or the wiki hedges, recorded rather than silently resolved.</summary>
    [JsonPropertyName("disputed")]
    public List<string> Disputed { get; init; } = [];

    [JsonPropertyName("notable")]
    public List<Named> Notable { get; init; } = [];

    [JsonPropertyName("sources")]
    public List<Citation> Sources { get; init; } = [];
}

public sealed record Named
{
    [JsonPropertyName("name")]
    public string Name { get; init; } = "";

    [JsonPropertyName("what")]
    public string What { get; init; } = "";
}

/// <summary>Where a piece of the corpus came from. The Rust calls this <c>Source</c>; the name is taken here by the collections' one.</summary>
public sealed record Citation
{
    [JsonPropertyName("title")]
    public string Title { get; init; } = "";

    [JsonPropertyName("url")]
    public string Url { get; init; } = "";
}

/// <summary>A dungeon or raid as the page shows it: the guide's account, or ours, and the page says which.</summary>
public sealed record Delve
{
    public string Name { get; init; } = "";

    public string Description { get; init; } = "";

    /// <summary>True when the words are Armory's rather than Blizzard's.</summary>
    public bool Ours { get; init; }

    public string? Assumes { get; init; }

    public List<string> Disputed { get; init; } = [];

    public List<Encounter> Bosses { get; init; } = [];
}

/// <summary>What one evening in this place amounted to.</summary>
public sealed record Visit
{
    public string Character { get; init; } = "";

    public DateTimeOffset At { get; init; }

    public List<string> Quests { get; init; } = [];

    public List<string> Deaths { get; init; } = [];

    public List<string> Rares { get; init; } = [];
}

/// <summary>Something that drops here and can actually be sold. Bind-on-Equip only, because most raid loot has no market at any price.</summary>
public sealed record Spoil
{
    public long Item { get; init; }

    /// <summary>The name, once one has been fetched. Ids arrive before names do.</summary>
    public string? Name { get; init; }

    public string From { get; init; } = "";

    /// <summary>The cheapest it is listed for, in copper.</summary>
    public long Cheapest { get; init; }

    public long Quantity { get; init; }
}

/// <summary>Everything about one place, assembled on Blizzard's <c>UiMapID</c>. The name is never the key: there are two Nagrands.</summary>
public sealed record Place
{
    public long Map { get; init; }

    public string Name { get; init; } = "";

    public Lore? Lore { get; init; }

    public List<Delve> Delves { get; init; } = [];

    public List<Visit> Visits { get; init; } = [];

    /// <summary>Seconds this account has spent here, across every character.</summary>
    public long Spent { get; init; }

    public List<(string Killer, long Count)> Killers { get; init; } = [];

    public List<Spoil> Spoils { get; init; } = [];

    /// <summary>A zone with lore nobody has visited is worth a page; so is one somebody has lived in that Armory has no lore for.</summary>
    public bool IsWorthShowing => Lore is not null || Visits.Count > 0 || Spent > 0;
}

/// <summary>What the page knows about an item: its name and whether it can be sold at all.</summary>
public sealed record ItemFact(string Name, bool Sellable);

/// <summary>The zone corpus and the join over it.</summary>
public static class Places
{
    private static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.General);

    /// <summary>The lore corpus, compiled in. Shipped rather than fetched: a zone page costs no request, works with no network, and needs no licence.</summary>
    public static List<Lore> Corpus() => Embedded<List<Lore>>("zones.json") ?? [];

    /// <summary>The raids Blizzard's own guide has nothing to say about, by journal id.</summary>
    public static Dictionary<long, Written> Unwritten()
    {
        var written = new Dictionary<long, Written>();
        foreach (var entry in Embedded<List<Written>>("instances.json") ?? [])
        {
            written[entry.Journal] = entry;
        }
        return written;
    }

    private static T? Embedded<T>(string name)
        where T : class
    {
        using var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream(name);
        if (stream is null)
        {
            return null;
        }
        try
        {
            return JsonSerializer.Deserialize<T>(stream, Json);
        }
        catch (JsonException)
        {
            return null;
        }
    }

    /// <summary>
    /// Assemble one place from everything on hand. Sessions are filtered here
    /// because the filter is the interesting part: a session passes through
    /// several zones, and what counts is the moments that happened while the
    /// character was standing in this one.
    /// </summary>
    public static Place Assemble(
        long map,
        Lore? lore,
        Guide guide,
        IReadOnlyDictionary<long, Written> written,
        IEnumerable<Session> sessions,
        Tallies tallies,
        IReadOnlyDictionary<long, ItemFact> items,
        IReadOnlyDictionary<long, (long Cheapest, long Quantity)> market)
    {
        var name = lore?.Zone ?? "";
        var delves = new List<Delve>();
        var spoils = new List<Spoil>();
        // An instance sits on its own map, so this is what a person standing
        // inside one sees.
        if (guide.At(map) is { } instance)
        {
            delves.Add(DelveOf(instance, guide, written));
            if (name.Length == 0)
            {
                name = instance.Name;
            }
            spoils = SpoilsOf(instance, guide, items, market);
        }

        var visits = sessions.Select(session => Visited(session, map)).OfType<Visit>().OrderByDescending(visit => visit.At).ToList();

        long spent = 0;
        var key = map.ToString(CultureInfo.InvariantCulture);
        foreach (var counted in tallies.Values)
        {
            foreach (var entry in Counters.Of(counted, Counting.Zone))
            {
                if (entry.Key == key)
                {
                    spent += entry.Count;
                    if (name.Length == 0)
                    {
                        name = entry.Label;
                    }
                }
            }
        }

        return new Place { Map = map, Name = name, Lore = lore, Delves = delves, Visits = visits, Spent = spent, Spoils = spoils };
    }

    /// <summary>One instance as the page shows it, preferring Blizzard's words to ours. Ours only where the guide is silent, which is every raid older than Mists.</summary>
    internal static Delve DelveOf(Instance instance, Guide guide, IReadOnlyDictionary<long, Written> written)
    {
        var ours = instance.Description.Length == 0 && written.TryGetValue(instance.Id, out var w) ? w : null;
        return new Delve
        {
            Name = instance.Name,
            Description = ours?.History ?? instance.Description,
            Ours = ours is not null,
            Assumes = ours?.Assumes,
            Disputed = ours?.Disputed.ToList() ?? [],
            Bosses = instance.Encounters.Select(id => guide.Encounters.GetValueOrDefault(id)).OfType<Encounter>().ToList(),
        };
    }

    /// <summary>
    /// What drops here, can be sold, and has a price on it right now. An item
    /// absent from the item table has an unknown binding, and an unknown is
    /// not shown: the alternative is offering somebody a Bind-on-Pickup drop
    /// as a thing to sell.
    /// </summary>
    internal static List<Spoil> SpoilsOf(Instance instance, Guide guide, IReadOnlyDictionary<long, ItemFact> items, IReadOnlyDictionary<long, (long Cheapest, long Quantity)> market)
    {
        var spoils = new List<Spoil>();
        foreach (var id in instance.Encounters)
        {
            if (!guide.Encounters.TryGetValue(id, out var encounter))
            {
                continue;
            }
            foreach (var item in encounter.Loot)
            {
                if (!items.TryGetValue(item, out var fact) || !fact.Sellable || !market.TryGetValue(item, out var priced))
                {
                    continue;
                }
                spoils.Add(new Spoil
                {
                    Item = item,
                    Name = fact.Name,
                    From = encounter.Name.TrimEnd(',', ' '),
                    Cheapest = priced.Cheapest,
                    Quantity = priced.Quantity,
                });
            }
        }
        return spoils
            .OrderByDescending(spoil => spoil.Cheapest)
            .GroupBy(spoil => spoil.Item)
            .Select(group => group.First())
            .ToList();
    }

    /// <summary>What a character did while standing in one zone. A session that never entered this map answers null rather than an empty visit.</summary>
    internal static Visit? Visited(Session session, long map)
    {
        var here = false;
        var ever = false;
        var visit = new Visit { Character = session.DisplayName, At = session.StartedAt };
        foreach (var moment in session.Moments)
        {
            switch (moment.What)
            {
                case Happening.Arrived arrived:
                    here = arrived.Map == map;
                    ever |= here;
                    break;
                case Happening.Completed completed when here:
                    visit.Quests.Add(completed.Title);
                    break;
                case Happening.Died died when here:
                    visit.Deaths.Add(died.To ?? "something");
                    break;
                case Happening.Rare rare when here:
                    visit.Rares.Add(rare.Name);
                    break;
                default:
                    break;
            }
        }
        return ever ? visit : null;
    }
}
