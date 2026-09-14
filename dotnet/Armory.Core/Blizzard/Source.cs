using System.Globalization;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace Armory.Blizzard;

/// <summary>
/// Who is being asked. The seam between "what to ask" and "asking it": every
/// source is a pair of pure functions, one builds a <see cref="Request"/>, the
/// other turns a response body into an <see cref="Outcome{T}"/>. Nothing in
/// the core opens a socket, which is what makes every source, every malformed
/// response and every failure mode testable from a recorded fixture.
/// </summary>
public enum SourceId
{
    /// <summary>The OAuth endpoints: token exchange and refresh.</summary>
    BattleNetOAuth,

    /// <summary><c>/profile/...</c>: this account and its characters.</summary>
    BlizzardProfile,

    /// <summary><c>/data/...</c>: the catalogue, and the auction house.</summary>
    BlizzardGameData,

    /// <summary>
    /// The local llama-server, which writes the journal entries. The only
    /// source on this machine, and the only one whose status codes mean
    /// something other than what Blizzard's mean.
    /// </summary>
    Journal,
}

public static class SourceIdExtensions
{
    public static IReadOnlyList<SourceId> All { get; } = [SourceId.BattleNetOAuth, SourceId.BlizzardProfile, SourceId.BlizzardGameData, SourceId.Journal];

    public static string Label(this SourceId source) => source switch
    {
        SourceId.BattleNetOAuth => "Battle.net sign-in",
        SourceId.BlizzardProfile => "Character profiles",
        SourceId.BlizzardGameData => "Game data",
        _ => "Journal entries",
    };

    /// <summary>
    /// Whether this is one of Blizzard's endpoints. The HTTP client reads a
    /// handful of status codes as things that are true of Blizzard and false
    /// everywhere else: a 403 is a privacy setting there.
    /// </summary>
    public static bool IsBlizzard(this SourceId source) => source != SourceId.Journal;
}

public enum Method
{
    Get,
    Post,
}

/// <summary>One HTTP call, described but not made.</summary>
public sealed record Request
{
    public required SourceId Source { get; init; }

    public Method Method { get; init; } = Method.Get;

    public required string Url { get; init; }

    public List<(string Name, string Value)> Headers { get; init; } = [];

    /// <summary>Form-encoded body, for the token endpoint. Nothing else here posts.</summary>
    public string? Body { get; init; }

    public static Request Get(SourceId source, string url) => new() { Source = source, Url = url };

    public static Request Post(SourceId source, string url, string body) => new()
    {
        Source = source,
        Method = Method.Post,
        Url = url,
        Headers = [("Content-Type", "application/x-www-form-urlencoded")],
        Body = body,
    };

    public Request Header(string name, string value) => this with { Headers = [.. Headers, (name, value)] };

    /// <summary>Attach the bearer token. Blizzard stopped accepting a query token on 2024-09-30; the header is the only way.</summary>
    public Request Bearer(string token) => Header("Authorization", $"Bearer {token}");

    /// <summary>
    /// Ask the server to answer 304 if nothing has changed since the stamp.
    /// The whole reason syncing twenty-three characters is affordable.
    /// </summary>
    public Request IfModifiedSince(string stamp) => Header("If-Modified-Since", stamp);

    /// <summary>The cache key: the URL. Two requests that differ only in a header are the same question asked with a fresher token.</summary>
    public string CacheKey => Url;

    public string? HeaderValue(string name) => Headers.FirstOrDefault(header => string.Equals(header.Name, name, StringComparison.OrdinalIgnoreCase)).Value;

    public bool Equals(Request? other) =>
        other is not null && Source == other.Source && Method == other.Method && Url == other.Url && Body == other.Body && Headers.SequenceEqual(other.Headers);

    public override int GetHashCode() => HashCode.Combine(Source, Method, Url, Body);
}

/// <summary>Why a source could not be used.</summary>
public abstract record Reason
{
    public sealed record Timeout : Reason;

    public sealed record RateLimited : Reason;

    public sealed record Http(int Code) : Reason;

    /// <summary>The body arrived and did not parse.</summary>
    public sealed record Malformed(string What) : Reason;

    /// <summary>Never asked: no client registered, no token, or switched off.</summary>
    public sealed record NotConfigured(string What) : Reason;

    /// <summary>The token expired or the user withdrew consent. The fix is to sign in again, not to set the application up.</summary>
    public sealed record Unauthorised(string What) : Reason;

    /// <summary>Third-party data sharing is off on the Battle.net account. Saying "HTTP 403" would send someone hunting for a bug that is a checkbox.</summary>
    public sealed record SharingDisabled : Reason;

    /// <summary>The service answered, said no, and said why in its own words. Neither a fault nor something to retry.</summary>
    public sealed record Declined(string What) : Reason;

    public sealed record Network(string What) : Reason;

