using System.Globalization;
using System.Text.Json;
using System.Text.Json.Serialization;
using Armory.Roster;

namespace Armory.Chronicle;

/// <summary>
/// One recorded play session, exactly as the addon saw it.
/// </summary>
/// <remarks>
/// The chronicle is the only thing in Armory that is about time rather than
/// state, and it is fed entirely by the addon: Blizzard's profile API is a
/// logout snapshot with no history in it. <c>session.json</c> is a column
/// that travels, so this record's JSON is the Rust <c>Session</c>'s field for
/// field.
/// </remarks>
public sealed record Session
{
    [JsonPropertyName("character")]
    public required CharacterKey Character { get; init; }

    /// <summary>As the game capitalises it.</summary>
    [JsonPropertyName("display_name")]
    public string DisplayName { get; init; } = "";

    [JsonPropertyName("realm_name")]
    public string RealmName { get; init; } = "";

    [JsonPropertyName("class")]
    public string Class { get; init; } = "";

    [JsonPropertyName("race")]
    public string Race { get; init; } = "";

    [JsonPropertyName("faction")]
    public Faction Faction { get; init; }

    [JsonPropertyName("started_at")]
    public DateTimeOffset StartedAt { get; init; }

    [JsonPropertyName("ended_at")]
    public DateTimeOffset EndedAt { get; init; }

    [JsonPropertyName("start_level")]
    public int StartLevel { get; init; }

    [JsonPropertyName("end_level")]
    public int EndLevel { get; init; }

    /// <summary>Copper.</summary>
    [JsonPropertyName("start_money")]
    public long StartMoney { get; init; }

    [JsonPropertyName("end_money")]
    public long EndMoney { get; init; }

    [JsonPropertyName("start_item_level")]
    public int StartItemLevel { get; init; }

    [JsonPropertyName("end_item_level")]
    public int EndItemLevel { get; init; }

    [JsonPropertyName("moments")]
    public List<Moment> Moments { get; init; } = [];

    /// <summary>Factions that went up a rank, with the rank reached. A session total rather than a moment.</summary>
    [JsonPropertyName("risen")]
    public List<Risen> Risen { get; init; } = [];

    /// <summary>Yards covered, on foot and in the air together. There is no event for moving at all.</summary>
    [JsonPropertyName("travelled")]
    public long Travelled { get; init; }

    /// <summary>The longest single fight, in seconds.</summary>
    [JsonPropertyName("longest_fight")]
    public long LongestFight { get; init; }

    /// <summary>The key an entry is filed under: one character, one start time.</summary>
    public SessionId Id => new(Character, StartedAt);

    public TimeSpan Duration => EndedAt - StartedAt;

    public bool Equals(Session? other) =>
        other is not null
        && Character == other.Character
        && DisplayName == other.DisplayName
        && RealmName == other.RealmName
        && Class == other.Class
        && Race == other.Race
        && Faction == other.Faction
        && StartedAt == other.StartedAt
        && EndedAt == other.EndedAt
        && StartLevel == other.StartLevel
        && EndLevel == other.EndLevel
        && StartMoney == other.StartMoney
        && EndMoney == other.EndMoney
        && StartItemLevel == other.StartItemLevel
        && EndItemLevel == other.EndItemLevel
        && Moments.SequenceEqual(other.Moments)
        && Risen.SequenceEqual(other.Risen)
        && Travelled == other.Travelled
        && LongestFight == other.LongestFight;

    public override int GetHashCode() => HashCode.Combine(Character, StartedAt, Moments.Count);
}

/// <summary>A faction that went up a rank. The Rust's <c>(String, u8)</c>, a JSON array.</summary>
[JsonConverter(typeof(RisenConverter))]
public sealed record Risen(string Faction, int Rank);

public sealed class RisenConverter : JsonConverter<Risen>
{
    public override Risen Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        if (reader.TokenType != JsonTokenType.StartArray)
        {
            throw new JsonException("a rank reached is a pair");
        }
        reader.Read();
        var faction = reader.GetString() ?? "";
        reader.Read();
        var rank = reader.GetInt32();
        reader.Read();
        return new Risen(faction, rank);
    }

    public override void Write(Utf8JsonWriter writer, Risen value, JsonSerializerOptions options)
    {
        writer.WriteStartArray();
        writer.WriteStringValue(value.Faction);
        writer.WriteNumberValue(value.Rank);
        writer.WriteEndArray();
    }
}

/// <summary>What identifies a session everywhere it is stored or referred to.</summary>
public sealed record SessionId(CharacterKey Character, DateTimeOffset StartedAt)
{
    public override string ToString() =>
        string.Create(CultureInfo.InvariantCulture, $"{Character.RealmSlug}/{Character.Name}@{Stamps.Rfc3339WithOffset(StartedAt)}");
}

