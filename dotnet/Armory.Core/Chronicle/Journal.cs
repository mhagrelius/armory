using System.Globalization;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Armory.Blizzard;
using Armory.Roster;
using Armory.Tally;

namespace Armory.Chronicle;

/// <summary>A written entry, as the model returned it.</summary>
public sealed record Written(string Title, string Body, string Model);

/// <summary>
/// A program to run: the same seam as <see cref="Request"/>, for a source
/// reached through a subprocess rather than a socket. The core says what to
/// run and what to feed it; the shell runs it.
/// </summary>
public sealed record Command(string Program, IReadOnlyList<string> Arguments, string Stdin)
{
    public bool Equals(Command? other) =>
        other is not null && Program == other.Program && Stdin == other.Stdin && Arguments.SequenceEqual(other.Arguments);

    public override int GetHashCode() => HashCode.Combine(Program, Stdin, Arguments.Count);
}

/// <summary>
/// The one source that is not a game database: a local llama-server, asked
/// to write four hundred words about an evening that already happened, from
/// facts Armory already holds.
/// </summary>
/// <remarks>
/// It talks to llama.cpp's OpenAI-compatible endpoint on this machine. No
/// credential, no bill, and nothing leaves the machine. The model is given
/// facts and no research errand: everything in the brief came off the
/// player's own screen, and it is told not to invent events.
/// </remarks>
public static class Journal
{
    /// <summary>Where a llama-server usually is.</summary>
    public const string DefaultServer = "http://127.0.0.1:8080";

    /// <summary>How long a journal entry may run to, in tokens. Generous, because a thinking model spends some of it reasoning.</summary>
    private const int MaxTokens = 2_048;

    private const SourceId Source = SourceId.Journal;

    /// <summary>
    /// How the entry should read: a short list of ways to be wrong rather
    /// than a long list of ways to be right.
    /// </summary>
    private const string Voice =
        "You keep a travel journal for a character in World of Warcraft. You are given " +
        "the log of one play session and you write that evening's entry.\n\n" +
        "Write in the character's own voice: first person, past tense, the register of " +
        "somebody writing at the end of a long day rather than reciting a report. Two " +
        "to four short paragraphs, 200-350 words. No headings, no bullet lists, no " +
        "summary of statistics — the application already shows the numbers beside your " +
        "entry and repeating them wastes the reader's attention.\n\n" +
        "Every event you mention must come from the log. Do not invent quests, people, " +
        "places, kills, loot or outcomes, and do not imply an outcome the log does not " +
        "record. If the log says a boss was fought and lost to, it was lost to.\n\n" +
        "You may use what you know of Warcraft's world to give the facts their setting " +
        "— who a faction is, why a place matters, what a name means. That framing is " +
        "what makes this a journal rather than a list. Keep it in service of the " +
        "evening's own events; never let it become a lore essay the character was not " +
        "part of.\n\n" +
        "Quest text in the log is what the game itself put on the screen. Treat it as " +
        "the evening's source material and write from it.\n\n" +
        "The log has three kinds of dialogue and they are not worth the same. " +
        "\"Overheard\" is what the world said unbidden — NPCs talking to each other, a " +
        "boss mid-fight, an escort narrating itself — and it is the evening's " +
        "atmosphere; use it freely. \"Spoken to you\" is what an NPC said when the " +
        "character walked up and asked, and much of it is a shopkeeper's greeting or a " +
        "flight master's patter that means nothing; take the lines that carry the " +
        "evening and ignore the rest without remarking on them. A cutscene is listed " +
        "only as having happened, because its contents cannot be read — do not describe " +
        "one, though you may note that the character stood and watched something.\n\n" +
        "Some evenings are quiet. When the log is thin, write a short, honest entry " +
        "about a quiet evening. Do not inflate it, and do not apologise for it.\n\n" +
        "The title is a short phrase, at most sixty characters, that could head a diary " +
        "page. Not a summary sentence, and not the character's name.";

    /// <summary>Ask the server what it is running. The only way to put a name on an entry: the server ignores the request's model field entirely.</summary>
    public static Request Identify(string server) => Request.Get(Source, $"{server.TrimEnd('/')}/props");

    /// <summary>Read the server's properties for the model's name. Everything in that response is optional.</summary>
    public static string? ParseIdentity(byte[] body)
    {
        JsonNode? props;
        try
        {
            props = JsonNode.Parse(body);
        }
        catch (JsonException)
        {
            return null;
        }
        var alias = props.At("model_alias").Str();
        if (!string.IsNullOrEmpty(alias))
        {
            return alias;
        }
        var path = props.At("model_path").Str();
        if (string.IsNullOrEmpty(path))
        {
            return null;
        }
        var file = path.Split('/', '\\')[^1];
        var name = file.EndsWith(".gguf", StringComparison.Ordinal) ? file[..^5] : file;
        return name.Length == 0 ? null : name;
    }

