using System.Globalization;
using System.Text;
using Armory.Blizzard;
using Armory.Collections;
using Armory.Market;
using Armory.Provenance;
using Armory.Roster;
using Armory.Run;
using Armory.Tally;

namespace Armory.Addon;

/// <summary>Which World of Warcraft this file came out of.</summary>
/// <remarks>
/// The same addon runs on the retail client, on Classic Era and on whatever
/// Forever turns out to be, and a character called Aeltor on Whitemane can
/// exist on two of them at once. The project id is the client's own word for
/// which it is; the version is there because Blizzard mints a new project id
/// for every Classic flavour.
/// </remarks>
public sealed record Client
{
    /// <summary><c>WOW_PROJECT_ID</c>: 1 is the retail client, 2 is Classic Era.</summary>
    public long Project { get; init; }

    public string Version { get; init; } = "";

    public long Build { get; init; }

    /// <summary>The interface number the <c>.toc</c> is matched against.</summary>
    public long Interface { get; init; }

    /// <summary>Whether this is the retail client, the only one the web API and every retail-only system describe.</summary>
    public bool IsMainline => Project == 1;
}

/// <summary>What one dump of the addon's account-wide data contains.</summary>
/// <remarks>
/// Two files. The account-wide one carries attribution, criteria,
/// collections, currencies and the Warband bank; the per-character one is one
/// character. Between them these are enough for Armory to work with no web
/// API at all, which matters more than it sounds: Blizzard's developer portal
/// has been answering 500 to client creation since late 2025.
/// </remarks>
public sealed record Collected
{
    /// <summary>Achievement id to the character who earned it.</summary>
    public Dictionary<long, CharacterKey> EarnedBy { get; init; } = [];

    /// <summary>Achievement id to when the account finished it.</summary>
    public Dictionary<long, DateTimeOffset> Completed { get; init; } = [];

    /// <summary>Achievement id to its criteria ids, flat, which is the shape the game hands over.</summary>
    public Dictionary<long, List<long>> Tree { get; init; } = [];

    /// <summary>Criterion id to what it measures.</summary>
    public Dictionary<long, CriterionKind> Criteria { get; init; } = [];

    /// <summary>The achievement catalogue: names, points, categories, descriptions. The game already knows, so this is cheaper than the API and available when it is not.</summary>
    public Dictionary<long, Achievement> Catalogue { get; init; } = [];

    /// <summary>Currency id to amount, per character.</summary>
    public Dictionary<CharacterKey, Dictionary<long, long>> Currencies { get; init; } = [];

    /// <summary>Item id to count, across the Warband bank.</summary>
    public Dictionary<long, long> WarbandBank { get; init; } = [];

    /// <summary>Mounts, pets, toys and decor: everything that exists.</summary>
    public List<Collectible> Collectibles { get; init; } = [];

    /// <summary>What of it the account has.</summary>
    public HashSet<(Kind Kind, long Id)> Owned { get; init; } = [];

    /// <summary>What each character has personally been observed earning. No endpoint attributes a point of reputation to a character.</summary>
    public Dictionary<CharacterKey, Earned> Earned { get; init; } = [];

    /// <summary>What each character can make. A character missing from here has not opened their profession window, which is silence.</summary>
    public RecipeBooks Recipes { get; init; } = [];

    /// <summary>Counters no Blizzard system keeps, per character.</summary>
    public Tallies Tallies { get; init; } = [];

    /// <summary>Pet species to how many of it the journal holds. Account state, like the owned set.</summary>
    public Dictionary<long, long> PetsHeld { get; init; } = [];

    /// <summary>When the addon last wrote, as it saw the clock.</summary>
    public DateTimeOffset? WrittenAt { get; init; }

    /// <summary>Which game client wrote this. Absent from files older than the field.</summary>
    public Client? Client { get; init; }

