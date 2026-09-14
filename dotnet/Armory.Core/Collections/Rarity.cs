using System.Globalization;
using Armory.Addon;

namespace Armory.Collections;

/// <summary>One item's estimated drop chance, and the ids it can be joined on.</summary>
public sealed record Chance
{
    /// <summary>The English name Rarity keys the entry by. For the tooltip, not for joining.</summary>
    public string Name { get; init; } = "";

    /// <summary>A mount's summoning spell: what Armory's mount link id is.</summary>
    public long? SpellId { get; init; }

    /// <summary>A pet's creature: what Armory's pet link id is.</summary>
    public long? CreatureId { get; init; }

    /// <summary>The item that teaches or contains it: what a toy's link id is.</summary>
    public long? ItemId { get; init; }

    /// <summary>One in this many. Never zero.</summary>
    public long OneIn { get; init; }
}

/// <summary>Every drop chance Rarity knows, indexed the three ways a collectible joins.</summary>
public sealed class Chances
{
    private readonly Dictionary<long, long> bySpell = [];
    private readonly Dictionary<long, long> byCreature = [];
    private readonly Dictionary<long, long> byItem = [];

    public Chances(IEnumerable<Chance> entries)
    {
        foreach (var entry in entries)
        {
            Known++;
            if (entry.SpellId is { } spell)
            {
                bySpell[spell] = entry.OneIn;
            }
            if (entry.CreatureId is { } creature)
            {
                byCreature[creature] = entry.OneIn;
            }
            if (entry.ItemId is { } item)
            {
                byItem[item] = entry.OneIn;
            }
        }
    }

    /// <summary>How many entries were read, for the line that says where this came from.</summary>
    public int Known { get; }

    public bool IsEmpty => Known == 0;

    /// <summary>
    /// One in how many, for a thing Armory knows about. Joined on the link
    /// id, which is a different id space per kind and exactly the one Rarity
    /// keys by. A toy whose link id is still the collection id standing in
    /// for an item is refused: the id is a guess, and a guess joined against
    /// a real table lands on a real wrong answer.
    /// </summary>
    public long? OneIn(Collectible collectible)
    {
        if (collectible.Kind is Kind.Toy or Kind.Decor && collectible.LinkId == collectible.Id)
        {
            return null;
        }
        var table = collectible.Kind switch
        {
            Kind.Mount => bySpell,
            Kind.Pet => byCreature,
            _ => byItem,
        };
        return table.TryGetValue(collectible.LinkId, out var oneIn) ? oneIn : null;
    }
}

/// <summary>
/// Drop chances, read out of the Rarity addon a person already has installed.
/// </summary>
/// <remarks>
/// <para><b>Read, never shipped.</b> Rarity is GPL-2.0 with no "or later"
/// grant and Armory is GPL-3.0-or-later, so its database cannot go in this
/// repository or the binary. Reading a file on the machine that already runs
/// both is a different act.</para>
/// <para><c>chance = 100</c> means one in a hundred, not a hundred per cent,
/// and the figures are estimates read off Wowhead by Rarity's authors.</para>
/// <para>This is not a Lua interpreter. The files are hand-written Lua with
/// <c>LibStub</c> calls and an early <c>return {}</c>; this scans for a shape
/// it knows and takes four scalar fields, dropping any entry it cannot read
/// whole.</para>
/// </remarks>
public static class Rarity
{
    private const string Addon = "Rarity";
    private const string Database = "DB";

    /// <summary>Read every database file in an installed Rarity. Absent, unreadable or unrecognisable all answer the same empty set.</summary>
    public static Chances Read(string wowPath)
    {
        var entries = new List<Chance>();
        Collect(Path.Combine(Files.AddonDirectory(wowPath, Addon), Database), entries, 0);
        return new Chances(entries);
    }

    /// <summary>The database is one level of subdirectories deep. A bound rather than a trusted shape: a symlink loop in somebody else's folder is not Armory's problem.</summary>
    private static void Collect(string directory, List<Chance> into, int depth)
    {
        if (depth > 2 || !Directory.Exists(directory))
        {
            return;
        }
        IEnumerable<string> entries;
        try
        {
            entries = Directory.EnumerateFileSystemEntries(directory);
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException)
        {
            return;
        }
        foreach (var path in entries)
        {
            if (Directory.Exists(path))
            {
                Collect(path, into, depth + 1);
            }
            else if (string.Equals(Path.GetExtension(path), ".lua", StringComparison.OrdinalIgnoreCase))
            {
                try
                {
                    into.AddRange(Parse(File.ReadAllText(path)));
                }
                catch (Exception error) when (error is IOException or UnauthorizedAccessException)
                {
                    // One unreadable file is not a reason to lose the rest.
                }
            }
        }
    }

