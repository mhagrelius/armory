using System.Globalization;
using Armory.Zones;
using Microsoft.EntityFrameworkCore;

namespace Armory.Store;

/// <summary>The Adventure Guide, kept like the achievement catalogue: static game data, fetched and never purged.</summary>
public sealed partial class Store
{
    public Result<Unit, StoreError> SaveInstance(Instance instance) => Work(() =>
    {
        var held = Context.Instances.Find(instance.Id);
        var encounters = Joined(instance.Encounters);
        if (held is null)
        {
            Context.Instances.Add(new InstanceRow
            {
                Id = instance.Id,
                Name = instance.Name,
                Map = instance.Map,
                Description = instance.Description,
                Expansion = instance.Expansion ?? "",
                Encounters = encounters,
            });
        }
        else
        {
            held.Name = instance.Name;
            held.Map = instance.Map;
            held.Description = instance.Description;
            held.Expansion = instance.Expansion ?? "";
            held.Encounters = encounters;
        }
    });

    public Result<Unit, StoreError> SaveEncounter(Encounter encounter) => Work(() =>
    {
        var held = Context.Encounters.Find(encounter.Id);
        var loot = Joined(encounter.Loot);
        if (held is null)
        {
            Context.Encounters.Add(new EncounterRow { Id = encounter.Id, Name = encounter.Name, Description = encounter.Description, Loot = loot });
        }
        else
        {
            held.Name = encounter.Name;
            held.Description = encounter.Description;
            held.Loot = loot;
        }
    });

    /// <summary>Everything the guide has told us so far.</summary>
    public Result<Guide, StoreError> GuideHeld() => Work(() =>
    {
        var guide = new Guide();
        foreach (var row in Context.Instances.AsNoTracking())
        {
            guide.Instances[row.Id] = new Instance
            {
                Id = row.Id,
                Name = row.Name,
                Map = row.Map,
                Description = row.Description,
                Expansion = row.Expansion.Length == 0 ? null : row.Expansion,
                Encounters = Split(row.Encounters),
            };
        }
        foreach (var row in Context.Encounters.AsNoTracking())
        {
            guide.Encounters[row.Id] = new Encounter { Id = row.Id, Name = row.Name, Description = row.Description, Loot = Split(row.Loot) };
        }
        return guide;
    });

    /// <summary>Which instances and encounters have not been fetched yet, answered from the store so there is no second copy of the guide to disagree with the first.</summary>
    public Result<(List<long> Instances, List<long> Encounters), StoreError> GuideGaps(IEnumerable<(long Id, string Name)> known) => Work(() =>
    {
        var have = Context.Instances.AsNoTracking().Select(row => row.Id).ToHashSet();
        var instances = known.Select(entry => entry.Id).Where(id => !have.Contains(id)).ToList();
        var seen = Context.Encounters.AsNoTracking().Select(row => row.Id).ToHashSet();
        var wanted = Context.Instances.AsNoTracking().AsEnumerable()
            .SelectMany(row => Split(row.Encounters))
            .Where(id => !seen.Contains(id))
            .ToList();
        return (instances, wanted);
    });

    /// <summary>A list of ids as one column. An encounter's loot is read whole and never queried by item.</summary>
    private static string Joined(IEnumerable<long> ids) => string.Join(",", ids.Select(id => id.ToString(CultureInfo.InvariantCulture)));

    private static List<long> Split(string text)
    {
        var ids = new List<long>();
        foreach (var part in text.Split(','))
        {
            if (long.TryParse(part, NumberStyles.Integer, CultureInfo.InvariantCulture, out var id))
            {
                ids.Add(id);
            }
        }
        return ids;
    }
}