    /// <summary>
    /// Rebuild the achievement list a run is planned from. This is what lets
    /// planning run with no web API. The tree is one level deep, a root with
    /// a child per criterion, which is the shape the evaluator wants and all
    /// the game gives.
    /// </summary>
    public List<AchievementProgress> Progress()
    {
        var ids = Tree.Keys.Concat(Completed.Keys).Distinct().OrderBy(id => id).ToList();
        return ids.Select(id => new AchievementProgress
        {
            Id = id,
            CompletedAt = Completed.TryGetValue(id, out var at) ? at : null,
            Criteria = Tree.TryGetValue(id, out var children)
                ? new Criterion
                {
                    Id = id,
                    Kind = CriterionKind.Unknown,
                    // Zero means "all of them". The game does not expose the
                    // "any N of these" threshold, so demanding all is the
                    // conservative reading: it can leave a goal open that is
                    // really done, never the reverse.
                    Required = 0,
                    Children = children
                        .Select(criterion => Criterion.Leaf(criterion, Criteria.GetValueOrDefault(criterion, CriterionKind.Unknown), 1))
                        .ToList(),
                }
                : null,
        }).ToList();
    }
}

/// <summary>One character, as its own file describes it.</summary>
public sealed record CollectedCharacter
{
    public required Character Character { get; init; }

    public Detail Detail { get; init; } = new();

    public HashSet<long> Quests { get; init; } = [];

    /// <summary>Which game client this character lives on.</summary>
    public Client? Client { get; init; }

    /// <summary>What this character's own data can answer.</summary>
    public PrimaryData Primary() => new() { Quests = [.. Quests] };
}

/// <summary>Why a collector file could not be used.</summary>
public abstract record ReadError
{
    /// <summary>The file is not Lua we will read.</summary>
    public sealed record Unparsable(string Detail) : ReadError
    {
        public override string ToString() => Detail;
    }

    /// <summary>Parsed, but it is not the collector's file.</summary>
    public sealed record NotCollectorData : ReadError
    {
        public override string ToString() => "this file was not written by the Armory collector";
    }

    /// <summary>
    /// Written by an addon newer than this application understands. Distinct
    /// from unparsable on purpose: the fix is to update Armory, and saying
    /// "unreadable" would send someone reinstalling the addon that is working.
    /// </summary>
    public sealed record FromTheFuture(long Format) : ReadError
    {
        public override string ToString() =>
            string.Create(CultureInfo.InvariantCulture, $"the collector addon is writing format {Format} and this version of Armory reads {Collector.Format} — update Armory");
    }
}

/// <summary>
/// Reading what the collector addon wrote. The shape is fixed by
/// <c>Armory_Collector.lua</c>, which is in this repository: both halves of
/// this format are ours, so it can be narrow and explicit.
/// </summary>
public static class Collector
{
    /// <summary>The account-wide global the addon declares.</summary>
    private const string Global = "ArmoryCollectorDB";

    /// <summary>The per-character one.</summary>
    private const string CharacterGlobal = "ArmoryCollectorCharDB";

    /// <summary>The format this reader understands. Bumped when the addon's shape changes.</summary>
    public const long Format = 5;

    private static readonly (string Key, Kind Kind)[] Collections =
    [
        ("mounts", Kind.Mount),
        ("pets", Kind.Pet),
        ("toys", Kind.Toy),
        // Filed under the catalogue's record id; the item goes in the link
        // column, because Wowhead indexes decor by the item.
        ("decor", Kind.Decor),
    ];

    /// <summary>Read the account-wide collector file.</summary>
    public static Result<Collected, ReadError> Read(string source) => Read(Encoding.UTF8.GetBytes(source));

