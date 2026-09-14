using System.Globalization;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace Armory.Roster;

/// <summary>
/// How a character is addressed on the profile endpoints. Ordered, so a
/// roster sorts stably by realm and then by name.
/// </summary>
/// <remarks>
/// The name is lowercased because the endpoints 404 on a capitalised one.
/// That is a fact about URLs rather than about the character, and anywhere a
/// key reaches prose <see cref="DisplayName"/> puts it back.
/// </remarks>
public sealed record CharacterKey : IComparable<CharacterKey>
{
    public CharacterKey(string realmSlug, string name)
    {
        RealmSlug = realmSlug;
        Name = name.ToLowerInvariant();
    }

    [JsonPropertyName("realm_slug")]
    public string RealmSlug { get; }

    /// <summary>Lowercased. The endpoints 404 on a capitalised name.</summary>
    [JsonPropertyName("name")]
    public string Name { get; }

    /// <summary>
    /// The name as a person would write it. Titlecased rather than looked up:
    /// this is used in sentences about characters who may have been deleted
    /// or transferred, which is exactly when the roster no longer has them.
    /// </summary>
    public string DisplayName() =>
        Name.Length == 0 ? "" : string.Concat(Name[..1].ToUpperInvariant(), Name.AsSpan(1));

    public int CompareTo(CharacterKey? other)
    {
        if (other is null)
        {
            return 1;
        }
        var realm = string.CompareOrdinal(RealmSlug, other.RealmSlug);
        return realm != 0 ? realm : string.CompareOrdinal(Name, other.Name);
    }

    public static bool operator <(CharacterKey left, CharacterKey right) => left.CompareTo(right) < 0;

    public static bool operator >(CharacterKey left, CharacterKey right) => left.CompareTo(right) > 0;

    public static bool operator <=(CharacterKey left, CharacterKey right) => left.CompareTo(right) <= 0;

    public static bool operator >=(CharacterKey left, CharacterKey right) => left.CompareTo(right) >= 0;
}

/// <summary>
/// One character, as the account index describes them. The cheap summary
/// that the account profile returns for everybody; the expensive
/// <see cref="Detail"/> hangs off it and is fetched only for the cohort.
/// </summary>
/// <remarks>
/// A character is identified two ways and both are needed: the profile
/// endpoints take a realm slug and a lowercased name, and the protected
/// endpoint, the only one that knows about gold, takes numeric realm and
/// character ids. Carrying both from the start is why this looks redundant.
/// </remarks>
public sealed record Character
{
    [JsonPropertyName("key")]
    public required CharacterKey Key { get; init; }

    /// <summary>The numeric id, for the protected endpoint.</summary>
    [JsonPropertyName("id")]
    public long Id { get; init; }

    [JsonPropertyName("realm_id")]
    public long RealmId { get; init; }

    /// <summary>As Blizzard capitalises it, for showing to a person.</summary>
    [JsonPropertyName("display_name")]
    public string DisplayName { get; init; } = "";

    [JsonPropertyName("realm_name")]
    public string RealmName { get; init; } = "";

    [JsonPropertyName("level")]
    public int Level { get; init; }

    [JsonPropertyName("class")]
    public string Class { get; init; } = "";

    [JsonPropertyName("race")]
    public string Race { get; init; } = "";

    [JsonPropertyName("faction")]
    public Faction Faction { get; init; }

    /// <summary>
    /// Which WoW licence under the Battle.net account this character sits on.
    /// One login can hold several; collections are shared across them.
    /// </summary>
    [JsonPropertyName("wow_account_id")]
    public long WowAccountId { get; init; }

    /// <summary>How the character is written in the interface.</summary>
    public string FullName() => $"{DisplayName} — {RealmName}";

    /// <summary>The path segment pair the protected-character endpoint wants.</summary>
    public string ProtectedId() => string.Create(CultureInfo.InvariantCulture, $"{RealmId}-{Id}");
}