    public sealed override string ToString() => this switch
    {
        Timeout => "timed out",
        RateLimited => "rate limited",
        Http http => string.Create(CultureInfo.InvariantCulture, $"HTTP {http.Code}"),
        Malformed malformed => $"unreadable response: {malformed.What}",
        NotConfigured missing => $"not set up: {missing.What}",
        Unauthorised lapsed => $"not signed in: {lapsed.What}",
        SharingDisabled => "this account has third-party data sharing turned off in its Battle.net privacy settings",
        Declined declined => declined.What,
        Network network => network.What,
        _ => "unknown",
    };
}

/// <summary>
/// What a source came back with.
/// </summary>
/// <remarks>
/// The distinction between <see cref="Empty"/> and <see cref="Stale"/> is the
/// whole reason this is not a nullable or a result. A character who has
/// collected no mounts and a mounts parser that has stopped understanding the
/// response both produce an empty list; if they are the same value, a broken
/// parser silently empties a collection and makes a run look finished.
/// </remarks>
public abstract record Outcome<T>
{
    /// <summary>The source answered and had something.</summary>
    public sealed record Found(T Value) : Outcome<T>;

    /// <summary>The source answered and genuinely has nothing.</summary>
    public sealed record Empty : Outcome<T>;

    /// <summary>Nothing has changed since the stamp we sent. Not an error and not an answer.</summary>
    public sealed record Unchanged : Outcome<T>;

    /// <summary>The source could not be reached or refused.</summary>
    public sealed record Unusable(Reason Why) : Outcome<T>;

    /// <summary>The source answered, but not in a shape we recognise.</summary>
    public sealed record Stale(Reason Why) : Outcome<T>;

    public bool IsFound => this is Found;

    public T? FoundValue => this is Found found ? found.Value : default;

    /// <summary>
    /// Why this source contributed nothing, if it should be reported as a gap.
    /// Empty and Unchanged are not gaps. The reason comes back whole because
    /// the caller has to tell "never set up" from "sign-in lapsed" from
    /// "actually broke".
    /// </summary>
    public Reason? Gap() => this switch
    {
        Unusable unusable => unusable.Why,
        Stale stale => new Reason.Malformed($"check failed — {stale.Why}"),
        _ => null,
    };

    public Outcome<TNext> Map<TNext>(Func<T, TNext> map) => this switch
    {
        Found found => new Outcome<TNext>.Found(map(found.Value)),
        Empty => new Outcome<TNext>.Empty(),
        Unchanged => new Outcome<TNext>.Unchanged(),
        Unusable unusable => new Outcome<TNext>.Unusable(unusable.Why),
        Stale stale => new Outcome<TNext>.Stale(stale.Why),
        _ => throw new InvalidOperationException("not an outcome"),
    };
}

public static class Outcomes
{
    /// <summary>Found when the collection has anything in it, Empty when it does not.</summary>
    public static Outcome<List<T>> OfCollection<T>(List<T> items) =>
        items.Count == 0 ? new Outcome<List<T>>.Empty() : new Outcome<List<T>>.Found(items);

    /// <summary>
    /// Parse a body as JSON, or say why it is stale. A source that answers
    /// 200 with something that is not JSON has changed shape under us; that
    /// is the definition of stale, and it is never Empty.
    /// </summary>
    public static Result<JsonNode, Outcome<T>> ParseJson<T>(SourceId source, byte[] body)
    {
        try
        {
            var node = JsonNode.Parse(body);
            return node is null
                ? Result<JsonNode, Outcome<T>>.Err(new Outcome<T>.Stale(new Reason.Malformed($"{source.Label()} sent non-JSON: null")))
                : Result<JsonNode, Outcome<T>>.Ok(node);
        }
        catch (JsonException error)
        {
            return Result<JsonNode, Outcome<T>>.Err(new Outcome<T>.Stale(new Reason.Malformed($"{source.Label()} sent non-JSON: {error.Message}")));
        }
    }
}

/// <summary>The small readers every parser here uses on a JSON node.</summary>
public static class JsonReading
{
    public static JsonNode? At(this JsonNode? node, string key) => node is JsonObject obj ? obj[key] : null;

    public static string? Str(this JsonNode? node) => node is JsonValue value && value.TryGetValue<string>(out var text) ? text : null;

    public static long? Int(this JsonNode? node)
    {
        if (node is not JsonValue value)
        {
            return null;
        }
        if (value.TryGetValue<long>(out var integer))
        {
            return integer;
        }
        if (value.TryGetValue<double>(out var real) && double.IsFinite(real))
        {
            return (long)real;
        }
        return null;
    }

    public static double? Num(this JsonNode? node) =>
        node is JsonValue value && value.TryGetValue<double>(out var real) ? real : null;

    public static bool? Flag(this JsonNode? node) =>
        node is JsonValue value && value.TryGetValue<bool>(out var flag) ? flag : null;

    public static IEnumerable<JsonNode?> Items(this JsonNode? node) => node is JsonArray array ? array : [];

    /// <summary>The display name out of one of Blizzard's <c>{key, name, id}</c> references.</summary>
    public static string Named(this JsonNode? node) => node.At("name").Str() ?? "";
}