    /// <summary>Ask for one entry. The shape is constrained by a JSON schema the sampler cannot leave, which is a stronger guarantee than a hosted API gives.</summary>
    public static Request Write(string server, Digest digest)
    {
        var body = new JsonObject
        {
            ["model"] = "armory-chronicle",
            ["max_tokens"] = MaxTokens,
            ["stream"] = false,
            ["temperature"] = 0.7,
            ["response_format"] = new JsonObject
            {
                ["type"] = "json_schema",
                ["json_schema"] = new JsonObject
                {
                    ["name"] = "journal_entry",
                    ["strict"] = true,
                    ["schema"] = new JsonObject
                    {
                        ["type"] = "object",
                        ["properties"] = new JsonObject
                        {
                            ["title"] = new JsonObject { ["type"] = "string", ["description"] = "A short diary-page heading, at most sixty characters." },
                            ["entry"] = new JsonObject { ["type"] = "string", ["description"] = "The journal entry itself, in the character's voice." },
                        },
                        ["required"] = new JsonArray("title", "entry"),
                        ["additionalProperties"] = false,
                    },
                },
            },
            ["messages"] = new JsonArray(
                new JsonObject { ["role"] = "system", ["content"] = Voice },
                new JsonObject { ["role"] = "user", ["content"] = Brief(digest) }),
        };
        return new Request
        {
            Source = Source,
            Method = Method.Post,
            Url = $"{server.TrimEnd('/')}/v1/chat/completions",
            Headers = [("content-type", "application/json")],
            Body = body.ToJsonString(),
        };
    }

