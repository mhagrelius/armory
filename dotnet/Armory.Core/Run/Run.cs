using System.Globalization;
using System.Text.Json;
using System.Text.Json.Serialization;
using Armory.Roster;

namespace Armory.Run;

/// <summary>
/// Where an achievement stands relative to this run, as distinct from where it
/// stands relative to the account.
/// </summary>
/// <remarks>
/// The account says done. The run has not done it. Reading a completion flag
/// would produce an empty backlog on day one. But most of the account is not
/// a problem: an achievement nobody has earned behaves as designed, and one an
/// enrolled character earned is the run's. Only one earned before the
/// baseline by someone outside the cohort is <see cref="Poisoned"/>, because
/// a second character completing an account-wide achievement produces no
/// signal at all. Only poisoned goals reach the expensive machinery.
/// </remarks>
[JsonConverter(typeof(StandingConverter))]
public abstract record Standing
{
    /// <summary>Nobody on the account has it. The flag works normally from here.</summary>
    public sealed record Unearned : Standing;

    /// <summary>Earned after the baseline, so it belongs to the run by definition: nobody but the cohort has been playing.</summary>
    public sealed record EarnedDuringRun(DateTimeOffset At) : Standing;

    /// <summary>Earned before the baseline, by a character who is in the cohort.</summary>
    public sealed record EarnedByCohort(CharacterKey By) : Standing;

    /// <summary>
    /// Earned before the baseline by someone outside the cohort, or by someone
    /// we cannot identify. <see cref="By"/> is null when the addon has not
    /// reported attribution, which is deliberately pessimistic: without the
    /// addon every already-earned achievement has to be assumed poisoned.
    /// </summary>
    public sealed record Poisoned(CharacterKey? By) : Standing;

    /// <summary>Decide where an achievement stands. <paramref name="earnedBy"/> is null when the addon has not run.</summary>
    public static Standing Classify(DateTimeOffset? completedAt, CharacterKey? earnedBy, Cohort cohort, DateTimeOffset baselineTakenAt)
    {
        if (completedAt is not { } completed)
        {
            return new Unearned();
        }
        // Anything finished since the baseline belongs to the run.
        if (completed >= baselineTakenAt)
        {
            return new EarnedDuringRun(completed);
        }
        return earnedBy switch
        {
            { } key when cohort.Contains(key) => new EarnedByCohort(key),
            { } key => new Poisoned(key),
            null => new Poisoned(null),
        };
    }

    /// <summary>Whether the run already has this, with no further work.</summary>
    public bool IsSettled => this is EarnedDuringRun or EarnedByCohort;

    /// <summary>Whether the expensive classification in <see cref="Bucket"/> applies.</summary>
    public bool IsPoisoned => this is Poisoned;
}

