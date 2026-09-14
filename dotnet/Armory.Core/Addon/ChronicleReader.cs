using System.Globalization;
using System.Text;
using Armory.Blizzard;
using Armory.Chronicle;
using Armory.Roster;

namespace Armory.Addon;

/// <summary>Why a chronicle file could not be used. The same three cases as the collector's, for the same reasons.</summary>
public abstract record ChronicleReadError
{
    public sealed record Unparsable(string Detail) : ChronicleReadError
    {
        public override string ToString() => Detail;
    }

    public sealed record NotChronicleData : ChronicleReadError
    {
        public override string ToString() => "this file was not written by the Armory chronicle";
    }

    public sealed record FromTheFuture(long Format) : ChronicleReadError
    {
        public override string ToString() =>
            string.Create(CultureInfo.InvariantCulture, $"the chronicle addon is writing format {Format} and this version of Armory reads {ChronicleReader.Format} — update Armory");
    }
}

/// <summary>
/// Reading what the chronicle addon wrote: one global, <c>ArmoryChronicleDB</c>,
/// per character.
/// </summary>
/// <remarks>
/// Every event row is the same five positions, <c>{ at, kind, a, b, c }</c>,
/// with absent fields written as empty strings rather than left nil. WoW's
/// serializer writes a table with a hole in the middle as keyed entries rather
/// than as a padded array, so dense rows mean one shape to read.
/// </remarks>
public static class ChronicleReader
{
    private const string Global = "ArmoryChronicleDB";

    /// <summary>The format this reader understands.</summary>
    public const long Format = 3;

    public static Result<List<Session>, ChronicleReadError> Read(string source) => Read(Encoding.UTF8.GetBytes(source));

    /// <summary>
    /// Read one character's chronicle file into sessions. A session that will
    /// not read is skipped rather than failing the file: one unreadable
    /// evening is a better outcome than losing the other thirty-nine.
    /// </summary>
    public static Result<List<Session>, ChronicleReadError> Read(byte[] source)
    {
        var parsed = Lua.Parse(source);
        if (!parsed.IsOk)
        {
            return Result<List<Session>, ChronicleReadError>.Err(new ChronicleReadError.Unparsable(parsed.Error.ToString()));
        }
        if (!parsed.Value.TryGetValue(Global, out var db))
        {
            return Result<List<Session>, ChronicleReadError>.Err(new ChronicleReadError.NotChronicleData());
        }
        var format = db.Get("format")?.AsInteger() ?? 0;
        if (format > Format)
        {
            return Result<List<Session>, ChronicleReadError>.Err(new ChronicleReadError.FromTheFuture(format));
        }
        if (db.Get("sessions") is not { } list)
        {
            return Result<List<Session>, ChronicleReadError>.Ok([]);
        }
        return Result<List<Session>, ChronicleReadError>.Ok(list.Items().Select(ReadSession).OfType<Session>().ToList());
    }

    private static Session? ReadSession(LuaValue entry)
    {
        if (entry.Get("name")?.AsStr() is not { } name || entry.Get("realm")?.AsStr() is not { } realm)
        {
            return null;
        }
        if (Epoch(entry.Get("startedAt")?.AsDouble()) is not { } startedAt)
        {
            return null;
        }
        // A session with no end was interrupted. The events in it are still
        // real, so it is closed at the last one rather than thrown away.
        var endedAt = Epoch(entry.Get("endedAt")?.AsDouble()) ?? startedAt;
        var moments = entry.Get("events")?.Items().Select(ReadMoment).OfType<Moment>().ToList() ?? [];
        if (moments.Count > 0)
        {
            var last = startedAt + TimeSpan.FromSeconds(moments[^1].At);
            if (last > endedAt)
            {
                endedAt = last;
            }
        }

        var risen = new List<Risen>();
        foreach (var row in entry.Get("risen")?.Items() ?? [])
        {
            if (row.Items() is [var faction, var rank, ..] && faction.AsStr() is { } named)
            {
                risen.Add(new Risen(named, (int)(rank.AsInteger() ?? 0)));
            }
        }

        return new Session
        {
            Character = new CharacterKey(Slug.RealmSlug(realm), name),
            DisplayName = name,
            RealmName = realm,
            Class = Collector.Titlecase(entry.Get("class")?.AsStr() ?? ""),
            Race = Collector.Titlecase(entry.Get("race")?.AsStr() ?? ""),
            Faction = entry.Get("faction")?.AsStr() switch
            {
                "Alliance" => Faction.Alliance,
                "Horde" => Faction.Horde,
                _ => Faction.Neutral,
            },
            StartedAt = startedAt,
            EndedAt = endedAt,
            StartLevel = (int)Number(entry, "startLevel"),
            EndLevel = (int)Number(entry, "endLevel"),
            StartMoney = Number(entry, "startMoney"),
            EndMoney = Number(entry, "endMoney"),
            StartItemLevel = (int)Number(entry, "startItemLevel"),
            EndItemLevel = (int)Number(entry, "endItemLevel"),
            Moments = moments,
            // Session totals, written at logout. A kill count, the hardest
            // hit and the lowest health were read here until patch 12.0
            // closed the combat log to addons; older files still carry them
            // and are simply not asked.
            Travelled = Number(entry, "travelled"),
            LongestFight = Number(entry, "longestFight"),
            Risen = risen,
        };
    }