    /// <summary>The evening, as the model is told about it. The whole input to the entry: everything the model is allowed to say has to be visible in it.</summary>
    public static string Brief(Digest digest)
    {
        var text = new StringBuilder();
        text.Append(CultureInfo.InvariantCulture, $"Character: {digest.DisplayName} of {digest.RealmName}, a {digest.Race.ToLowerInvariant()} {digest.Class.ToLowerInvariant()} of the {digest.Faction.Label()}.\n");
        var started = digest.StartedAt.UtcDateTime;
        text.Append(CultureInfo.InvariantCulture, $"Session: {started.ToString("dddd d MMMM yyyy", CultureInfo.InvariantCulture)} — {started.ToString("HH:mm", CultureInfo.InvariantCulture)} UTC, lasting {Prose.Spell(digest.Duration)}.\n");
        if (digest.EndLevel > digest.StartLevel)
        {
            text.Append(CultureInfo.InvariantCulture, $"Level {digest.StartLevel} at the start, {digest.EndLevel} at the end.\n");
        }
        else
        {
            text.Append(CultureInfo.InvariantCulture, $"Level {digest.EndLevel}.\n");
        }

        Section(text, "Where they went", digest.Route.Select(stop =>
        {
            var within = stop.Within.Count == 0 ? "" : $" (through {string.Join(", ", stop.Within)})";
            return $"{stop.Zone}{within}, about {Prose.Spell(TimeSpan.FromSeconds(stop.Stayed))}";
        }));
        Section(text, "Instances entered", digest.Instances.Select(instance => $"{instance.Name} ({instance.Kind})"));
        Section(text, "Mythic keystones", digest.Keystones.Select(key =>
            string.Create(CultureInfo.InvariantCulture, $"{key.Dungeon} at +{key.Level}, {(key.InTime ? "timed" : "over the timer")} in {Prose.Spell(TimeSpan.FromSeconds(key.Seconds))}{(key.Upgrades == 0 ? "" : $", key up {key.Upgrades}")}")));
        Section(text, "Scenarios and delves finished", digest.Scenarios);
        Section(text, "World tier the open world was set to (its difficulty)", digest.WorldTiers);
        Section(text, "Weather, as it changed", digest.Weather);

        // Before the quests, because it is the frame they hang in.
        if (digest.Campaigns.Count > 0)
        {
            text.Append("\nStorylines these quests belong to:\n");
            foreach (var (name, summary) in digest.Campaigns)
            {
                text.Append(CultureInfo.InvariantCulture, $"- {name}\n");
                if (summary is not null)
                {
                    text.Append(CultureInfo.InvariantCulture, $"    {summary}\n");
                }
            }
        }
        if (digest.Quests.Count > 0)
        {
            text.Append("\nQuests completed, in order. The quoted text is what the game showed on screen:\n");
            foreach (var quest in digest.Quests)
            {
                text.Append(CultureInfo.InvariantCulture, $"- \"{quest.Title}\"");
                if (quest.Money > 0)
                {
                    text.Append(CultureInfo.InvariantCulture, $" (paid {Prose.Money(quest.Money)})");
                }
                text.Append('\n');
                if (quest.Premise is not null)
                {
                    text.Append(CultureInfo.InvariantCulture, $"    asked for: {quest.Premise}\n");
                }
                if (quest.Story is not null)
                {
                    text.Append(CultureInfo.InvariantCulture, $"    on finishing: {quest.Story}\n");
                }
            }
        }

        Section(text, "Quests taken and not finished", digest.TakenUp);
        Section(text, "Levels gained", digest.Levels.Select(level => string.Create(CultureInfo.InvariantCulture, $"reached {level.Level} in {level.Zone}")));
        Section(text, "Bosses defeated", digest.Felled);
        Section(text, "Fought and lost to (no kill followed)", digest.LostTo);
        Section(text, "Rares and world bosses killed", digest.Rares);
        Section(text, "Deaths", digest.Deaths.Select(death =>
        {
            var line = $"died in {death.Zone}";
            if (death.Subzone is not null)
            {
                line += $" ({death.Subzone})";
            }
            // Named only where the combat log caught it.
            if (death.To is not null)
            {
                line += $", killed by {death.To}";
            }
            return line;
        }));
        Section(text, "Achievements earned", digest.Achievements.Select(achievement => achievement.Name));
        Section(text, "Added to the collection", digest.Acquired.Select(got => $"{got.Name} (a {got.Kind.Label()})"));
        Section(text, "Notable loot", digest.Loot.Select(loot => $"{loot.Name} ({QualityName(loot.Quality)})"));
        Section(text, "Sold at auction", digest.Sales.Select(sale => $"{sale.Subject} — {Prose.Money(sale.Money)}"));
        Section(text, "Reputation ranks reached", digest.Risen.Select(risen => $"{risen.Faction} — now {Prose.Standing(risen.Rank)}"));
        Section(text, "Gear upgraded", digest.Equipped.Take(5).Select(gear => gear.From is { } source
            ? string.Create(CultureInfo.InvariantCulture, $"{gear.Name} (item level {gear.ItemLevel}, up {gear.Gained}, off {source})")
            : string.Create(CultureInfo.InvariantCulture, $"{gear.Name} (item level {gear.ItemLevel}, up {gear.Gained})")));
        Section(text, "Professions improved", digest.Practised.Select(skill => string.Create(CultureInfo.InvariantCulture, $"{skill.Profession} now at {skill.Skill}")));
        Section(text, "New appearances collected", digest.Appearances);
        Section(text, "Overheard", digest.Overheard.Select(said => said.Who.Length == 0 ? said.Line : $"{said.Who}: \"{said.Line}\""));
        Section(text, "Cutscenes that played (contents unknown — the dialogue, if any, is under Overheard)", digest.Cutscenes.Select(cutscene => $"one in {cutscene.Zone}"));
        Section(text, "Spoken to you (much of this is functional — use only what carries the evening)", digest.Told.Select(told => told.Who.Length == 0 ? told.Line : $"{told.Who}: \"{told.Line}\""));
        Section(text, "Who sent you out", digest.Questgivers.Select(giver => giver.Given == 1 ? giver.Who : string.Create(CultureInfo.InvariantCulture, $"{giver.Who} ({giver.Given} quests)")));
        Section(text, "Recipes learned", digest.Learned);
        Section(text, "In the party", digest.Companions);
        Section(text, "Money in", digest.Income.Select(entry => $"{entry.Purpose.Label(true)}: {Prose.Money(entry.Amount)}"));
        Section(text, "Money out", digest.Spending.Select(entry => $"{entry.Purpose.Label(false)}: {Prose.Money(entry.Amount)}"));
        Section(text, "Made at the workbench", digest.Crafted.Select(made => made.Made == 1 ? made.Name : string.Create(CultureInfo.InvariantCulture, $"{made.Name} ×{made.Made}")));

        if (digest.LongestFight >= 60)
        {
            text.Append(CultureInfo.InvariantCulture, $"\nThe longest single fight lasted {Counters.Spent(digest.LongestFight)}.\n");
        }
        if (digest.Flights > 0 || digest.Travelled > 0)
        {
            text.Append(CultureInfo.InvariantCulture, $"\nGround covered: {Counters.Far(digest.Travelled)}, {Prose.Plural(digest.Flights, "flight taken", "flights taken")}.\n");
        }
        text.Append(CultureInfo.InvariantCulture, $"\nPurse: {Prose.Purse(digest.Purse)} over the session.");
        if (digest.QuestIncome > 0)
        {
            text.Append(CultureInfo.InvariantCulture, $" Quests paid {Prose.Money(digest.QuestIncome)}.");
        }
        if (digest.SaleIncome > 0)
        {
            text.Append(CultureInfo.InvariantCulture, $" Auctions paid {Prose.Money(digest.SaleIncome)}.");
        }
        text.Append('\n');
        return text.ToString();
    }