/// <summary>
/// Which row of the Great Vault a slot is in: Blizzard's
/// <c>WeeklyRewardChestThresholdType</c>, by the numbers the addon writes.
/// A row this version does not know is shown by its number rather than
/// dropped: a new row is still a slot.
/// </summary>
[JsonConverter(typeof(VaultRowConverter))]
public readonly record struct VaultRow(VaultRowKind Kind, long Number)
{
    public static VaultRow Dungeons => new(VaultRowKind.Dungeons, 1);

    public static VaultRow Pvp => new(VaultRowKind.Pvp, 2);

    public static VaultRow Raids => new(VaultRowKind.Raids, 3);

    public static VaultRow World => new(VaultRowKind.World, 6);

    public static VaultRow FromNumber(long number) => number switch
    {
        1 => Dungeons,
        2 => Pvp,
        3 => Raids,
        6 => World,
        _ => new VaultRow(VaultRowKind.Other, number),
    };

    public string Label() => Kind switch
    {
        VaultRowKind.Dungeons => "Dungeons",
        VaultRowKind.Pvp => "PvP",
        VaultRowKind.Raids => "Raids",
        VaultRowKind.World => "World",
        _ => string.Create(CultureInfo.InvariantCulture, $"Row {Number}"),
    };
}

public enum VaultRowKind
{
    Dungeons,
    Pvp,
    Raids,
    World,
    Other,
}

/// <summary>The Rust enum's JSON: a bare name for the known rows, <c>{"Other": n}</c> for the rest.</summary>
public sealed class VaultRowConverter : JsonConverter<VaultRow>
{
    public override VaultRow Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        if (reader.TokenType == JsonTokenType.String)
        {
            return reader.GetString() switch
            {
                "Dungeons" => VaultRow.Dungeons,
                "Pvp" => VaultRow.Pvp,
                "Raids" => VaultRow.Raids,
                "World" => VaultRow.World,
                var other => throw new JsonException($"not a vault row: {other}"),
            };
        }
        if (reader.TokenType != JsonTokenType.StartObject)
        {
            throw new JsonException("a vault row is a name or an object");
        }
        reader.Read();
        if (reader.TokenType != JsonTokenType.PropertyName || reader.GetString() != "Other")
        {
            throw new JsonException("a vault row object carries Other");
        }
        reader.Read();
        var number = reader.GetInt64();
        reader.Read();
        return new VaultRow(VaultRowKind.Other, number);
    }

    public override void Write(Utf8JsonWriter writer, VaultRow value, JsonSerializerOptions options)
    {
        if (value.Kind == VaultRowKind.Other)
        {
            writer.WriteStartObject();
            writer.WriteNumber("Other", value.Number);
            writer.WriteEndObject();
            return;
        }
        writer.WriteStringValue(value.Kind.ToString());
    }
}

/// <summary>
/// One slot of the Great Vault, from the game client, which is the only
/// thing that knows it, and true as of the last logout.
/// </summary>
public sealed record VaultSlot
{
    [JsonPropertyName("row")]
    public VaultRow Row { get; init; }

    /// <summary>First, second or third slot in the row.</summary>
    [JsonPropertyName("index")]
    public int Index { get; init; }

    /// <summary>How many runs, kills or activities the slot wants.</summary>
    [JsonPropertyName("threshold")]
    public int Threshold { get; init; }

    [JsonPropertyName("progress")]
    public int Progress { get; init; }

    /// <summary>What decides the reward: a keystone level, a difficulty, a tier.</summary>
    [JsonPropertyName("level")]
    public int Level { get; init; }

    /// <summary>The level spelled by the client where a number would mislead. Empty where the number speaks for itself.</summary>
    [JsonPropertyName("level_name")]
    public string LevelName { get; init; } = "";

    [JsonIgnore]
    public bool IsUnlocked => Threshold > 0 && Progress >= Threshold;

    /// <summary>The reward tier as a person would say it: <c>+12</c>, <c>Heroic</c>, <c>Tier 8</c>.</summary>
    public string Reward()
    {
        if (LevelName.Length > 0)
        {
            return LevelName;
        }
        return Row.Kind switch
        {
            VaultRowKind.Dungeons => string.Create(CultureInfo.InvariantCulture, $"+{Level}"),
            VaultRowKind.World => string.Create(CultureInfo.InvariantCulture, $"Tier {Level}"),
            _ => Level.ToString(CultureInfo.InvariantCulture),
        };
    }
}