/// <summary>One thing that happened, and how far into the session it happened.</summary>
public sealed record Moment
{
    /// <summary>Seconds since the session began. "An hour in" is a fact about the evening and a Unix timestamp is not.</summary>
    [JsonPropertyName("at")]
    public long At { get; init; }

    [JsonPropertyName("what")]
    public required Happening What { get; init; }
}

/// <summary>What money was doing when it moved. The frame that was open is the attribution.</summary>
[JsonConverter(typeof(JsonStringEnumConverter<Purpose>))]
public enum Purpose
{
    Quest,

    /// <summary>Coin picked up, with nothing open to explain it.</summary>
    Loot,
    Vendor,
    Repair,

    /// <summary>An auction bid or buyout.</summary>
    Bid,

    /// <summary>A listing deposit.</summary>
    Deposit,

    /// <summary>The auction house paying out. Income.</summary>
    Sale,

    /// <summary>Gold another character on this account sent over. Not income.</summary>
    Transfer,

    /// <summary>Gold somebody else sent.</summary>
    Gift,

    /// <summary>Money out of a mailbox whose sender was not readable.</summary>
    Mail,
    Trade,
    Taxi,
    Trainer,
    Transmog,
    Barber,
    GuildBank,

    /// <summary>Money left with nothing open to account for it. Admitted rather than filed under something plausible.</summary>
    Unknown,
}

public static class PurposeExtensions
{
    public static Purpose FromToken(string token) => token switch
    {
        "quest" => Purpose.Quest,
        "loot" => Purpose.Loot,
        "vendor" => Purpose.Vendor,
        "repair" => Purpose.Repair,
        "bid" => Purpose.Bid,
        "deposit" => Purpose.Deposit,
        "sale" => Purpose.Sale,
        "transfer" => Purpose.Transfer,
        "gift" => Purpose.Gift,
        "mail" => Purpose.Mail,
        "trade" => Purpose.Trade,
        "taxi" => Purpose.Taxi,
        "trainer" => Purpose.Trainer,
        "transmog" => Purpose.Transmog,
        "barber" => Purpose.Barber,
        "guildbank" => Purpose.GuildBank,
        _ => Purpose.Unknown,
    };

    /// <summary>How it reads on a card, as the receiving or the paying side.</summary>
    public static string Label(this Purpose purpose, bool incoming) => (purpose, incoming) switch
    {
        (Purpose.Quest, _) => "Quest rewards",
        (Purpose.Loot, _) => "Found",
        (Purpose.Vendor, true) => "Sold to vendors",
        (Purpose.Vendor, false) => "Bought from vendors",
        (Purpose.Repair, _) => "Repairs",
        (Purpose.Bid, true) => "Auction refunds",
        (Purpose.Bid, false) => "Auction purchases",
        (Purpose.Deposit, _) => "Auction deposits",
        (Purpose.Sale, _) => "Auction sales",
        (Purpose.Transfer, _) => "Sent from another character",
        (Purpose.Gift, _) => "Sent by somebody else",
        (Purpose.Mail, true) => "Mail",
        (Purpose.Mail, false) => "Postage",
        (Purpose.Trade, true) => "Traded to you",
        (Purpose.Trade, false) => "Traded away",
        (Purpose.Taxi, _) => "Flights",
        (Purpose.Trainer, _) => "Training",
        (Purpose.Transmog, _) => "Transmogrification",
        (Purpose.Barber, _) => "The barber",
        (Purpose.GuildBank, true) => "From the guild bank",
        (Purpose.GuildBank, false) => "To the guild bank",
        (_, true) => "Arrived",
        (_, false) => "Spent",
    };
}

[JsonConverter(typeof(JsonStringEnumConverter<Acquisition>))]
public enum Acquisition
{
    Mount,
    Pet,
    Toy,
}

public static class AcquisitionExtensions
{
    public static Acquisition? FromToken(string token) => token switch
    {
        "mount" => Acquisition.Mount,
        "pet" => Acquisition.Pet,
        "toy" => Acquisition.Toy,
        _ => null,
    };

    public static string Label(this Acquisition kind) => kind switch
    {
        Acquisition.Mount => "mount",
        Acquisition.Pet => "pet",
        _ => "toy",
    };
}

/// <summary>
/// What the addon can tell us happened. A closed set, and deliberately
/// narrow: every variant is something the game raises a documented event
/// for. On the wire it is serde's externally tagged form,
/// <c>{"Arrived":{"zone":"…","subzone":null,"map":null}}</c>.
/// </summary>
[JsonConverter(typeof(HappeningConverter))]
public abstract record Happening
{
    /// <summary>Entered a zone. The map is Blizzard's <c>UiMapID</c>, the join key, because there are two Nagrands.</summary>
    public sealed record Arrived(
        [property: JsonPropertyName("zone")] string Zone,
        [property: JsonPropertyName("subzone")] string? Subzone,
        [property: JsonPropertyName("map")] long? Map) : Happening;

