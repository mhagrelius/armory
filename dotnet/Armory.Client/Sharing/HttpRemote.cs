using System.Globalization;
using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using Armory.Sharing;

namespace Armory.Client.Sharing;

/// <summary>One account the server is holding, and how much it is.</summary>
public sealed record Held
{
    [JsonPropertyName("name")]
    public string Name { get; init; } = "";

    [JsonPropertyName("rows")]
    public long Rows { get; init; }
}

/// <summary>
/// Talking to <c>armory-server</c>, and nothing else.
/// </summary>
/// <remarks>
/// <para><see cref="IRemote"/> is an interface so that the core never learns
/// what a socket is; this is the plain HTTP answering it. Blunt by design:
/// send a request, read the answer, turn a refusal into a sentence.</para>
/// <para>It is its own <see cref="HttpClient"/> rather than the Blizzard one,
/// because that client exists to obey Blizzard's quota, and a fifty-thousand
/// row first pass against a server on the tailnet that has no quota at all
/// must not spend the budget a roster sync needs.</para>
/// <para><b>Everything here blocks, and none of it touches the store.</b> It
/// is handed a parcel and gives back an answer; the shell runs it on a worker
/// and does every read and write on the thread that owns the database.</para>
/// </remarks>
public sealed class HttpRemote : IRemote, IDisposable
{
    /// <summary>How long an ordinary call may take. Generous, because a first push is megabytes over a tailnet.</summary>
    public static readonly TimeSpan Timeout = TimeSpan.FromSeconds(60);

    /// <summary>
    /// How long to hold a parked <c>/wait</c>. Comfortably past the server's
    /// own fifty seconds: the server giving up is the normal end of a quiet
    /// wait and must not look like the network failing.
    /// </summary>
    public static readonly TimeSpan WaitTimeout = TimeSpan.FromSeconds(75);

    private const int ServerPort = 8084;

    private readonly HttpClient http;

    private HttpRemote(string address, HttpClient http)
    {
        Address = address;
        this.http = http;
    }

    /// <summary><c>host:port</c>, as the server is addressed.</summary>
    public string Address { get; }

    /// <summary>
    /// <c>http://host:port</c>, a token, this installation's id and the
    /// account on the server it belongs to. <paramref name="handler"/> is for
    /// a test that answers in-process; the application passes none.
    /// </summary>
    public static Result<HttpRemote, SyncError> Create(
        string url,
        string token,
        string machine,
        string account,
        HttpMessageHandler? handler = null)
    {
        return ParseAddress(url).Map(address =>
        {
            var http = handler is null ? new HttpClient() : new HttpClient(handler, disposeHandler: true);
            http.BaseAddress = new Uri($"http://{address}/");
            // Timeouts are per call, below; the client's own would cut a wait
            // short at the ordinary length.
            http.Timeout = System.Threading.Timeout.InfiniteTimeSpan;
            http.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Bearer", token.Trim());
            http.DefaultRequestHeaders.Add("X-Armory-Machine", machine);
            http.DefaultRequestHeaders.Add("X-Armory-Account", account.Trim());
            return new HttpRemote(address, http);
        });
    }

    /// <summary>
    /// The <c>host:port</c> an address names. <c>https://</c> is refused
    /// rather than quietly downgraded: accepting it and connecting in the
    /// clear would be the worst of the three possible behaviours. A port is
    /// not optional, because nothing serves this on 80.
    /// </summary>
    internal static Result<string, SyncError> ParseAddress(string url)
    {
        var trimmed = url.Trim();
        if (trimmed.StartsWith("https://", StringComparison.OrdinalIgnoreCase))
        {
            return Result<string, SyncError>.Err(new SyncError("this speaks plain HTTP; use http:// and a tailnet address"));
        }
        // The scheme comes off before the slashes do. The other order turns
        // `http://` into `http:`, which then looks like a host with a port.
        var host = trimmed.StartsWith("http://", StringComparison.OrdinalIgnoreCase) ? trimmed["http://".Length..] : trimmed;
        host = host.Trim('/');
        if (host.Length == 0)
        {
            return Result<string, SyncError>.Err(new SyncError("no server address"));
        }
        return Result<string, SyncError>.Ok(host.Contains(':', StringComparison.Ordinal) ? host : $"{host}:{ServerPort}");
    }

    public void Dispose() => http.Dispose();

    /// <summary>Every account this server holds, and how much each one is.</summary>
    public Result<List<Held>, SyncError> Accounts() => Call<List<Held>>(HttpMethod.Get, "/accounts", null, Timeout);

    /// <summary>
    /// Delete one, and everything in it. The name goes in the path and again
    /// in <c>confirm</c>, because the server refuses unless they match. There
    /// is no undo and no second copy on the server afterwards.
    /// </summary>
    public Result<Unit, SyncError> ForgetAccount(string name) =>
        Send(HttpMethod.Delete, $"/accounts/{Uri.EscapeDataString(name)}?confirm={Uri.EscapeDataString(name)}", null, Timeout).Map(_ => Unit.Value);

