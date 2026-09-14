using Armory.Tally;

namespace Armory.Collections;

/// <summary>
/// Something missing, what drops it, and how long you have been at it.
/// </summary>
/// <remarks>
/// Blizzard publishes no drop chance for anything. What Armory has instead is
/// better for the question a person is actually asking: "you have killed
/// Attumen the Huntsman forty-seven times and never seen it" is exact, it is
/// about you, and nothing else in the world can tell you it.
/// </remarks>
public sealed record Quarry
{
    public Kind Kind { get; init; }

    public long Id { get; init; }

    /// <summary>What you are after.</summary>
    public string Name { get; init; } = "";

    /// <summary>What drops it, as the in-game journal names it.</summary>
    public string From { get; init; } = "";

    /// <summary>Where that is. Null for a world drop, which names a creature and no place.</summary>
    public string? Place { get; init; }

    /// <summary>
    /// Times this account has put that creature down since the addon was
    /// installed. Never a lifetime figure: nothing in the game records how
    /// many times anybody has killed anything.
    /// </summary>
    public long Attempts { get; init; }
}

public static class Hunt
{
    /// <summary>
    /// The creature a journal sentence names, and where it says to find it.
    /// Deliberately narrow: anything that is not a leading <c>Drop:</c>
    /// followed by a name is refused rather than guessed at, because a wrong
    /// creature is a count of somebody else's kills attached to your mount.
    /// </summary>
    public static (string Who, string? Place)? DroppedBy(string description)
    {
        var colon = description.IndexOf(':', StringComparison.Ordinal);
        if (colon < 0 || !string.Equals(description[..colon].Trim(), "drop", StringComparison.OrdinalIgnoreCase))
        {
            return null;
        }
        var rest = description[(colon + 1)..].Trim();
        var comma = rest.IndexOf(',', StringComparison.Ordinal);
        var who = (comma < 0 ? rest : rest[..comma]).Trim();
        var place = comma < 0 ? null : rest[(comma + 1)..].Trim();
        if (who.Length == 0)
        {
            return null;
        }
        return (who, string.IsNullOrEmpty(place) ? null : place);
    }

    /// <summary>
    /// What this account is still hunting, longest-suffering first. It must
    /// be missing, the journal must say what drops it, and you must actually
    /// have fought it. Attempts are summed across every character, because a
    /// collection is account-wide.
    /// </summary>
    public static List<Quarry> Hunting(IEnumerable<Collectible> catalogue, IReadOnlySet<long> owned, Tallies tallies)
    {
        var kills = new Dictionary<string, long>(StringComparer.Ordinal);
        foreach (var counted in tallies.Values)
        {
            foreach (var kind in new[] { Counting.Victory, Counting.Rare })
            {
                foreach (var entry in Counters.Of(counted, kind))
                {
                    var creature = entry.Label.ToLowerInvariant();
                    kills[creature] = kills.GetValueOrDefault(creature) + entry.Count;
                }
            }
        }
        if (kills.Count == 0)
        {
            return [];
        }

        var hunts = new List<Quarry>();
        foreach (var entry in catalogue)
        {
            if (entry.Source != Source.Drop || owned.Contains(entry.Id) || entry.Description is null)
            {
                continue;
            }
            if (DroppedBy(entry.Description) is not var (from, place) || !kills.TryGetValue(from.ToLowerInvariant(), out var attempts) || attempts <= 0)
            {
                continue;
            }
            hunts.Add(new Quarry { Kind = entry.Kind, Id = entry.Id, Name = entry.Name, From = from, Place = place, Attempts = attempts });
        }
        return hunts.OrderByDescending(quarry => quarry.Attempts).ThenBy(quarry => quarry.Name, StringComparer.Ordinal).ToList();
    }
}
