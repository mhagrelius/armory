using System.Globalization;

namespace Armory.Zones;

/// <summary>
/// The sentences and figures the Zones page is drawn in. Pure, and ported
/// from the GTK page's helpers with their tests, so a page in any shell
/// says the same things.
/// </summary>
public static class ZoneProse
{
    /// <summary>How many quests an evening names before counting the rest.</summary>
    public const int QuestsNamed = 3;

    /// <summary>
    /// A span as a figure rather than as a sentence. Hours where there are
    /// hours, minutes where there are not: "3 hr 41 min" is right in prose
    /// and wrong in a hero figure beside a zone's name.
    /// </summary>
    public static string Span(long seconds)
    {
        var minutes = seconds / 60;
        return minutes < 60
            ? string.Create(CultureInfo.InvariantCulture, $"{minutes}m")
            : string.Create(CultureInfo.InvariantCulture, $"{minutes / 60}h");
    }

    /// <summary>The quests an evening closed, named and then counted.</summary>
    public static string TurnedIn(IReadOnlyList<string> quests)
    {
        var named = quests.Take(QuestsNamed).ToList();
        var rest = quests.Count - named.Count;
        var list = string.Join(", ", named);
        return rest == 0 ? $"Turned in {list}" : string.Create(CultureInfo.InvariantCulture, $"Turned in {list} and {rest} more");
    }

    /// <summary>
    /// The same name said twice in one evening, counted rather than
    /// repeated. Three identical chips in a row read as a bug in the page
    /// rather than as three deaths. First seen first, so the order is still
    /// the evening's.
    /// </summary>
    public static List<(string Name, int Count)> Tallied(IEnumerable<string> names)
    {
        var counted = new List<(string Name, int Count)>();
        foreach (var name in names)
        {
            var index = counted.FindIndex(entry => entry.Name == name);
            if (index >= 0)
            {
                counted[index] = (name, counted[index].Count + 1);
            }
            else
            {
                counted.Add((name, 1));
            }
        }
        return counted;
    }

    /// <summary>"Died to a Gorian Warlock", or "Died to a Gorian Warlock ×3".</summary>
    public static string Counted(string lead, string name, int count) =>
        count > 1 ? string.Create(CultureInfo.InvariantCulture, $"{lead} {name} ×{count}") : $"{lead} {name}";

    /// <summary>
    /// What keeps killing you here, most often first. The place's own tally
    /// when there is one; the evenings counted when there is not. Both
    /// answer the same question from the same records. By name where the
    /// counts tie, so two redraws of the same evening do not shuffle the list.
    /// </summary>
    public static List<(string Killer, long Count)> Killers(Place place)
    {
        if (place.Killers.Count > 0)
        {
            return place.Killers.ToList();
        }
        return place.Visits
            .SelectMany(visit => visit.Deaths)
            .GroupBy(death => death, StringComparer.Ordinal)
            .Select(group => (Killer: group.Key, Count: (long)group.Count()))
            .OrderByDescending(entry => entry.Count)
            .ThenBy(entry => entry.Killer, StringComparer.Ordinal)
            .ToList();
    }
}
