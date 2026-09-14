using System.Globalization;
using System.Text;

namespace Armory.Chronicle;

/// <summary>The few words every page says the same way.</summary>
public static class Prose
{
    /// <summary><c>1 quest</c>, <c>3 quests</c>. Public because the page counts the same things elsewhere, and two pluralisers is how "1 quests" reaches a screen.</summary>
    public static string Plural(long count, string one, string many) =>
        string.Create(CultureInfo.InvariantCulture, $"{count} {(count == 1 ? one : many)}");

    /// <summary>Copper as a person says it: <c>1,204g 30s 05c</c>.</summary>
    public static string Money(long copper)
    {
        var gold = copper / 10_000;
        var silver = (copper % 10_000) / 100;
        var bronze = copper % 100;
        if (gold > 0)
        {
            return string.Create(CultureInfo.InvariantCulture, $"{Thousands(gold)}g {silver:00}s {bronze:00}c");
        }
        if (silver > 0)
        {
            return string.Create(CultureInfo.InvariantCulture, $"{silver}s {bronze:00}c");
        }
        return string.Create(CultureInfo.InvariantCulture, $"{bronze}c");
    }

    /// <summary>The same, signed, for a delta.</summary>
    public static string Purse(long copper) =>
        copper < 0 ? "−" + Money(Math.Abs(copper)) : "+" + Money(copper);

    /// <summary>
    /// A number grouped so it can be read rather than counted. The comma, as
    /// the Rust core's <c>chronicle::money</c> writes it, and the one grouping
    /// this shell uses; the GTK almanac's narrow no-break space was a
    /// typographic choice of that design, which is not carried over.
    /// </summary>
    public static string Thousands(long number)
    {
        var digits = number.ToString(CultureInfo.InvariantCulture);
        var builder = new StringBuilder(digits.Length + digits.Length / 3);
        for (var index = 0; index < digits.Length; index++)
        {
            if (index > 0 && (digits.Length - index) % 3 == 0)
            {
                builder.Append(',');
            }
            builder.Append(digits[index]);
        }
        return builder.ToString();
    }

    /// <summary>
    /// A count, in words.
    /// "Eleven closed this week" is a sentence and "11 closed this week" is a
    /// readout. A headline is the one place that is written rather than
    /// reported, so it spells its number — up to the point where a word stops
    /// being easier to read than a figure.
    /// </summary>
    public static string Spelled(long count)
    {
        string[] words =
        [
            "Nothing", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Ten",
            "Eleven", "Twelve", "Thirteen", "Fourteen", "Fifteen", "Sixteen", "Seventeen", "Eighteen", "Nineteen", "Twenty",
        ];
        return count >= 0 && count < words.Length ? words[count] : count.ToString(CultureInfo.InvariantCulture);
    }

    /// <summary>
    /// A reputation rank as its name, in the game's own one-based order.
    /// Anything outside it is described rather than guessed at: a wrong
    /// standing is a wrong claim about somebody's play.
    /// </summary>
    public static string Standing(int rank) => rank switch
    {
        1 => "Hated",
        2 => "Hostile",
        3 => "Unfriendly",
        4 => "Neutral",
        5 => "Friendly",
        6 => "Honored",
        7 => "Revered",
        8 => "Exalted",
        _ => string.Create(CultureInfo.InvariantCulture, $"rank {rank}"),
    };

    /// <summary>How long something took, said the way a person would.</summary>
    public static string Spell(TimeSpan duration)
    {
        var minutes = Math.Max((long)duration.TotalMinutes, 0);
        if (minutes < 60)
        {
            return string.Create(CultureInfo.InvariantCulture, $"{minutes} min");
        }
        var hours = minutes / 60;
        var rest = minutes % 60;
        return rest == 0
            ? string.Create(CultureInfo.InvariantCulture, $"{hours} hr")
            : string.Create(CultureInfo.InvariantCulture, $"{hours} hr {rest} min");
    }
}