    public static Result<Collected, ReadError> Read(byte[] source)
    {
        var parsed = Lua.Parse(source);
        if (!parsed.IsOk)
        {
            return Result<Collected, ReadError>.Err(new ReadError.Unparsable(parsed.Error.ToString()));
        }
        if (!parsed.Value.TryGetValue(Global, out var db))
        {
            return Result<Collected, ReadError>.Err(new ReadError.NotCollectorData());
        }
        var format = db.Get("format")?.AsInteger() ?? 0;
        if (format > Format)
        {
            return Result<Collected, ReadError>.Err(new ReadError.FromTheFuture(format));
        }

        var collected = new Collected
        {
            WrittenAt = Epoch(db.Get("writtenAt")?.AsDouble()),
            Client = ReadClient(db),
        };

        if (db.Get("achievements") is { } achievements)
        {
            foreach (var (id, who) in achievements.Entries())
            {
                if (id.AsInteger() is { } achievement && who.AsStr() is { } text && ParseCharacter(text) is { } key)
                {
                    collected.EarnedBy[achievement] = key;
                }
            }
        }

        if (db.Get("completed") is { } completed)
        {
            foreach (var (id, at) in completed.Entries())
            {
                if (id.AsInteger() is not { } achievement)
                {
                    continue;
                }
                // `true` rather than a date means the game gave no date. The
                // achievement is still complete, and standing needs a time:
                // the epoch is before any baseline and reads as "earned long
                // ago", which is what it means.
                DateTimeOffset? when = at switch
                {
                    LuaValue.Number seconds => Epoch(seconds.Value),
                    LuaValue.Bool { Flag: true } => DateTimeOffset.UnixEpoch,
                    _ => null,
                };
                if (when is { } stamp)
                {
                    collected.Completed[achievement] = stamp;
                }
            }
        }

        if (db.Get("tree") is { } tree)
        {
            foreach (var (id, children) in tree.Entries())
            {
                if (id.AsInteger() is not { } achievement)
                {
                    continue;
                }
                var ids = children.Items().Select(child => child.AsInteger()).OfType<long>().ToList();
                if (ids.Count > 0)
                {
                    collected.Tree[achievement] = ids;
                }
            }
        }

        if (db.Get("criteria") is { } criteria)
        {
            foreach (var (id, pair) in criteria.Entries())
            {
                // `{criteriaType, assetID}`, a positional pair. A row that is
                // not that shape is skipped rather than half-read.
                if (id.AsInteger() is not { } criterion || pair.Items() is not [var kind, var asset])
                {
                    continue;
                }
                if (kind.AsInteger() is { } type && asset.AsInteger() is { } target)
                {
                    collected.Criteria[criterion] = CriterionKind.FromCatalogue(type, target);
                }
            }
        }

        if (db.Get("names") is { } names)
        {
            foreach (var (id, row) in names.Entries())
            {
                if (id.AsInteger() is not { } achievement)
                {
                    continue;
                }
                var columns = row.Items();
                if (columns.ElementAtOrDefault(0)?.AsStr() is not { } name)
                {
                    continue;
                }
                var category = columns.ElementAtOrDefault(2)?.AsStr() ?? "";
                collected.Catalogue[achievement] = new Achievement
                {
                    Id = achievement,
                    Name = StripMarkup(name),
                    Category = category,
                    Points = columns.ElementAtOrDefault(1)?.AsInteger() ?? 0,
                    Description = StripMarkup(columns.ElementAtOrDefault(3)?.AsStr() ?? ""),
                    // A Feat of Strength cannot be earned twice by anybody, so
                    // it leaves a run rather than sitting in it as a permanent
                    // zero. The category is the only thing that says so.
                    IsUnrepeatable = category.Contains("Feats of Strength", StringComparison.Ordinal) || category.Contains("Legacy", StringComparison.Ordinal),
                };
            }
        }

        if (db.Get("currencies") is { } currencies)
        {
            foreach (var (who, amounts) in currencies.Entries())
            {
                if (ParseCharacter(who.AsStr()) is not { } key)
                {
                    continue;
                }
                var perCharacter = new Dictionary<long, long>();
                foreach (var (id, amount) in amounts.Entries())
                {
                    if (id.AsInteger() is { } currency)
                    {
                        perCharacter[currency] = (long)(amount.AsDouble() ?? 0);
                    }
                }
                collected.Currencies[key] = perCharacter;
            }
        }

        if (db.Get("earned") is { } earnedTable)
        {
            foreach (var (who, mine) in earnedTable.Entries())
            {
                if (ParseCharacter(who.AsStr()) is not { } key)
                {
                    continue;
                }
                var earned = new Earned();
                if (mine.Get("rep") is { } factions)
                {
                    foreach (var (id, row) in factions.Entries())
                    {
                        // `{ points, renownEarned, renownSeen, accountWide }`.
                        if (id.AsInteger() is not { } faction || row.Items() is not [var points, var renown, var seen, var wide, ..])
                        {
                            continue;
                        }
                        earned.Reputation[faction] = new EarnedReputation
                        {
                            Points = points.AsInteger() ?? 0,
                            Renown = renown.AsInteger() ?? 0,
                            RenownSeen = seen.AsInteger() ?? 0,
                            AccountWide = wide.AsInteger() == 1,
                        };
                    }
                }
                if (mine.Get("currency") is { } gained)
                {
                    foreach (var (id, row) in gained.Entries())
                    {
                        // `{ gained, earned, accountWide, transferable, tracksEarned }`.
                        if (id.AsInteger() is not { } currency || row.Items() is not [var got, var counted, var wide, var transferable, var tracks, ..])
                        {
                            continue;
                        }
                        earned.Currency[currency] = new EarnedCurrency
                        {
                            Gained = (long)(got.AsDouble() ?? 0),
                            Earned = (long)(counted.AsDouble() ?? 0),
                            // Read rather than inferred. The game returns a
                            // flat zero for currencies it does not track.
                            TracksEarned = tracks.AsInteger() == 1,
                            AccountWide = wide.AsInteger() == 1,
                            Transferable = transferable.AsInteger() == 1,
                        };
                    }
                }
                collected.Earned[key] = earned;
            }
        }

        // `recipes[character][recipeID] = { name, output, makes, { { qty, tiers } } }`.
        if (db.Get("recipes") is { } recipes)
        {
            foreach (var (who, mine) in recipes.Entries())
            {
                if (ParseCharacter(who.AsStr()) is not { } character)
                {
                    continue;
                }
                var book = new List<Recipe>();
                foreach (var (id, row) in mine.Entries())
                {
                    if (id.AsInteger() is not { } recipe || row.Items() is not [var name, var output, var makes, var reagents, ..])
                    {
                        continue;
                    }
                    if (name.AsStr() is not { } recipeName || output.AsInteger() is not { } outputId)
                    {
                        continue;
                    }
                    var slots = new List<Reagent>();
                    foreach (var slot in reagents.Items())
                    {
                        if (slot.Items() is not [var quantity, var tiers, ..])
                        {
                            continue;
                        }
                        var tierIds = tiers.Items().Select(tier => tier.AsInteger()).OfType<long>().ToList();
                        if (tierIds.Count > 0)
                        {
                            slots.Add(new Reagent { Quantity = Math.Max(quantity.AsInteger() ?? 1, 1), Tiers = tierIds });
                        }
                    }
                    if (slots.Count == 0)
                    {
                        continue;
                    }
                    book.Add(new Recipe
                    {
                        Id = recipe,
                        Name = recipeName,
                        Output = outputId,
                        // A recipe that makes none of something is not a
                        // recipe; one is the floor and the addon's default.
                        Makes = Math.Max(makes.AsInteger() ?? 1, 1),
                        Reagents = slots,
                    });
                }
                if (book.Count > 0)
                {
                    collected.Recipes[character] = book;
                }
            }
        }

        // `tally[character][kind][key] = { count, label }`. One table for every counter.
        if (db.Get("tally") is { } tally)
        {
            foreach (var (who, mine) in tally.Entries())
            {
                if (ParseCharacter(who.AsStr()) is not { } character)
                {
                    continue;
                }
                var counted = new List<Tally.Tally>();
                foreach (var (token, rows) in mine.Entries())
                {
                    // A kind this version does not know is skipped rather than
                    // filed under a plausible one.
                    if (CountingExtensions.FromToken(token.AsStr()) is not { } kind)
                    {
                        continue;
                    }
                    foreach (var (key, row) in rows.Entries())
                    {
                        if (row.Items() is not [var count, var label, ..] || label.AsStr() is not { } text)
                        {
                            continue;
                        }
                        counted.Add(new Tally.Tally { Kind = kind, Key = key.AsStr(), Label = text, Count = (long)(count.AsDouble() ?? 0) });
                    }
                }
                if (counted.Count > 0)
                {
                    collected.Tallies[character] = counted;
                }
            }
        }

        if (db.Get("warbandBank") is { } bank)
        {
            foreach (var (id, count) in bank.Entries())
            {
                if (id.AsInteger() is { } item)
                {
                    collected.WarbandBank[item] = (long)(count.AsDouble() ?? 0);
                }
            }
        }

        foreach (var (key, kind) in Collections)
        {
            if (db.Get(key) is not { } table)
            {
                continue;
            }
            // Both halves of the table, because WoW's serializer uses both. A
            // collection keyed by mount id comes out positional while the ids
            // stay dense, padded with `nil` for the holes, and switches to
            // keyed entries once they are not. Lua arrays are one-based, so
            // the index is the id.
            var positional = table.Items().Select((entry, index) => ((long)index + 1, entry));
            var keyed = table.Entries()
                .Where(entry => entry.Key.AsInteger() is not null)
                .Select(entry => (entry.Key.AsInteger()!.Value, entry.Value));
            foreach (var (id, entry) in positional.Concat(keyed))
            {
                var row = entry.Items();
                if (row.Count < 3 || row[0].AsStr() is not { } title)
                {
                    // `nil` padding, and anything else that is not a row.
                    continue;
                }
                if (row[1].AsInteger() == 1)
                {
                    collected.Owned.Add((kind, id));
                }
                var sourceText = StripMarkup(row[2].AsStr() ?? "");
                var flavour = row.ElementAtOrDefault(5)?.AsStr() is { } said ? StripMarkup(said) : "";
                var icon = row.ElementAtOrDefault(6)?.AsInteger();
                var display = row.ElementAtOrDefault(7)?.AsInteger();
                var link = row.ElementAtOrDefault(4)?.AsInteger();
                collected.Collectibles.Add(new Collectible
                {
                    Kind = kind,
                    Id = id,
                    Name = title,
                    // A sentence rather than the web API's one word. This is
                    // the whole reason the addon is the better source here.
                    Source = Sources.FromText(sourceText),
                    Description = sourceText.Length > 0 ? sourceText : null,
                    Flavour = flavour.Length > 0 ? flavour : null,
                    Icon = icon > 0 ? icon : null,
                    Display = display > 0 ? display : null,
                    // `-1` is the addon saying "anyone", which is not a faction.
                    Faction = row.ElementAtOrDefault(9)?.AsDouble() switch
                    {
                        0.0 => Roster.Faction.Horde,
                        1.0 => Roster.Faction.Alliance,
                        _ => null,
                    },
                    // Whichever id Wowhead indexes this kind under, falling
                    // back to the collection id.
                    LinkId = link > 0 ? link.Value : id,
                    // Pets only, and only from collectors new enough to write
                    // it. An older file has no tenth column, which is silence.
                    Tradeable = kind == Kind.Pet && row.ElementAtOrDefault(10)?.AsInteger() is { } flag ? flag == 1 : null,
                });
                if (kind == Kind.Pet && row.ElementAtOrDefault(11)?.AsInteger() is { } held)
                {
                    collected.PetsHeld[id] = held;
                }
            }
        }

        return Result<Collected, ReadError>.Ok(collected);
    }