    /// <summary>
    /// Ask whether it is there at all, without sending anything. Uses
    /// <c>/health</c>, which needs no token, so a server that is up and a
    /// token that is wrong are two different answers rather than one.
    /// </summary>
    public Result<Unit, SyncError> Reachable() => Send(HttpMethod.Get, "/health", null, Timeout).Map(_ => Unit.Value);

    public Result<Applied, SyncError> Push(Parcel parcel)
    {
        byte[] body;
        try
        {
            body = JsonSerializer.SerializeToUtf8Bytes(parcel, Wire.Json);
        }
        catch (JsonException error)
        {
            return Result<Applied, SyncError>.Err(new SyncError($"could not write the parcel: {error.Message}"));
        }
        return Call<PushReport>(HttpMethod.Post, "/push", body, Timeout)
            .Map(report => new Applied(report.Written, report.Removed, report.Kept, report.Unreadable));
    }

    public Result<Pulled, SyncError> Pull(long since, int limit) =>
        Call<Pulled>(HttpMethod.Get, FormattableString.Invariant($"/pull?since={since}&limit={limit}"), null, Timeout);

    public Result<bool, SyncError> Wait(long since) =>
        Call<Waited>(HttpMethod.Get, FormattableString.Invariant($"/wait?since={since}"), null, WaitTimeout).Map(waited => waited.Changed);

    private Result<T, SyncError> Call<T>(HttpMethod method, string path, byte[]? body, TimeSpan timeout) =>
        Send(method, path, body, timeout).Then(answer =>
        {
            try
            {
                var value = JsonSerializer.Deserialize<T>(answer, Wire.Json);
                return value is null
                    ? Result<T, SyncError>.Err(new SyncError("could not read the answer: it was empty"))
                    : Result<T, SyncError>.Ok(value);
            }
            catch (JsonException error)
            {
                return Result<T, SyncError>.Err(new SyncError($"could not read the answer: {error.Message}"));
            }
        });

    private Result<byte[], SyncError> Send(HttpMethod method, string path, byte[]? body, TimeSpan timeout)
    {
        try
        {
            using var request = new HttpRequestMessage(method, path);
            if (body is not null)
            {
                request.Content = new ByteArrayContent(body);
                request.Content.Headers.ContentType = new MediaTypeHeaderValue("application/json");
            }
            using var cancel = new CancellationTokenSource(timeout);
            using var response = http.Send(request, HttpCompletionOption.ResponseContentRead, cancel.Token);
            using var stream = response.Content.ReadAsStream(cancel.Token);
            using var buffer = new MemoryStream();
            stream.CopyTo(buffer);
            return ReadAnswer((int)response.StatusCode, buffer.ToArray());
        }
        catch (HttpRequestException error)
        {
            return Result<byte[], SyncError>.Err(new SyncError($"could not reach {Address}: {error.Message}"));
        }
        catch (OperationCanceledException)
        {
            return Result<byte[], SyncError>.Err(new SyncError(FormattableString.Invariant($"no answer from {Address} within {timeout.TotalSeconds:0} seconds")));
        }
        catch (IOException error)
        {
            return Result<byte[], SyncError>.Err(new SyncError($"no answer: {error.Message}"));
        }
    }

    /// <summary>The body on success, and a refusal as a sentence.</summary>
    internal static Result<byte[], SyncError> ReadAnswer(int status, byte[] body) => status switch
    {
        200 => Result<byte[], SyncError>.Ok(body),
        // The one failure somebody can actually fix, so it says what to fix.
        // "unexpected status 401" sends people looking at the network.
        401 => Result<byte[], SyncError>.Err(new SyncError("the server refused the token — check the sync token in Settings")),
        _ => Result<byte[], SyncError>.Err(new SyncError(
            string.Format(CultureInfo.InvariantCulture, "the server answered {0}: {1}", status, Encoding.UTF8.GetString(body).Trim()))),
    };

    /// <summary>
    /// What the server says a push did. The same four numbers as
    /// <see cref="Applied"/>, read separately because the core owns what a
    /// merge means and this is a wire shape. It also carries a cursor the
    /// client does not use.
    /// </summary>
    private sealed record PushReport
    {
        [JsonPropertyName("written")]
        public int Written { get; init; }

        [JsonPropertyName("removed")]
        public int Removed { get; init; }

        [JsonPropertyName("kept")]
        public int Kept { get; init; }

        [JsonPropertyName("unreadable")]
        public int Unreadable { get; init; }

        [JsonPropertyName("cursor")]
        public long Cursor { get; init; }
    }

    /// <summary>The answer to <c>/wait</c>.</summary>
    private sealed record Waited
    {
        [JsonPropertyName("changed")]
        public bool Changed { get; init; }
    }
}