/// <summary>One worn item.</summary>
public sealed record Equipped
{
    /// <summary>
    /// Every slot a character can fill, in the order Blizzard's own character
    /// sheet reads down. The equipment response says nothing about what is
    /// not worn, and "the off hand is empty" is the most actionable line on
    /// the character page.
    /// </summary>
    public static IReadOnlyList<(string Slot, string Name)> Slots { get; } =
    [
        ("HEAD", "Head"),
        ("NECK", "Neck"),
        ("SHOULDER", "Shoulder"),
        ("BACK", "Back"),
        ("CHEST", "Chest"),
        ("WRIST", "Wrist"),
        ("HANDS", "Hands"),
        ("WAIST", "Waist"),
        ("LEGS", "Legs"),
        ("FEET", "Feet"),
        ("FINGER_1", "Ring 1"),
        ("FINGER_2", "Ring 2"),
        ("TRINKET_1", "Trinket 1"),
        ("TRINKET_2", "Trinket 2"),
        ("MAIN_HAND", "Main Hand"),
        ("OFF_HAND", "Off Hand"),
    ];

    /// <summary>Worn for the look of the thing, with no item level, and never counted as empty.</summary>
    public static IReadOnlyList<string> Cosmetic { get; } = ["SHIRT", "TABARD"];

    /// <summary>Blizzard's own slot type, the stable half, in which <see cref="Slots"/> is written.</summary>
    [JsonPropertyName("slot")]
    public string Slot { get; init; } = "";

    [JsonPropertyName("slot_name")]
    public string SlotName { get; init; } = "";

    [JsonPropertyName("name")]
    public string Name { get; init; } = "";

    /// <summary>
    /// Null for a cosmetic slot, which has no item level at all. Not zero and
    /// not a guess: a shirt with a fabricated 1 would sort to the top of a
    /// list whose purpose is to put the weakest slot first.
    /// </summary>
    [JsonPropertyName("level")]
    public int? Level { get; init; }

    [JsonIgnore]
    public bool IsCosmetic => Cosmetic.Contains(Slot);
}

/// <summary>The last boss to fall in one difficulty, and when. The Rust's <c>(String, DateTime)</c> pair, a JSON array.</summary>
[JsonConverter(typeof(KillConverter))]
public sealed record Kill(string Boss, DateTimeOffset At);

public sealed class KillConverter : JsonConverter<Kill>
{
    public override Kill Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        if (reader.TokenType != JsonTokenType.StartArray)
        {
            throw new JsonException("a kill is a pair");
        }
        reader.Read();
        var boss = reader.GetString() ?? "";
        reader.Read();
        var at = reader.GetDateTimeOffset();
        reader.Read();
        return new Kill(boss, at);
    }

    public override void Write(Utf8JsonWriter writer, Kill value, JsonSerializerOptions options)
    {
        writer.WriteStartArray();
        writer.WriteStringValue(value.Boss);
        writer.WriteStringValue(value.At.ToUniversalTime().ToString("yyyy-MM-dd'T'HH:mm:ss'Z'", CultureInfo.InvariantCulture));
        writer.WriteEndArray();
    }
}

/// <summary>One difficulty of one raid.</summary>
public sealed record RaidDifficulty
{
    /// <summary><c>Normal</c>, <c>Heroic</c>, <c>Mythic</c>, <c>Raid Finder</c>.</summary>
    [JsonPropertyName("name")]
    public string Name { get; init; } = "";

    [JsonPropertyName("defeated")]
    public int Defeated { get; init; }

    [JsonPropertyName("total")]
    public int Total { get; init; }

    /// <summary>Null when nothing has fallen.</summary>
    [JsonPropertyName("last_kill")]
    public Kill? LastKill { get; init; }
}

/// <summary>
/// One raid, and how far into it this character has got. A tier here is a
/// raid instance rather than an expansion, because that is the unit a person
/// means by "the current tier".
/// </summary>
public sealed record RaidTier
{
    [JsonPropertyName("name")]
    public string Name { get; init; } = "";

    [JsonPropertyName("expansion")]
    public string Expansion { get; init; } = "";

    /// <summary>One entry per difficulty set foot in. Nothing attempted and nothing killed are different facts.</summary>
    [JsonPropertyName("difficulties")]
    public List<RaidDifficulty> Difficulties { get; init; } = [];

    /// <summary>The last kill anywhere in this raid, at any difficulty.</summary>
    public (string Boss, DateTimeOffset At, string Difficulty)? LastKill()
    {
        (string, DateTimeOffset, string)? latest = null;
        foreach (var difficulty in Difficulties)
        {
            if (difficulty.LastKill is { } kill && (latest is null || kill.At > latest.Value.Item2))
            {
                latest = (kill.Boss, kill.At, difficulty.Name);
            }
        }
        return latest;
    }
}