    /// <summary>Took a quest, with the premise the quest giver gave.</summary>
    public sealed record Accepted(
        [property: JsonPropertyName("title")] string Title,
        [property: JsonPropertyName("premise")] string? Premise) : Happening;

    /// <summary>Turned a quest in. The story is the sentences the player actually read, which no endpoint returns.</summary>
    public sealed record Completed(
        [property: JsonPropertyName("quest")] long Quest,
        [property: JsonPropertyName("title")] string Title,
        [property: JsonPropertyName("story")] string? Story) : Happening;

    /// <summary>What a quest paid, kept apart from the session's gold.</summary>
    public sealed record Paid(
        [property: JsonPropertyName("quest")] long Quest,
        [property: JsonPropertyName("money")] long Money,
        [property: JsonPropertyName("experience")] long Experience) : Happening;

    /// <summary>A storyline a quest belonged to, named once per campaign per session.</summary>
    public sealed record Campaign(
        [property: JsonPropertyName("name")] string Name,
        [property: JsonPropertyName("summary")] string? Summary) : Happening;

    public sealed record Levelled(
        [property: JsonPropertyName("level")] int Level,
        [property: JsonPropertyName("zone")] string Zone) : Happening;

    /// <summary>Died, and where the log caught it, to what.</summary>
    public sealed record Died(
        [property: JsonPropertyName("zone")] string Zone,
        [property: JsonPropertyName("subzone")] string? Subzone,
        [property: JsonPropertyName("to")] string? To) : Happening;

    /// <summary>Entered an instance: a dungeon, a raid, a scenario, a battleground.</summary>
    public sealed record Entered(
        [property: JsonPropertyName("name")] string Name,
        [property: JsonPropertyName("kind")] string Kind,
        [property: JsonPropertyName("group")] int Group) : Happening;

    /// <summary>The open world's difficulty changed, or was first seen.</summary>
    public sealed record WorldTier([property: JsonPropertyName("tier")] string Tier) : Happening;

    /// <summary>The weather turned, over the zone the character was in.</summary>
    public sealed record Weather(
        [property: JsonPropertyName("kind")] string Kind,
        [property: JsonPropertyName("zone")] string? Zone) : Happening;

    /// <summary>A keystone finished.</summary>
    public sealed record Keystone(
        [property: JsonPropertyName("dungeon")] string Dungeon,
        [property: JsonPropertyName("level")] int Level,
        [property: JsonPropertyName("in_time")] bool InTime,
        [property: JsonPropertyName("upgrades")] int Upgrades,
        [property: JsonPropertyName("seconds")] long Seconds) : Happening;

    /// <summary>A scenario or delve completed. The tier is the whole difference between one delve and another.</summary>
    public sealed record Scenario(
        [property: JsonPropertyName("name")] string Name,
        [property: JsonPropertyName("tier")] string? Tier) : Happening;

    /// <summary>Something rare, rare-elite or a world boss, killed and named.</summary>
    public sealed record Rare(
        [property: JsonPropertyName("name")] string Name,
        [property: JsonPropertyName("rank")] string Rank) : Happening;

    /// <summary>A profession that got better at something.</summary>
    public sealed record Practised(
        [property: JsonPropertyName("profession")] string Profession,
        [property: JsonPropertyName("skill")] int Skill) : Happening;

    /// <summary>Something better got worn. Only ever an upgrade.</summary>
    public sealed record Equipped(
        [property: JsonPropertyName("name")] string Name,
        [property: JsonPropertyName("item_level")] int ItemLevel,
        [property: JsonPropertyName("gained")] int Gained) : Happening;

    /// <summary>An appearance the account had never seen.</summary>
    public sealed record Appearance([property: JsonPropertyName("name")] string Name) : Happening;

    /// <summary>Who gave, or took, a quest.</summary>
    public sealed record Gave(
        [property: JsonPropertyName("who")] string Who,
        [property: JsonPropertyName("quest")] long? Quest,
        [property: JsonPropertyName("creature")] long? Creature) : Happening;

    /// <summary>Something an NPC said unbidden, and who said it. Never a player.</summary>
    public sealed record Said(
        [property: JsonPropertyName("who")] string Who,
        [property: JsonPropertyName("line")] string Line) : Happening;

    /// <summary>What an NPC said when the player clicked them.</summary>
    public sealed record Told(
        [property: JsonPropertyName("who")] string Who,
        [property: JsonPropertyName("line")] string Line) : Happening;

    /// <summary>A cutscene played, and where. The movie id is set only for a pre-rendered one.</summary>
    public sealed record Cutscene(
        [property: JsonPropertyName("zone")] string Zone,
        [property: JsonPropertyName("movie")] long? Movie) : Happening;