/// <summary>Serde's externally tagged form: <c>"Unearned"</c>, <c>{"EarnedDuringRun":{"at":"…"}}</c>, <c>{"Poisoned":{"by":null}}</c>.</summary>
public sealed class StandingConverter : JsonConverter<Standing>
{
    public override Standing Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        if (reader.TokenType == JsonTokenType.String)
        {
            return reader.GetString() == "Unearned" ? new Standing.Unearned() : throw new JsonException("not a standing");
        }
        if (reader.TokenType != JsonTokenType.StartObject)
        {
            throw new JsonException("a standing is a name or an object");
        }
        reader.Read();
        var tag = reader.GetString();
        reader.Read();
        var payload = JsonDocument.ParseValue(ref reader).RootElement;
        reader.Read();
        return tag switch
        {
            "EarnedDuringRun" => new Standing.EarnedDuringRun(payload.GetProperty("at").GetDateTimeOffset()),
            "EarnedByCohort" => new Standing.EarnedByCohort(payload.GetProperty("by").Deserialize<CharacterKey>(options)!),
            "Poisoned" => new Standing.Poisoned(payload.GetProperty("by").ValueKind == JsonValueKind.Null ? null : payload.GetProperty("by").Deserialize<CharacterKey>(options)),
            _ => throw new JsonException($"not a standing: {tag}"),
        };
    }

    public override void Write(Utf8JsonWriter writer, Standing value, JsonSerializerOptions options)
    {
        switch (value)
        {
            case Standing.Unearned:
                writer.WriteStringValue("Unearned");
                break;
            case Standing.EarnedDuringRun earned:
                writer.WriteStartObject();
                writer.WriteStartObject("EarnedDuringRun");
                writer.WriteString("at", Stamps.Rfc3339(earned.At));
                writer.WriteEndObject();
                writer.WriteEndObject();
                break;
            case Standing.EarnedByCohort byCohort:
                writer.WriteStartObject();
                writer.WriteStartObject("EarnedByCohort");
                writer.WritePropertyName("by");
                JsonSerializer.Serialize(writer, byCohort.By, options);
                writer.WriteEndObject();
                writer.WriteEndObject();
                break;
            case Standing.Poisoned poisoned:
                writer.WriteStartObject();
                writer.WriteStartObject("Poisoned");
                writer.WritePropertyName("by");
                if (poisoned.By is null)
                {
                    writer.WriteNullValue();
                }
                else
                {
                    JsonSerializer.Serialize(writer, poisoned.By, options);
                }
                writer.WriteEndObject();
                writer.WriteEndObject();
                break;
            default:
                throw new JsonException("not a standing");
        }
    }
}

/// <summary>Why a poisoned goal has been taken out of the run entirely.</summary>
[JsonConverter(typeof(JsonStringEnumConverter<Exclusion>))]
public enum Exclusion
{
    /// <summary>An account-wide collectible the account already owns. Not a difficulty rating; an impossibility.</summary>
    AlreadyOwned,

    /// <summary>A Feat of Strength, or content that no longer exists.</summary>
    Unrepeatable,

    /// <summary>Nothing measures it and nobody could honestly attest to it either.</summary>
    Unmeasurable,

    /// <summary>The user excluded it.</summary>
    ByHand,
}

public static class ExclusionExtensions
{
    public static string Label(this Exclusion exclusion) => exclusion switch
    {
        Exclusion.AlreadyOwned => "already collected on this account",
        Exclusion.Unrepeatable => "cannot be earned again",
        Exclusion.Unmeasurable => "no way to measure progress",
        _ => "excluded by you",
    };
}

/// <summary>How a poisoned goal is tracked, once it is known to be poisoned. Unpoisoned goals never enter this classification.</summary>
[JsonConverter(typeof(BucketConverter))]
public abstract record Bucket
{
    /// <summary>Every criterion resolves against per-character data. A bar can honestly be drawn.</summary>
    public sealed record Observable : Bucket;

    /// <summary>No per-character signal exists, but a person knows whether they did it.</summary>
    public sealed record Attestable : Bucket;

    /// <summary>Out of the run.</summary>
    public sealed record Excluded(Exclusion Why) : Bucket;
}

/// <summary>Serde's form: <c>"Observable"</c>, <c>"Attestable"</c>, <c>{"Excluded":"AlreadyOwned"}</c>.</summary>
public sealed class BucketConverter : JsonConverter<Bucket>
{
    public override Bucket Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        if (reader.TokenType == JsonTokenType.String)
        {
            return reader.GetString() switch
            {
                "Observable" => new Bucket.Observable(),
                "Attestable" => new Bucket.Attestable(),
                var other => throw new JsonException($"not a bucket: {other}"),
            };
        }
        if (reader.TokenType != JsonTokenType.StartObject)
        {
            throw new JsonException("a bucket is a name or an object");
        }
        reader.Read();
        if (reader.GetString() != "Excluded")
        {
            throw new JsonException("a bucket object carries Excluded");
        }
        reader.Read();
        var why = Enum.Parse<Exclusion>(reader.GetString() ?? "");
        reader.Read();
        return new Bucket.Excluded(why);
    }

    public override void Write(Utf8JsonWriter writer, Bucket value, JsonSerializerOptions options)
    {
        switch (value)
        {
            case Bucket.Observable:
                writer.WriteStringValue("Observable");
                break;
            case Bucket.Attestable:
                writer.WriteStringValue("Attestable");
                break;
            case Bucket.Excluded excluded:
                writer.WriteStartObject();
                writer.WriteString("Excluded", excluded.Why.ToString());
                writer.WriteEndObject();
                break;
            default:
                throw new JsonException("not a bucket");
        }
    }
}