/// <summary>One raid this character is saved to for the current reset. From the game client.</summary>
public sealed record RaidLock
{
    [JsonPropertyName("name")]
    public string Name { get; init; } = "";

    [JsonPropertyName("difficulty")]
    public string Difficulty { get; init; } = "";

    [JsonPropertyName("defeated")]
    public int Defeated { get; init; }

    [JsonPropertyName("total")]
    public int Total { get; init; }
}

/// <summary>A specialisation tree, and whether it has been opened. The Rust's <c>(String, bool)</c>, a JSON array.</summary>
[JsonConverter(typeof(SpecialisationConverter))]
public sealed record Specialisation(string Name, bool Opened);

public sealed class SpecialisationConverter : JsonConverter<Specialisation>
{
    public override Specialisation Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        if (reader.TokenType != JsonTokenType.StartArray)
        {
            throw new JsonException("a specialisation is a pair");
        }
        reader.Read();
        var name = reader.GetString() ?? "";
        reader.Read();
        var opened = reader.GetBoolean();
        reader.Read();
        return new Specialisation(name, opened);
    }

    public override void Write(Utf8JsonWriter writer, Specialisation value, JsonSerializerOptions options)
    {
        writer.WriteStartArray();
        writer.WriteStringValue(value.Name);
        writer.WriteBooleanValue(value.Opened);
        writer.WriteEndArray();
    }
}

/// <summary>One profession and how far along it is.</summary>
public sealed record Profession
{
    [JsonPropertyName("name")]
    public string Name { get; init; } = "";

    /// <summary>The current expansion's tier, which is the one that matters.</summary>
    [JsonPropertyName("tier")]
    public string? Tier { get; init; }

    [JsonPropertyName("skill")]
    public int? Skill { get; init; }

    [JsonPropertyName("max_skill")]
    public int? MaxSkill { get; init; }

    /// <summary>Whether this is a primary profession rather than cooking or fishing.</summary>
    [JsonPropertyName("is_primary")]
    public bool IsPrimary { get; init; }

    /// <summary>Specialisation trees, and whether each has been opened. Addon only; no endpoint could supply it.</summary>
    [JsonPropertyName("specialisations")]
    public List<Specialisation> Specialisations { get; init; } = [];

    /// <summary>How much knowledge this profession has ever been given. Zero is "not specialised, or a client that predates them".</summary>
    [JsonPropertyName("knowledge")]
    public long Knowledge { get; init; }
}

/// <summary>
/// The expensive half of a character, fetched only for the enrolled cohort.
/// Every field is optional because every one comes from a different endpoint
/// that can independently be empty, unchanged or broken.
/// </summary>
public sealed record Detail
{
    /// <summary>Averaged across everything worn, and the one people compare.</summary>
    [JsonPropertyName("item_level")]
    public int? ItemLevel { get; init; }

    /// <summary>Counting only equipped slots. Lower than the average when a slot is empty, which is the case worth seeing.</summary>
    [JsonPropertyName("equipped_item_level")]
    public int? EquippedItemLevel { get; init; }

    [JsonPropertyName("spec")]
    public string? Spec { get; init; }

    [JsonPropertyName("guild")]
    public string? Guild { get; init; }

    /// <summary>Copper. From the protected endpoint; the plain profile has no money.</summary>
    [JsonPropertyName("money")]
    public long? Money { get; init; }

    [JsonPropertyName("achievement_points")]
    public long? AchievementPoints { get; init; }

    /// <summary>When Blizzard last saw them log out, which is when everything else here was true.</summary>
    [JsonPropertyName("last_login")]
    public DateTimeOffset? LastLogin { get; init; }

    [JsonPropertyName("professions")]
    public List<Profession> Professions { get; init; } = [];

    /// <summary>Best Mythic+ rating this season.</summary>
    [JsonPropertyName("mythic_rating")]
    public long? MythicRating { get; init; }

    /// <summary>Highest renown across the account-wide major factions. A fact about the account, never run progress.</summary>
    [JsonPropertyName("renown")]
    public long? Renown { get; init; }

    /// <summary>What is worn, slot by slot. Only the slots that hold something; the page draws the absence.</summary>
    [JsonPropertyName("equipment")]
    public List<Equipped>? Equipment { get; init; }