    /// <summary>Read one per-character collector file.</summary>
    public static Result<CollectedCharacter, ReadError> ReadCharacter(string source) => ReadCharacter(Encoding.UTF8.GetBytes(source));

    public static Result<CollectedCharacter, ReadError> ReadCharacter(byte[] source)
    {
        var parsed = Lua.Parse(source);
        if (!parsed.IsOk)
        {
            return Result<CollectedCharacter, ReadError>.Err(new ReadError.Unparsable(parsed.Error.ToString()));
        }
        if (!parsed.Value.TryGetValue(CharacterGlobal, out var db))
        {
            return Result<CollectedCharacter, ReadError>.Err(new ReadError.NotCollectorData());
        }
        var format = db.Get("format")?.AsInteger() ?? 0;
        if (format > Format)
        {
            return Result<CollectedCharacter, ReadError>.Err(new ReadError.FromTheFuture(format));
        }
        if (db.Get("name")?.AsStr() is not { } name || db.Get("realm")?.AsStr() is not { } realm)
        {
            return Result<CollectedCharacter, ReadError>.Err(new ReadError.NotCollectorData());
        }

        var character = new Character
        {
            Key = new CharacterKey(Slug.RealmSlug(realm), name),
            // The game does not know its own numeric ids. Only the protected
            // endpoint wants them, and that is a call this path does without.
            Id = 0,
            RealmId = 0,
            DisplayName = name,
            RealmName = realm,
            Level = (int)(db.Get("level")?.AsInteger() ?? 0),
            Class = Titlecase(db.Get("class")?.AsStr() ?? ""),
            Race = Titlecase(db.Get("race")?.AsStr() ?? ""),
            Faction = db.Get("faction")?.AsStr() switch
            {
                "Alliance" => Roster.Faction.Alliance,
                "Horde" => Roster.Faction.Horde,
                _ => Roster.Faction.Neutral,
            },
            WowAccountId = 0,
        };

        var professions = new List<Profession>();
        foreach (var entry in db.Get("professions")?.Items() ?? [])
        {
            var columns = entry.Items();
            if (columns is not [var pname, var rank, var max, var primary, ..] || pname.AsStr() is not { } professionName)
            {
                continue;
            }
            // The last two are newer than the first four, and an older addon
            // writes a four-element row. Absent is silence rather than "no
            // specialisations".
            var trees = columns.Count >= 6 ? columns[4] : null;
            var learned = columns.Count >= 6 ? columns[5].AsInteger() : null;
            professions.Add(new Profession
            {
                Name = professionName,
                Tier = null,
                Skill = (int?)rank.AsInteger(),
                MaxSkill = (int?)max.AsInteger(),
                IsPrimary = primary.AsInteger() == 1,
                Specialisations = trees?.Items()
                    .Select(tree => tree.Items() is [var treeName, var open, ..] && treeName.AsStr() is { } named ? new Specialisation(named, open.AsInteger() == 1) : null)
                    .OfType<Specialisation>()
                    .ToList() ?? [],
                Knowledge = learned ?? 0,
            });
        }

        var detail = new Detail
        {
            ItemLevel = db.Get("itemLevel")?.AsDouble() is { } level ? (int)level : null,
            EquippedItemLevel = null,
            Spec = db.Get("spec")?.AsStr(),
            Guild = db.Get("guild")?.AsStr(),
            Money = db.Get("money")?.AsDouble() is { } money ? (long)money : null,
            AchievementPoints = null,
            LastLogin = Epoch(db.Get("scannedAt")?.AsDouble()),
            Professions = professions,
            MythicRating = null,
            Renown = null,
            Equipment = ReadEquipment(db),
            // Lifetime raid progress is the web API's answer and the client
            // cannot give it. What the client knows is the current lockout.
            Raids = null,
            RaidLocks = ReadRaidLocks(db),
            Vault = ReadVault(db),
            VaultReady = db.Get("vaultReady")?.AsInteger() == 1,
        };

        return Result<CollectedCharacter, ReadError>.Ok(new CollectedCharacter
        {
            Character = character,
            Detail = detail,
            Client = ReadClient(db),
            Quests = db.Get("quests")?.Items().Select(id => id.AsInteger()).OfType<long>().ToHashSet() ?? [],
        });
    }