/// <summary>Someone saying they did it, because nothing else can say so.</summary>
public sealed record Attestation
{
    [JsonPropertyName("character")]
    public required CharacterKey Character { get; init; }

    [JsonPropertyName("at")]
    public DateTimeOffset At { get; init; }
}

/// <summary>One thing the run is trying to do.</summary>
public sealed record Goal
{
    [JsonPropertyName("achievement_id")]
    public long AchievementId { get; init; }

    [JsonPropertyName("standing")]
    public Standing Standing { get; init; } = new Standing.Unearned();

    /// <summary>Only meaningful when the standing is poisoned.</summary>
    [JsonPropertyName("bucket")]
    public Bucket Bucket { get; init; } = new Bucket.Observable();

    /// <summary>Only meaningful when the bucket is attestable.</summary>
    [JsonPropertyName("attestation")]
    public Attestation? Attestation { get; init; }

    /// <summary>Whichever enrolled character the evaluation was measured against. Not stored: recomputed on every sync.</summary>
    [JsonIgnore]
    public CharacterKey? Nearest { get; init; }

    /// <summary>Only meaningful when the bucket is observable. Not stored: a stale copy would outlive the data it came from.</summary>
    [JsonIgnore]
    public Evaluation? Evaluation { get; init; }

    /// <summary>Whether the run has done this.</summary>
    [JsonIgnore]
    public bool IsDone
    {
        get
        {
            if (Standing.IsSettled)
            {
                return true;
            }
            if (!Standing.IsPoisoned)
            {
                return false;
            }
            return Bucket switch
            {
                // An excluded goal is not done, it is gone. Counting it as
                // done would inflate the run as badly as an alt's reputation.
                Bucket.Excluded => false,
                Bucket.Attestable => Attestation is not null,
                _ => Evaluation is { IsComplete: true },
            };
        }
    }

    /// <summary>Whether this goal is part of what the run is measured against.</summary>
    [JsonIgnore]
    public bool Counts => Bucket is not Bucket.Excluded || !Standing.IsPoisoned;

    /// <summary>How far along, for a progress bar. Null when no honest bar can be drawn.</summary>
    public double? Fraction()
    {
        if (IsDone)
        {
            return 1.0;
        }
        return Bucket is Bucket.Observable && Evaluation is { Observable: true, Inherited: false } evaluation
            ? evaluation.Fraction
            : null;
    }
}

/// <summary>One achievement already complete at the baseline, and when. The Rust's <c>(u32, DateTime)</c> pair, a JSON array.</summary>
[JsonConverter(typeof(CompletedConverter))]
public sealed record Completed(long Id, DateTimeOffset At);

public sealed class CompletedConverter : JsonConverter<Completed>
{
    public override Completed Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        if (reader.TokenType != JsonTokenType.StartArray)
        {
            throw new JsonException("a completion is a pair");
        }
        reader.Read();
        var id = reader.GetInt64();
        reader.Read();
        var at = reader.GetDateTimeOffset();
        reader.Read();
        return new Completed(id, at);
    }

    public override void Write(Utf8JsonWriter writer, Completed value, JsonSerializerOptions options)
    {
        writer.WriteStartArray();
        writer.WriteNumberValue(value.Id);
        writer.WriteStringValue(Stamps.Rfc3339(value.At));
        writer.WriteEndArray();
    }
}

