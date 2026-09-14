using System.Globalization;
using Armory.Chronicle;
using Armory.Roster;

namespace Armory.Client.Roster;

/// <summary>What the roster page says about a character, worked out away from any widget so it can be tested. The pure half of the GTK roster page.</summary>
public static class RosterProse
{
    /// <summary>
    /// The sentence at the top of the page.
    /// Spelled rather than reported, because it is the one line here that is
    /// written about the account rather than measured off it. Past twenty
    /// <see cref="Prose.Spelled"/> hands back a figure, which is where a word
    /// stops being easier to read than a number.
    /// </summary>
    public static string Headline(int enrolled, int total)
    {
        var of = Prose.Spelled(total).ToLowerInvariant();
        return enrolled switch
        {
            0 => "Nobody is enrolled, so there is nothing for a run to be about",
            1 => $"One of {of} is what this run is about",
            _ => $"{Prose.Spelled(enrolled)} of {of} are what this run is about",
        };
    }

    /// <summary>
    /// The line under a character's name.
    /// Level, race and class always; the spec and the primary professions once
    /// they have arrived. A character whose detail has not landed yet reads the
    /// same as one with none rather than showing a row of dashes that look like
    /// missing data.
    /// </summary>
    public static string Subtitle(Character character, Detail? detail)
    {
        // The spec replaces the bare class: "Restoration Shaman" says more
        // than "Shaman" and takes the same room.
        var who = detail?.Spec is { Length: > 0 } spec
            ? string.Create(CultureInfo.InvariantCulture, $"Level {character.Level} {character.Race} {spec} {character.Class}")
            : string.Create(CultureInfo.InvariantCulture, $"Level {character.Level} {character.Race} {character.Class}");
        if (detail is null)
        {
            return who;
        }
        var primaries = detail.Professions.Where(profession => profession.IsPrimary).Select(profession => profession.Name).ToList();
        return primaries.Count == 0 ? who : $"{who} · {string.Join(", ", primaries)}";
    }

    /// <summary>
    /// What the row has no room for: the specialisation trees, and the rating.
    /// Two characters with Alchemy at 100 can have spent a year of weekly
    /// knowledge in completely different places, and the profile API cannot say
    /// which — it has the expansion tier and stops there.
    /// </summary>
    public static string? Depths(Detail detail)
    {
        var lines = new List<string>();
        foreach (var profession in detail.Professions)
        {
            var open = profession.Specialisations.Where(tree => tree.Opened).Select(tree => tree.Name).ToList();
            if (open.Count == 0)
            {
                continue;
            }
            var knowledge = profession.Knowledge == 0 ? "" : string.Create(CultureInfo.InvariantCulture, $" — {profession.Knowledge} knowledge");
            lines.Add($"{profession.Name}: {string.Join(", ", open)}{knowledge}");
        }
        if (detail.MythicRating is { } rating)
        {
            lines.Add(string.Create(CultureInfo.InvariantCulture, $"Mythic+ rating {rating}"));
        }
        return lines.Count == 0 ? null : string.Join("\n", lines);
    }

    /// <summary>What the portrait says when hovered: race, class and side.</summary>
    public static string Who(Character character) => $"{character.Race} {character.Class} — {character.Faction.Label()}";

    /// <summary>
    /// Whether a character answers the search. With thirty-one characters the
    /// question is as often "who is on Mannoroth" or "where is my druid" as it
    /// is a name.
    /// </summary>
    public static bool Matches(Character character, Detail? detail, string needle)
    {
        if (needle.Length == 0)
        {
            return true;
        }
        var text = $"{character.DisplayName} {character.RealmName} {character.Class} {character.Race} {character.Faction.Label()} {detail?.Spec ?? ""}";
        return text.Contains(needle, StringComparison.OrdinalIgnoreCase);
    }
}