    /// <summary><c>{ project, version, build, interface }</c>, as the addon writes it.</summary>
    private static Client? ReadClient(LuaValue db)
    {
        if (db.Get("flavour")?.Items() is not [var project, var version, var build, var iface, ..] || project.AsInteger() is not { } projectId)
        {
            return null;
        }
        return new Client
        {
            Project = projectId,
            Version = version.AsStr() ?? "",
            Build = build.AsInteger() ?? 0,
            Interface = iface.AsInteger() ?? 0,
        };
    }

    /// <summary>
    /// Strip WoW's UI markup out of a string. The journals' source text is
    /// written for a tooltip: colour codes and line breaks woven through one
    /// mount's provenance. The sequences are all introduced by a pipe:
    /// <c>cAARRGGBB</c> opens a colour and <c>r</c> closes it, <c>n</c> is a
    /// newline, <c>T…|t</c> is an inline texture, <c>H…|h text |h</c> is a
    /// hyperlink whose text is worth keeping, and <c>||</c> is a literal pipe.
    /// </summary>
    public static string StripMarkup(string text)
    {
        var builder = new StringBuilder(text.Length);
        var i = 0;
        while (i < text.Length)
        {
            var c = text[i++];
            if (c != '|')
            {
                builder.Append(c);
                continue;
            }
            if (i >= text.Length)
            {
                break;
            }
            var escape = text[i++];
            switch (escape)
            {
                case 'c':
                    // A colour opens with eight hex digits that are not content.
                    for (var n = 0; n < 8 && i < text.Length && char.IsAsciiHexDigit(text[i]); n++)
                    {
                        i++;
                    }
                    break;
                case 'r':
                case 'h':
                    break;
                case 'n':
                    builder.Append('\n');
                    break;
                case 'T':
                    // A texture runs to its closing `|t` and has no text in it.
                    while (i < text.Length)
                    {
                        if (text[i++] == '|' && i < text.Length && text[i] == 't')
                        {
                            i++;
                            break;
                        }
                    }
                    break;
                case 'H':
                    // A hyperlink's payload is `|Hlink|htext|h`: skip to the
                    // first `|h`, keep what follows, stop at the second.
                    while (i < text.Length)
                    {
                        if (text[i++] == '|' && i < text.Length && text[i] == 'h')
                        {
                            i++;
                            break;
                        }
                    }
                    break;
                case '|':
                    builder.Append('|');
                    break;
                default:
                    // An escape we do not know: drop the pipe, keep the
                    // character, so a future addition degrades to slightly
                    // odd text rather than to markup on screen.
                    builder.Append(escape);
                    break;
            }
        }
        return builder.ToString().Trim();
    }