/// <summary>
/// What the account had when the run began. Immutable once taken: this is
/// what makes "already owned, therefore excluded" a decidable question rather
/// than a moving one.
/// </summary>
public sealed record Baseline
{
    [JsonPropertyName("taken_at")]
    public DateTimeOffset TakenAt { get; init; }

    /// <summary>Mount, pet and toy ids the account already had.</summary>
    [JsonPropertyName("collected")]
    public List<long> Collected { get; init; } = [];

    /// <summary>Achievement ids already complete, and when.</summary>
    [JsonPropertyName("completed")]
    public List<Completed> Completed { get; init; } = [];

    public bool Equals(Baseline? other) =>
        other is not null && TakenAt == other.TakenAt && Collected.SequenceEqual(other.Collected) && Completed.SequenceEqual(other.Completed);

    public override int GetHashCode() => TakenAt.GetHashCode();
}

/// <summary>What the run has done, and what it has decided not to try.</summary>
public readonly record struct Progress(long Done, long Counted, long Excluded, long AwaitingAttestation)
{
    public double Fraction => Counted == 0 ? 0.0 : Done / (double)Counted;
}

/// <summary>A pass through the account's content, with its own idea of what is done.</summary>
public sealed record Run
{
    [JsonPropertyName("name")]
    public string Name { get; init; } = "";

    [JsonPropertyName("baseline")]
    public required Baseline Baseline { get; init; }

    [JsonPropertyName("cohort")]
    public Cohort Cohort { get; init; } = new();

    [JsonPropertyName("goals")]
    public List<Goal> Goals { get; init; } = [];

    /// <summary>
    /// Which character each closed goal is owed to, counted. An attestation
    /// wins over the measured nearest character, because a stated answer
    /// beats an inferred one. A goal credited to nobody is left out rather
    /// than shared around, so the counts are a floor.
    /// </summary>
    public Dictionary<CharacterKey, long> Credited()
    {
        var credit = new Dictionary<CharacterKey, long>();
        foreach (var goal in Goals)
        {
            if (!goal.Counts || !goal.IsDone)
            {
                continue;
            }
            var who = goal.Attestation?.Character ?? goal.Nearest;
            if (who is not null)
            {
                credit[who] = credit.GetValueOrDefault(who) + 1;
            }
        }
        return credit;
    }

    /// <summary>Count the run up. Excluded goals are outside the denominator, not zeroes inside it.</summary>
    public Progress Progress()
    {
        long done = 0, counted = 0, excluded = 0, awaiting = 0;
        foreach (var goal in Goals)
        {
            if (!goal.Counts)
            {
                excluded++;
                continue;
            }
            counted++;
            if (goal.IsDone)
            {
                done++;
            }
            else if (goal.Bucket is Bucket.Attestable && goal.Standing.IsPoisoned)
            {
                awaiting++;
            }
        }
        return new Progress(done, counted, excluded, awaiting);
    }

    /// <summary>The goals that need the expensive treatment. Everything else is a flag read.</summary>
    public IEnumerable<Goal> Poisoned() => Goals.Where(goal => goal.Standing.IsPoisoned);

    public bool Equals(Run? other) =>
        other is not null && Name == other.Name && Baseline == other.Baseline && Cohort == other.Cohort && Goals.SequenceEqual(other.Goals);

    public override int GetHashCode() => HashCode.Combine(Name, Baseline, Goals.Count);
}

/// <summary>The one way an instant is written into JSON here, so the Rust reads it and a key derived from it agrees.</summary>
public static class Stamps
{
    public static string Rfc3339(DateTimeOffset at) =>
        at.ToUniversalTime().ToString("yyyy-MM-dd'T'HH:mm:ss'Z'", CultureInfo.InvariantCulture);
}