    /// <summary>One <c>{ at, kind, a, b, c }</c> row. A kind this version does not know is dropped, which is what makes an older Armory readable by a newer addon within the same format.</summary>
    private static Moment? ReadMoment(LuaValue row)
    {
        var fields = row.Items();
        if (fields.Count < 2 || fields[0].AsDouble() is not { } at || fields[1].AsStr() is not { } kind)
        {
            return null;
        }
        var a = Text(fields.ElementAtOrDefault(2));
        var b = Text(fields.ElementAtOrDefault(3));
        var c = Text(fields.ElementAtOrDefault(4));

        Happening? what = kind switch
        {
            "zone" => a is null ? null : new Happening.Arrived(a, b, Long(c)),
            "accepted" => a is null ? null : new Happening.Accepted(a, b),
            "quest" => Long(a) is { } quest ? new Happening.Completed(quest, b ?? "", c) : null,
            "questpay" => Long(a) is { } paid ? new Happening.Paid(paid, Long(b) ?? 0, Long(c) ?? 0) : null,
            "campaign" => a is null ? null : new Happening.Campaign(a, b),
            "level" => Long(a) is { } level ? new Happening.Levelled((int)level, b ?? "") : null,
            "death" => a is null ? null : new Happening.Died(a, b, c),
            "instance" => a is null ? null : new Happening.Entered(a, b ?? "", (int)(Long(c) ?? 0)),
            "worldtier" => a is null ? null : new Happening.WorldTier(a),
            "weather" => a is null ? null : new Happening.Weather(a, b),
            "keystone" => KeystoneOf(a, b),
            "said" => b is null ? null : new Happening.Said(a ?? "", b),
            "giver" => a is null ? null : new Happening.Gave(a, Long(b), Long(c)),
            "gossip" => b is null ? null : new Happening.Told(a ?? "", b),
            "expired" => a is null ? null : new Happening.Expired(a),
            "cutscene" => a is null ? null : new Happening.Cutscene(a, Long(b)),
            "recipe" => a is null ? null : new Happening.Learned(a),
            "scenario" => a is null ? null : new Happening.Scenario(a, b),
            "rare" => a is null ? null : new Happening.Rare(a, b ?? ""),
            "skill" => a is null ? null : new Happening.Practised(a, (int)(Long(b) ?? 0)),
            "equipped" => a is null ? null : new Happening.Equipped(a, (int)(Long(b) ?? 0), (int)(Long(c) ?? 0)),
            "appearance" => a is null ? null : new Happening.Appearance(a),
            "coin" => a is null ? null : new Happening.Coin(PurposeExtensions.FromToken(a), Long(b) ?? 0, c == "1"),
            "craft" => Long(a) is { } recipe ? new Happening.Crafted(recipe, b ?? "") : null,
            "shot" => a is null ? null : new Happening.Pictured(a, b ?? ""),
            "boss" => a is null ? null : new Happening.Felled(a),
            "encounter" => a is null ? null : new Happening.Fought(a, b == "1"),
            "wipe" => a is not null && Long(b) is { } remaining ? new Happening.Wiped(a, (int)remaining) : null,
            "achievement" => Long(a) is { } achievement ? new Happening.Earned(achievement, b ?? "") : null,
            "gained" => a is not null && AcquisitionExtensions.FromToken(a) is { } got && b is not null ? new Happening.Acquired(got, b) : null,
            "loot" => Long(a) is { } item && b is not null ? new Happening.Looted(item, b, (int)(Long(c) ?? 0)) : null,
            "flight" => a is null ? null : new Happening.Flew(a),
            "sale" => a is null ? null : new Happening.Sold(a, Long(b) ?? 0),
            "with" => a is null ? null : new Happening.Alongside(a),
            _ => null,
        };
        return what is null ? null : new Moment { At = (long)at, What = what };
    }

    /// <summary><c>dungeon|level|onTime|upgrades</c> in one field, because five positions is what a row has and a keystone needs six.</summary>
    private static Happening? KeystoneOf(string? packed, string? seconds)
    {
        if (packed is null)
        {
            return null;
        }
        var parts = packed.Split('|');
        if (parts.Length < 4)
        {
            return null;
        }
        var upgrades = Long(parts[^1]);
        var inTime = parts[^2] == "1";
        var level = Long(parts[^3]);
        var dungeon = string.Join("|", parts[..^3]);
        if (upgrades is null || level is null)
        {
            return null;
        }
        return new Happening.Keystone(dungeon, (int)level.Value, inTime, (int)upgrades.Value, Long(seconds) ?? 0);
    }

    /// <summary>A field as text, with the addon's empty-string stand-in read back as absent. Numbers are rendered so one accessor serves both.</summary>
    private static string? Text(LuaValue? value) => value switch
    {
        LuaValue.Str { Text.Length: 0 } => null,
        LuaValue.Str text => text.Text,
        LuaValue.Number number when number.Value == Math.Floor(number.Value) => ((long)number.Value).ToString(CultureInfo.InvariantCulture),
        LuaValue.Number number => number.Value.ToString(CultureInfo.InvariantCulture),
        _ => null,
    };

    private static long? Long(string? text) =>
        text is not null && long.TryParse(text, NumberStyles.Integer, CultureInfo.InvariantCulture, out var number) ? number : null;

    private static long Number(LuaValue entry, string field)
    {
        var number = entry.Get(field)?.AsDouble();
        return number is { } value && value >= 0 ? (long)value : 0;
    }

    private static DateTimeOffset? Epoch(double? seconds) =>
        seconds is { } s && s >= 0 && s < 253_402_300_800 ? DateTimeOffset.FromUnixTimeSeconds((long)s) : null;
}