    /// <summary>Pull every entry this understands out of one database file.</summary>
    public static List<Chance> Parse(string source)
    {
        var found = new List<Chance>();
        Entry? open = null;
        // How deep inside the current entry's braces we are. The entry's own
        // fields are at one; anything deeper is coords, npcs or items and is
        // skipped whole, so a `m = 317` inside a coordinate is never a field.
        var depth = 0;

        foreach (var raw in source.Split('\n'))
        {
            var line = raw.Trim();
            if (open is null)
            {
                if (OpensEntry(line) is { } name)
                {
                    open = new Entry(name);
                    depth = 1 + Braces(line) - 1;
                }
                continue;
            }

            var before = depth;
            depth = Math.Max(depth + Delta(line), 0);
            if (depth == 0)
            {
                // The entry closed. Keep it only if it carried a usable chance
                // and at least one id to join it on.
                if (open.Finish() is { } chance)
                {
                    found.Add(chance);
                }
                open = null;
                continue;
            }
            if (before == 1 && depth == 1)
            {
                open.ReadField(line);
            }
        }
        return found;
    }

    /// <summary>The name a line opens an entry with: <c>["Cloudwing Hippogryph"] = {</c>, strictly.</summary>
    private static string? OpensEntry(string line)
    {
        if (!line.StartsWith("[\"", StringComparison.Ordinal))
        {
            return null;
        }
        var end = line.IndexOf("\"]", 2, StringComparison.Ordinal);
        if (end < 0)
        {
            return null;
        }
        var rest = line[(end + 2)..].TrimStart();
        if (!rest.StartsWith('='))
        {
            return null;
        }
        return rest[1..].Trim().StartsWith('{') ? line[2..end] : null;
    }

    private static int Braces(string line) => line.Count(c => c == '{');

    private static int Delta(string line) => line.Count(c => c == '{') - line.Count(c => c == '}');

    /// <summary>One entry, part-read.</summary>
    private sealed class Entry
    {
        private readonly string name;
        private long? spellId;
        private long? creatureId;
        private long? itemId;
        private long? oneIn;

        public Entry(string name)
        {
            this.name = name;
        }

        /// <summary>
        /// Take one of the four fields this understands. Only a bare number
        /// is accepted; an expression is left alone. The trailing comment is
        /// cut first, because <c>chance = 100, -- Blind guess</c> is a real
        /// line and eighty-odd entries carry one.
        /// </summary>
        public void ReadField(string line)
        {
            var comment = line.IndexOf("--", StringComparison.Ordinal);
            if (comment >= 0)
            {
                line = line[..comment];
            }
            var equals = line.IndexOf('=', StringComparison.Ordinal);
            if (equals < 0)
            {
                return;
            }
            var field = line[..equals].Trim();
            var value = line[(equals + 1)..].Trim().TrimEnd(',').Trim();
            // Parsed as a float and rounded: exactly one entry says 2.5.
            if (!double.TryParse(value, NumberStyles.Float, CultureInfo.InvariantCulture, out var number) || !double.IsFinite(number) || number < 0)
            {
                return;
            }
            var rounded = (long)Math.Round(number, MidpointRounding.AwayFromZero);
            switch (field)
            {
                case "spellId":
                    spellId = rounded;
                    break;
                case "creatureId":
                    creatureId = rounded;
                    break;
                case "itemId":
                    itemId = rounded;
                    break;
                case "chance":
                    oneIn = rounded;
                    break;
                default:
                    break;
            }
        }

        /// <summary>The entry, if it is worth keeping: a chance above zero and something to join it to.</summary>
        public Chance? Finish()
        {
            if (oneIn is not { } chance || chance <= 0 || (spellId is null && creatureId is null && itemId is null))
            {
                return null;
            }
            return new Chance { Name = name, SpellId = spellId, CreatureId = creatureId, ItemId = itemId, OneIn = chance };
        }
    }
}