    /// <summary>
    /// What the character was wearing when the addon last looked, in the web
    /// API's own slot names so nothing downstream asks which source it got.
    /// Absent is silence: a file written before this was recorded has no key
    /// at all, and an empty list would say "this character is naked". A
    /// cosmetic slot's level is read back as null, never zero.
    /// </summary>
    private static List<Equipped>? ReadEquipment(LuaValue db)
    {
        if (db.Get("equipment") is not { } equipment)
        {
            return null;
        }
        var worn = new List<Equipped>();
        foreach (var entry in equipment.Items())
        {
            if (entry.Items() is not [var slot, var name, var level, ..] || slot.AsStr() is not { } slotKey || name.AsStr() is not { } itemName)
            {
                continue;
            }
            var slotName = Equipped.Slots.Concat([("SHIRT", "Shirt"), ("TABARD", "Tabard")])
                .Where(known => known.Item1 == slotKey)
                .Select(known => known.Item2)
                .FirstOrDefault() ?? slotKey;
            worn.Add(new Equipped
            {
                Slot = slotKey,
                SlotName = slotName,
                Name = itemName,
                Level = level.AsInteger() is { } itemLevel && itemLevel > 0 ? (int)itemLevel : null,
            });
        }
        return worn;
    }

    /// <summary>Which raids this character is saved to for the current reset. Absent is silence; present and empty is "saved to nothing".</summary>
    private static List<RaidLock>? ReadRaidLocks(LuaValue db)
    {
        if (db.Get("raidLocks") is not { } locks)
        {
            return null;
        }
        var saved = new List<RaidLock>();
        foreach (var entry in locks.Items())
        {
            if (entry.Items() is not [var name, var difficulty, var defeated, var total, ..] || name.AsStr() is not { } raid || difficulty.AsStr() is not { } level)
            {
                continue;
            }
            saved.Add(new RaidLock { Name = raid, Difficulty = level, Defeated = (int)(defeated.AsInteger() ?? 0), Total = (int)(total.AsInteger() ?? 0) });
        }
        return saved;
    }