    /// <summary>A heading and its lines, or nothing at all. An empty heading invites the model to fill it in.</summary>
    private static void Section(StringBuilder text, string heading, IEnumerable<string> lines)
    {
        var list = lines.ToList();
        if (list.Count == 0)
        {
            return;
        }
        text.Append(CultureInfo.InvariantCulture, $"\n{heading}:\n");
        foreach (var line in list)
        {
            text.Append(CultureInfo.InvariantCulture, $"- {line}\n");
        }
    }

    private static string QualityName(int quality) => quality switch
    {
        5 => "legendary",
        4 => "epic",
        3 => "rare",
        _ => "uncommon",
    };

    /// <summary>Read the response into an entry: OpenAI's shape, which llama.cpp implements.</summary>
    public static Outcome<Written> ParseWritten(byte[] body)
    {
        var parsed = Outcomes.ParseJson<Written>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        var value = parsed.Value;
        var choice = value.At("choices").Items().FirstOrDefault();
        if (choice is null)
        {
            return new Outcome<Written>.Stale(new Reason.Malformed("the reply carried no choices"));
        }
        // Checked before the content: an entry cut off mid-JSON cannot be salvaged.
        if (choice.At("finish_reason").Str() == "length")
        {
            return new Outcome<Written>.Stale(new Reason.Malformed("the entry ran past the token budget before it finished"));
        }
        if (choice.At("message").At("content").Str() is not { } content)
        {
            return new Outcome<Written>.Stale(new Reason.Malformed("the reply carried no message"));
        }
        JsonNode? entry;
        try
        {
            entry = JsonNode.Parse(StripThinking(content).Trim());
        }
        catch (JsonException)
        {
            entry = null;
        }
        if (entry is null)
        {
            return new Outcome<Written>.Stale(new Reason.Malformed("the reply was not the shape that was asked for"));
        }
        if (entry.At("title").Str() is not { } title || entry.At("entry").Str() is not { } prose)
        {
            return new Outcome<Written>.Stale(new Reason.Malformed("the reply had no title and entry in it"));
        }
        if (prose.Trim().Length == 0)
        {
            return new Outcome<Written>.Empty();
        }
        var model = value.At("model").Str();
        return new Outcome<Written>.Found(new Written(
            title.Trim(),
            prose.Trim(),
            string.IsNullOrEmpty(model) || model == "armory-chronicle" ? "a local model" : model));
    }

    /// <summary>Cut a think block off the front of a reply. Plenty of GGUFs emit the tags inline, and the JSON parser refuses the lot.</summary>
    internal static string StripThinking(string content)
    {
        const string closing = "</think>";
        var end = content.IndexOf(closing, StringComparison.Ordinal);
        return end < 0 ? content : content[(end + closing.Length)..];
    }

    /// <summary>Read the error message out of a refused request, because "HTTP 400" sends somebody looking for a bug in Armory.</summary>
    public static string? ParseError(byte[] body)
    {
        try
        {
            var value = JsonNode.Parse(body);
            var error = value.At("error");
            return error.At("message").Str() ?? error.Str();
        }
        catch (JsonException)
        {
            return null;
        }
    }

    // -- Claude Code -------------------------------------------------------------
    //
    // The other way to get an entry written: Anthropic's own command-line,
    // signed in to this person's own subscription, run in its non-interactive
    // mode with the brief on stdin and JSON on stdout. No key is held here and
    // nothing is billed per entry; the CLI keeps the login and Armory only
    // asks whether there is one. The brief and the voice are the same as the
    // llama-server's, so what the model is told does not depend on which is
    // asked.

    /// <summary>The command-line, found on PATH. Anthropic's installer puts it there.</summary>
    public const string ClaudeProgram = "claude";

