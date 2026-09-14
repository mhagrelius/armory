using System.Globalization;
using System.Text;

namespace Armory.Client.Sharing;

/// <summary>The last pass as the dialog says it: when, what moved, and what went wrong.</summary>
public sealed record PassSummary(string When, int Sent, int Landed, int Removed, int Unreadable, string? Failed);

/// <summary>
/// What the sentence about sharing is written from. The port of the GTK sync
/// dialog's <c>State</c>, cut to the fields the wording reads.
/// </summary>
public sealed record SharingStatus
{
    /// <summary><c>http://host:port</c>, or empty when sharing is off.</summary>
    public string Server { get; init; } = "";

    /// <summary>Whether a pass is in flight right now.</summary>
    public bool Passing { get; init; }

    /// <summary>The last pass, or null before one has run.</summary>
    public PassSummary? Last { get; init; }

    /// <summary>Consecutive failures. Three is where it stops being noise.</summary>
    public int Failures { get; init; }
}

/// <summary>
/// The Account &amp; Sharing dialog's words, as pure functions: the port of
/// the six free functions at the foot of <c>src/ui/sync_dialog.rs</c>. The
/// dialog draws what these answer and adds nothing of its own.
/// </summary>
public static class SharingWording
{
    /// <summary>
    /// The account this machine belongs to when nothing has been chosen. An
    /// empty <c>SyncAccount</c> means this, and it is where a store from
    /// before the server held more than one was adopted.
    /// </summary>
    public const string DefaultAccount = "default";

    /// <summary>The longest name the server will take. <c>accounts::MAX_NAME</c> on its side.</summary>
    public const int MaxAccount = 64;

    /// <summary>
    /// The accounts worth offering: what the server holds, and the two that
    /// are true whether it has answered or not. <c>default</c> is always
    /// there because an empty setting means it. The current choice is always
    /// there because a server that has not been asked yet — or that has just
    /// been emptied — must not quietly move this machine somewhere else while
    /// the list is short.
    /// </summary>
    public static List<string> Options(IReadOnlyList<(string Name, long Rows)>? held, string current)
    {
        var names = (held ?? []).Select(account => account.Name).ToList();
        foreach (var known in new[] { DefaultAccount, current.Trim() })
        {
            if (known.Length > 0 && !names.Contains(known, StringComparer.Ordinal))
            {
                names.Add(known);
            }
        }
        return names;
    }

    /// <summary>Which option is this machine's, as an index into them.</summary>
    public static int? Selected(IReadOnlyList<string> options, string current)
    {
        var trimmed = current.Trim();
        var wanted = trimmed.Length == 0 ? DefaultAccount : trimmed;
        for (var index = 0; index < options.Count; index++)
        {
            if (string.Equals(options[index], wanted, StringComparison.Ordinal))
            {
                return index;
            }
        }
        return null;
    }

    /// <summary>
    /// Whether the server will take this as an account name: the same
    /// allow-list as <c>accounts::directory</c> on its side, which is what
    /// stands between a string off the network and that machine's filesystem.
    /// </summary>
    public static bool ValidAccount(string name) =>
        name.Length > 0
        && name.Length <= MaxAccount
        && name.All(Allowed)
        && !name.All(c => c == '.');

    /// <summary>
    /// A WoW account folder as a name the server will take. A Battle.net
    /// folder is very often <c>12345678#1</c>, and offering that verbatim
    /// would be offering a name that is saved, sent, and refused with a 400
    /// an hour later on a pass nobody is watching.
    /// </summary>
    public static string AccountFromFolder(string folder)
    {
        var cleaned = new StringBuilder();
        foreach (var rune in folder.Trim().EnumerateRunes())
        {
            if (cleaned.Length == MaxAccount)
            {
                break;
            }
            cleaned.Append(rune.IsAscii && Allowed((char)rune.Value) ? (char)rune.Value : '-');
        }
        var name = cleaned.ToString();
        return ValidAccount(name) ? name : DefaultAccount;
    }

    /// <summary>The last pass, in a sentence.</summary>
    public static string Describe(SharingStatus state)
    {
        if (state.Server.Length == 0)
        {
            return "Sharing is off.";
        }
        if (state.Passing)
        {
            return "Running now…";
        }
        if (state.Last is not { } last)
        {
            return "Not yet.";
        }
        if (last.Failed is { } error)
        {
            // The count is the thing worth knowing. One failed pass is a NAS
            // asleep or a machine between networks; five in a row is a problem.
            return $"{last.When} — failed {Plural(state.Failures, "time", "times")} in a row: {error}";
        }
        if (last.Sent + last.Landed + last.Removed == 0)
        {
            return $"{last.When} — nothing to do.";
        }

        var parts = new List<string>();
        if (last.Sent > 0)
        {
            parts.Add(Count(last.Sent, "up"));
        }
        if (last.Landed > 0)
        {
            parts.Add(Count(last.Landed, "down"));
        }
        if (last.Removed > 0)
        {
            parts.Add(Count(last.Removed, "removed"));
        }
        if (last.Unreadable > 0)
        {
            // Worth its own clause rather than a silent drop: a number here is
            // what one machine running an older build looks like.
            parts.Add(Count(last.Unreadable, "not understood"));
        }
        return $"{last.When} — {string.Join(", ", parts)}.";
    }

    /// <summary>
    /// A table's name as somebody would say it. The wire names are the SQL
    /// ones because one name is better than two, and <c>earned_reputation</c>
    /// is not what a person calls it.
    /// </summary>
    public static string Pretty(string scope) => scope switch
    {
        "character" => "Characters",
        "enrolment" => "Who is in the run",
        "detail" => "Character detail",
        "attribution" => "Who earned what",
        "currency" => "Currencies",
        "earned_reputation" => "Reputation earned",
        "earned_currency" => "Currency earned",
        "tally" => "Lifetime counters",
        "recipe" => "Recipes",
        "recipe_reagent" => "Recipe reagents",
        "instance" => "Dungeons and raids",
        "encounter" => "Bosses",
        "criterion" => "Achievement criteria",
        "warband_item" => "Warband bank",
        "pet_held" => "Pets held",
        "run" => "The run",
        "goal" => "Goals",
        "collectible" => "Collections",
        "achievement" => "Achievements",
        "price" => "Price history",
        "snapshot" => "The auction house",
        "item" => "Item names",
        "watched" => "Watched items",
        "watched_realm" => "Watched realms",
        "session" => "Evenings",
        "entry" => "Journal entries",
        "forgotten" => "Evenings you threw away",
        "response" => "Cached replies",
        _ => scope,
    };

    private static bool Allowed(char c) => char.IsAsciiLetterOrDigit(c) || c == '-' || c == '_' || c == '.';

    private static string Count(int count, string what) => string.Create(CultureInfo.InvariantCulture, $"{count} {what}");

    private static string Plural(int count, string one, string many) => Count(count, count == 1 ? one : many);
}