    /// <summary>An auction that came back unsold: the only evidence anywhere that something did not sell.</summary>
    public sealed record Expired([property: JsonPropertyName("what")] string What) : Happening;

    /// <summary>A recipe learned.</summary>
    public sealed record Learned([property: JsonPropertyName("name")] string Name) : Happening;

    /// <summary>Money moved, and what it was in front of at the time.</summary>
    public sealed record Coin(
        [property: JsonPropertyName("purpose")] Purpose Purpose,
        [property: JsonPropertyName("amount")] long Amount,
        [property: JsonPropertyName("incoming")] bool Incoming) : Happening;

    /// <summary>Something was made. Counted per recipe, which the game does not do.</summary>
    public sealed record Crafted(
        [property: JsonPropertyName("recipe")] long Recipe,
        [property: JsonPropertyName("name")] string Name) : Happening;

    /// <summary>A flight path taken, and where from.</summary>
    public sealed record Flew([property: JsonPropertyName("from")] string From) : Happening;

    /// <summary>A screenshot the addon asked for, and what of. Matched to a file by time afterwards.</summary>
    public sealed record Pictured(
        [property: JsonPropertyName("what")] string What,
        [property: JsonPropertyName("subject")] string Subject) : Happening;

    /// <summary>A boss that died.</summary>
    public sealed record Felled([property: JsonPropertyName("name")] string Name) : Happening;

    /// <summary>An encounter that ended, won or lost.</summary>
    public sealed record Fought(
        [property: JsonPropertyName("name")] string Name,
        [property: JsonPropertyName("won")] bool Won) : Happening;

    /// <summary>How far a wipe got: the boss's health, in percent, when the pull ended.</summary>
    public sealed record Wiped(
        [property: JsonPropertyName("name")] string Name,
        [property: JsonPropertyName("remaining")] int Remaining) : Happening;

    public sealed record Earned(
        [property: JsonPropertyName("achievement")] long Achievement,
        [property: JsonPropertyName("name")] string Name) : Happening;

    /// <summary>A mount, pet or toy that arrived.</summary>
    public sealed record Acquired(
        [property: JsonPropertyName("kind")] Acquisition Kind,
        [property: JsonPropertyName("name")] string Name) : Happening;

    /// <summary>Loot worth mentioning: rare quality and above.</summary>
    public sealed record Looted(
        [property: JsonPropertyName("item")] long Item,
        [property: JsonPropertyName("name")] string Name,
        [property: JsonPropertyName("quality")] int Quality) : Happening;

    /// <summary>The auction house sent money. The subject line carries the item's name.</summary>
    public sealed record Sold(
        [property: JsonPropertyName("subject")] string Subject,
        [property: JsonPropertyName("money")] long Money) : Happening;

    /// <summary>Somebody who was in the party.</summary>
    public sealed record Alongside([property: JsonPropertyName("name")] string Name) : Happening;
}

/// <summary>Serde's externally tagged enum: one object with one property, the variant's name, whose value is the variant's fields.</summary>
public sealed class HappeningConverter : JsonConverter<Happening>
{
    private static readonly Dictionary<string, Type> Variants = typeof(Happening)
        .GetNestedTypes()
        .Where(type => type.IsSubclassOf(typeof(Happening)))
        .ToDictionary(type => type.Name, StringComparer.Ordinal);

    public override Happening Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        if (reader.TokenType != JsonTokenType.StartObject)
        {
            throw new JsonException("a happening is a tagged object");
        }
        reader.Read();
        var tag = reader.GetString() ?? throw new JsonException("a happening has a tag");
        reader.Read();
        if (!Variants.TryGetValue(tag, out var variant))
        {
            throw new JsonException($"not a happening: {tag}");
        }
        var value = (Happening?)JsonSerializer.Deserialize(ref reader, variant, options) ?? throw new JsonException("an empty happening");
        reader.Read();
        return value;
    }

    public override void Write(Utf8JsonWriter writer, Happening value, JsonSerializerOptions options)
    {
        writer.WriteStartObject();
        writer.WritePropertyName(value.GetType().Name);
        JsonSerializer.Serialize(writer, value, value.GetType(), options);
        writer.WriteEndObject();
    }
}

/// <summary>The one way an instant is written where the Rust's <c>to_rfc3339</c> is the spelling that must match.</summary>
public static class Stamps
{
    /// <summary>chrono's <c>to_rfc3339()</c> on a UTC instant: <c>2026-08-03T19:00:00+00:00</c>. This is the session key column, so it must match to the character.</summary>
    public static string Rfc3339WithOffset(DateTimeOffset at) =>
        at.ToUniversalTime().ToString("yyyy-MM-dd'T'HH:mm:ss'+00:00'", CultureInfo.InvariantCulture);
}