    /// <summary>Raid progress, every boss ever killed. Web API only.</summary>
    [JsonPropertyName("raids")]
    public List<RaidTier>? Raids { get; init; }

    /// <summary>The raids this character is saved to this week. A different fact from <see cref="Raids"/>, kept apart.</summary>
    [JsonPropertyName("raid_locks")]
    public List<RaidLock>? RaidLocks { get; init; }

    /// <summary>The Great Vault, slot by slot, as the client last saw it. Addon only.</summary>
    [JsonPropertyName("vault")]
    public List<VaultSlot>? Vault { get; init; }

    /// <summary>Whether last week's vault is still waiting to be opened.</summary>
    [JsonPropertyName("vault_ready")]
    public bool VaultReady { get; init; }

    /// <summary>
    /// This, with everything <paramref name="fresh"/> actually answered
    /// taken, and the rest kept.
    /// </summary>
    /// <remarks>
    /// The two sources answer overlapping halves and neither answers all of
    /// it. The addon knows the specialisation trees and this week's lockouts
    /// and cannot know the Mythic+ rating or a lifetime of raiding; the API is
    /// the other way round. An addon read that assigned the whole record
    /// would blank four fields every logout. Null is silence, not an answer.
    /// </remarks>
    public Detail Absorb(Detail fresh)
    {
        var professions = Professions;
        if (fresh.Professions.Count > 0)
        {
            professions = fresh.Professions.Select(arriving =>
            {
                var held = Professions.FirstOrDefault(held => held.Name == arriving.Name);
                if (held is null)
                {
                    return arriving;
                }
                // A tier is the API's and the trees are the addon's, and a
                // profession row carries both or neither depending on who
                // wrote it last.
                return arriving with
                {
                    Tier = arriving.Tier ?? held.Tier,
                    Specialisations = arriving.Specialisations.Count == 0 ? held.Specialisations : arriving.Specialisations,
                    Knowledge = arriving.Knowledge == 0 ? held.Knowledge : arriving.Knowledge,
                };
            }).ToList();
        }

        return this with
        {
            ItemLevel = fresh.ItemLevel ?? ItemLevel,
            EquippedItemLevel = fresh.EquippedItemLevel ?? EquippedItemLevel,
            Spec = fresh.Spec ?? Spec,
            Guild = fresh.Guild ?? Guild,
            Money = fresh.Money ?? Money,
            AchievementPoints = fresh.AchievementPoints ?? AchievementPoints,
            LastLogin = fresh.LastLogin ?? LastLogin,
            MythicRating = fresh.MythicRating ?? MythicRating,
            Renown = fresh.Renown ?? Renown,
            Equipment = fresh.Equipment ?? Equipment,
            Raids = fresh.Raids ?? Raids,
            RaidLocks = fresh.RaidLocks ?? RaidLocks,
            // The flag travels with the slots: an answer that has the vault
            // has both halves of it, and one that does not has neither.
            Vault = fresh.Vault ?? Vault,
            VaultReady = fresh.Vault is not null ? fresh.VaultReady : VaultReady,
            Professions = professions,
        };
    }
}

/// <summary>Every character on the account, across every realm and every licence, in key order.</summary>
public sealed record Roster
{
    public Roster()
    {
    }

    public Roster(IEnumerable<Character> characters)
    {
        Characters = characters.OrderBy(character => character.Key).ToList();
    }

    [JsonPropertyName("characters")]
    public List<Character> Characters { get; init; } = [];

    public Character? Get(CharacterKey key) => Characters.FirstOrDefault(character => character.Key == key);

    [JsonIgnore]
    public bool IsEmpty => Characters.Count == 0;

    [JsonIgnore]
    public int Count => Characters.Count;

    /// <summary>
    /// The realms the account has characters on, in display order,
    /// deduplicated. Each is a separate auction house for everything except
    /// commodities.
    /// </summary>
    public List<(string Slug, string Name)> Realms() =>
        Characters.Select(character => (character.Key.RealmSlug, character.RealmName))
            .Distinct()
            .OrderBy(realm => realm.RealmSlug, StringComparer.Ordinal)
            .ThenBy(realm => realm.RealmName, StringComparer.Ordinal)
            .ToList();

    public bool Equals(Roster? other) => other is not null && Characters.SequenceEqual(other.Characters);

    public override int GetHashCode() => Characters.Count;
}
