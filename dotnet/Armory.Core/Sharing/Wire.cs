using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;

namespace Armory.Sharing;

/// <summary>
/// One row, on its way between machines.
/// </summary>
/// <remarks>
/// <c>key</c> and <c>fields</c> are positional, in the order the table's key
/// and columns give. The names are already agreed by both ends through
/// <see cref="Tables"/>, and repeating them on every row of a fifty-thousand
/// row first push is most of the bytes.
/// </remarks>
public sealed record Row
{
    /// <summary>
    /// The table's name, not the enum's, so a build that has never heard of a
    /// scope can still say which one it skipped.
    /// </summary>
    [JsonPropertyName("scope")]
    public required string Scope { get; init; }

    [JsonPropertyName("key")]
    public required List<JsonNode?> Key { get; init; }

    /// <summary>Absent when the row is gone.</summary>
    [JsonPropertyName("fields")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public List<JsonNode?>? Fields { get; init; }

    /// <summary>
    /// A row that is no longer there. Deletion is rare here and every case of
    /// it is somebody saying so out loud; it is never inferred from absence.
    /// </summary>
    [JsonIgnore]
    public bool IsGone => Fields is null;
}

/// <summary>A batch, in <c>seq</c> order.</summary>
public sealed record Parcel
{
    [JsonPropertyName("rows")]
    public List<Row> Rows { get; init; } = [];
}

/// <summary>What applying a parcel did.</summary>
public readonly record struct Applied(int Written, int Removed, int Kept, int Unreadable)
{
    public bool IsEmpty => this == default;

    /// <summary>Rows that landed or left.</summary>
    public int Count => Written + Removed;

    public static Applied operator +(Applied a, Applied b) =>
        new(a.Written + b.Written, a.Removed + b.Removed, a.Kept + b.Kept, a.Unreadable + b.Unreadable);

    public Applied Add(Applied other) => this + other;
}

/// <summary>
/// Why a pass could not finish. One string, because every one of them means
/// the same thing to the caller: try again later. The text is for the sync
/// page and a log line, not for branching.
/// </summary>
public sealed record SyncError(string Message)
{
    public override string ToString() => Message;
}

/// <summary>What a pull brought back, and where to ask from next time.</summary>
public sealed record Pulled
{
    [JsonPropertyName("parcel")]
    public Parcel Parcel { get; init; } = new();

    /// <summary>The cursor to send as <c>since</c> next time.</summary>
    [JsonPropertyName("cursor")]
    public long Cursor { get; init; }

    /// <summary>
    /// Whether the server has more above this cursor. A first sync is many
    /// batches, and a client that stopped after one would look finished.
    /// </summary>
    [JsonPropertyName("more")]
    public bool More { get; init; }
}

/// <summary>
/// The other side, whatever is carrying it. The core does not learn what a
/// socket is: the shell answers this over HTTP, and a test answers it
/// in-process.
/// </summary>
public interface IRemote
{
    /// <summary>Send these up. The server applies them with the same rules this store would.</summary>
    Result<Applied, SyncError> Push(Parcel parcel);

    /// <summary>Everything above <paramref name="since"/> that this machine did not write itself.</summary>
    Result<Pulled, SyncError> Pull(long since, int limit);

    /// <summary>Block until the server has something above <paramref name="since"/>, or gives up waiting.</summary>
    Result<bool, SyncError> Wait(long since);
}

public static class Wire
{
    /// <summary>
    /// The largest cached body worth carrying across the network. A connected
    /// realm's auction dump is tens of megabytes, replaced hourly, and already
    /// reduced into rows that do travel; a body over this stays where it is.
    /// </summary>
    public const int MaxBody = 4 * 1024 * 1024;

    /// <summary>
    /// How large a batch may get before it is sent short. A batch is bounded
    /// by a row count and by this, and this is the bound that matters:
    /// <c>response</c> rows carry whole bodies. A batch always carries at
    /// least one row, so a single body cannot wedge the queue.
    /// </summary>
    public const int MaxParcel = 16 * 1024 * 1024;

    /// <summary>The options every wire shape is written and read with.</summary>
    public static JsonSerializerOptions Json { get; } = new(JsonSerializerDefaults.General)
    {
        DefaultIgnoreCondition = JsonIgnoreCondition.Never,
    };

    /// <summary>
    /// Roughly what a row will weigh on the wire. The point of the exercise is
    /// the one field that can be enormous, so precision elsewhere buys nothing.
    /// </summary>
    public static int Weight(Row row)
    {
        var fields = 0;
        foreach (var value in row.Fields ?? [])
        {
            fields += value switch
            {
                null => 4,
                JsonValue text when text.TryGetValue<string>(out var s) => s.Length + 2,
                _ => value.ToJsonString().Length,
            };
        }
        return row.Scope.Length + row.Key.Count * 8 + fields + 32;
    }

    /// <summary>Standard base64 with padding, for the one byte column.</summary>
    public static string EncodeBase64(ReadOnlySpan<byte> bytes) => Convert.ToBase64String(bytes);

    /// <summary>
    /// Null for anything that is not base64, rather than a partial decode. A
    /// body half read is worse than one absent: the cache would answer with it
    /// and every parser downstream would report a Blizzard problem.
    /// </summary>
    public static byte[]? DecodeBase64(string text)
    {
        var cleaned = text.Replace("\n", "", StringComparison.Ordinal).Replace("\r", "", StringComparison.Ordinal);
        var buffer = new byte[cleaned.Length / 4 * 3 + 3];
        return Convert.TryFromBase64String(cleaned, buffer, out var written) ? buffer[..written] : null;
    }
}