    /// <summary>The shape asked for. The same schema the llama-server gets, and the CLI validates the reply against it.</summary>
    private static readonly string EntrySchema = new JsonObject
    {
        ["type"] = "object",
        ["properties"] = new JsonObject
        {
            ["title"] = new JsonObject { ["type"] = "string", ["description"] = "A short diary-page heading, at most sixty characters." },
            ["entry"] = new JsonObject { ["type"] = "string", ["description"] = "The journal entry itself, in the character's voice." },
        },
        ["required"] = new JsonArray("title", "entry"),
        ["additionalProperties"] = false,
    }.ToJsonString();

    /// <summary>
    /// Ask the CLI whether anybody is signed in. The readiness check, and
    /// the only credential question Armory asks: the answer names the plan
    /// and the account, never the token.
    /// </summary>
    public static Command AuthStatus() => new(ClaudeProgram, ["auth", "status"], "");

    /// <summary>Read the sign-in status for who is signed in, or nothing when nobody is.</summary>
    public static string? ParseAuthStatus(byte[] stdout)
    {
        JsonNode? status;
        try
        {
            status = JsonNode.Parse(stdout);
        }
        catch (JsonException)
        {
            return null;
        }
        if (status.At("loggedIn").Flag() != true)
        {
            return null;
        }
        var plan = status.At("subscriptionType").Str();
        var who = status.At("email").Str();
        var parts = new List<string> { "Claude Code" };
        if (!string.IsNullOrEmpty(plan))
        {
            parts.Add($"{char.ToUpperInvariant(plan[0])}{plan[1..]} plan");
        }
        if (!string.IsNullOrEmpty(who))
        {
            parts.Add(who);
        }
        return string.Join(" · ", parts);
    }

    /// <summary>
    /// Ask the CLI for one entry. Print mode, every tool switched off, no
    /// session kept, no settings read, the voice as the whole system prompt
    /// and the brief on stdin. Not <c>--bare</c>: bare mode skips the keychain
    /// and so never finds the login this whole path exists to use.
    /// </summary>
    public static Command Compose(string model, Digest digest) => new(
        ClaudeProgram,
        [
            "-p",
            "--output-format", "json",
            "--json-schema", EntrySchema,
            "--tools", "",
            "--no-session-persistence",
            "--setting-sources", "",
            "--model", model.Trim().Length == 0 ? Settings.Settings.DefaultJournalModel : model.Trim(),
            "--system-prompt", Voice,
        ],
        Brief(digest));

    /// <summary>
    /// Read the CLI's result object into an entry. The entry is in
    /// <c>structured_output</c> when the schema was honoured, and
    /// <c>is_error</c> with the reason in <c>result</c> when the run failed
    /// before a model was asked, which is what "Not logged in" looks like.
    /// </summary>
    public static Outcome<Written> ParseComposed(byte[] stdout)
    {
        var parsed = Outcomes.ParseJson<Written>(Source, stdout);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        var value = parsed.Value;
        if (value.At("is_error").Flag() == true)
        {
            var said = value.At("result").Str()?.Trim();
            return new Outcome<Written>.Unusable(string.IsNullOrEmpty(said)
                ? new Reason.Declined("Claude Code reported an error and did not say what")
                : new Reason.Declined(said));
        }
        var entry = value.At("structured_output");
        if (entry is null && value.At("result").Str() is { } result)
        {
            // The text result carries the same JSON when the schema was
            // honoured; without a structured field it is the fallback, not
            // the answer to trust first.
            try
            {
                entry = JsonNode.Parse(result.Trim());
            }
            catch (JsonException)
            {
                entry = null;
            }
        }
        if (entry.At("title").Str() is not { } title || entry.At("entry").Str() is not { } prose)
        {
            return new Outcome<Written>.Stale(new Reason.Malformed("the reply was not the shape that was asked for"));
        }
        if (prose.Trim().Length == 0)
        {
            return new Outcome<Written>.Empty();
        }
        return new Outcome<Written>.Found(new Written(title.Trim(), prose.Trim(), WhoWrote(value)));
    }

    /// <summary>
    /// The model that wrote the entry: the one that produced the most of the
    /// reply. The CLI spends a small helper model on housekeeping beside the
    /// one asked for, and naming that on the entry would be wrong.
    /// </summary>
    private static string WhoWrote(JsonNode value)
    {
        string? best = null;
        long most = -1;
        if (value.At("modelUsage") is JsonObject models)
        {
            foreach (var (name, usage) in models)
            {
                var output = usage.At("outputTokens").Int() ?? 0;
                if (output > most)
                {
                    most = output;
                    best = usage.At("canonicalModel").Str() ?? name;
                }
            }
        }
        return string.IsNullOrEmpty(best) ? "Claude Code" : best;
    }
}