    /// <summary><c>{ type, index, threshold, progress, level, levelName }</c> per slot. Absent and empty differ the way they do for lockouts.</summary>
    private static List<VaultSlot>? ReadVault(LuaValue db)
    {
        if (db.Get("vault") is not { } vault)
        {
            return null;
        }
        var slots = new List<VaultSlot>();
        foreach (var entry in vault.Items())
        {
            var columns = entry.Items();
            if (columns is not [var row, var index, var threshold, var progress, var level, ..] || row.AsInteger() is not { } rowNumber)
            {
                continue;
            }
            slots.Add(new VaultSlot
            {
                Row = VaultRow.FromNumber(rowNumber),
                Index = (int)(index.AsInteger() ?? 0),
                Threshold = (int)(threshold.AsInteger() ?? 0),
                Progress = (int)(progress.AsInteger() ?? 0),
                Level = (int)(level.AsInteger() ?? 0),
                LevelName = columns.Count > 5 ? columns[5].AsStr() ?? "" : "",
            });
        }
        return slots;
    }

    /// <summary>Seconds since the epoch, as the game counts them.</summary>
    private static DateTimeOffset? Epoch(double? seconds) =>
        seconds is { } s && s >= 0 && s < 253_402_300_800 ? DateTimeOffset.FromUnixTimeSeconds((long)s) : null;

    /// <summary><c>WARRIOR</c> is how the game spells a class token; <c>Warrior</c> is how a person does.</summary>
    internal static string Titlecase(string token) =>
        token.Length == 0 ? "" : string.Concat(token[..1].ToUpperInvariant(), token[1..].ToLowerInvariant());

    /// <summary>Split the addon's <c>Name-Realm</c> spelling into a key. The game writes the realm's display name; every endpoint wants the slug.</summary>
    internal static CharacterKey? ParseCharacter(string? text)
    {
        if (text is null)
        {
            return null;
        }
        var dash = text.IndexOf('-', StringComparison.Ordinal);
        if (dash <= 0 || dash == text.Length - 1)
        {
            return null;
        }
        return new CharacterKey(Slug.RealmSlug(text[(dash + 1)..]), text[..dash]);
    }
}
