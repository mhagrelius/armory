using System.Net.Http.Headers;
using System.Reflection;
using System.Text;
using Armory.Blizzard;
using Armory.Chronicle;

namespace Armory.Client.Blizzard;

/// <summary>A body, and the stamp that makes the next request for it conditional.</summary>
public sealed record Response(byte[] Body, string? LastModified);

/// <summary>
/// The only class in the client that performs a Blizzard request.
/// </summary>
/// <remarks>
/// <para>Everything under <c>Armory.Blizzard</c> builds a <see cref="Request"/>
/// and parses a body; this drives them. Keeping that boundary in one place is
/// what lets the whole source layer be tested from recorded fixtures with no
/// network.</para>
/// <para>Rate limiting is one token bucket for the whole application, because
/// Blizzard's limits are per client id rather than per endpoint or per IP:
/// 100 requests a second and 36,000 an hour, all sharing one budget. A
/// request that arrives too early is scheduled rather than dropped.</para>
/// <para>It is never thrown from: a source refusing is an expected outcome of
/// a sync, not an error in it, so every failure comes back as an
/// <see cref="Outcome{T}"/>.</para>
/// </remarks>
public sealed class Http : IDisposable
{
    /// <summary>Blizzard's per-second ceiling. The hourly one is two orders of magnitude away from anything a person clicking around will reach.</summary>
    public const int MaxPerSecond = 100;

    /// <summary>How long to wait on a request. Sized for an API that answers in milliseconds.</summary>
    public static readonly TimeSpan DefaultTimeout = TimeSpan.FromSeconds(20);

    /// <summary>Kept a little under the ceiling. A 429 costs a retry and a round trip; spacing requests costs microseconds.</summary>
    public static readonly TimeSpan MinInterval = TimeSpan.FromMilliseconds(1000 / MaxPerSecond + 2);

    /// <summary>How long to stand down after Blizzard says to slow down.</summary>
    public static readonly TimeSpan Backoff = TimeSpan.FromSeconds(5);

    private static readonly TimeSpan Window = TimeSpan.FromSeconds(1);

    private readonly HttpClient client;
    private readonly TimeProvider clock;
    private readonly object gate = new();
    private readonly Queue<DateTimeOffset> recent = new();
    private DateTimeOffset nextAllowed;

    /// <summary>
    /// A client with the ordinary timeout. <paramref name="handler"/> is for a
    /// test that answers in-process, and <paramref name="clock"/> for one that
    /// drives the gate; the application passes neither.
    /// </summary>
    public Http(HttpMessageHandler? handler = null, TimeProvider? clock = null)
        : this(DefaultTimeout, handler, clock)
    {
    }

    /// <summary>
    /// A client that will wait longer than an API call has any business
    /// taking: writing four hundred words takes a language model tens of
    /// seconds. Its own client rather than a per-request setting because it
    /// also wants its own rate gate, and one journal entry should not spend a
    /// sync's budget.
    /// </summary>
    public Http(TimeSpan timeout, HttpMessageHandler? handler = null, TimeProvider? clock = null)
    {
        this.clock = clock ?? TimeProvider.System;
        client = handler is null ? new HttpClient() : new HttpClient(handler, disposeHandler: true);
        client.Timeout = timeout;
        var version = Assembly.GetExecutingAssembly().GetName().Version?.ToString(3) ?? "0";
        client.DefaultRequestHeaders.UserAgent.Add(new ProductInfoHeaderValue("Armory", version));
        nextAllowed = this.clock.GetUtcNow();
    }

    public void Dispose() => client.Dispose();

    /// <summary>
    /// Perform a request. Completes exactly once with the response or with
    /// the reason there is not one, after whatever wait the gate requires.
    /// </summary>
    public async Task<Outcome<Response>> Fetch(Request request, CancellationToken cancellation = default)
    {
        var wait = Reserve();
        if (wait > TimeSpan.Zero)
        {
            await Task.Delay(wait, clock, cancellation).ConfigureAwait(false);
        }
        return await Send(request, cancellation).ConfigureAwait(false);
    }

    /// <summary>Claim the next slot, and say how long until it opens.</summary>
    internal TimeSpan Reserve()
    {
        lock (gate)
        {
            var now = clock.GetUtcNow();
            // Drop anything that left the one-second window.
            while (recent.TryPeek(out var oldest) && now - oldest >= Window)
            {
                recent.Dequeue();
            }

            var at = nextAllowed > now ? nextAllowed : now;
            if (recent.Count >= MaxPerSecond && recent.TryPeek(out var first))
            {
                var opens = first + Window;
                at = opens > at ? opens : at;
            }

            recent.Enqueue(at);
            nextAllowed = at + MinInterval;
            return at > now ? at - now : TimeSpan.Zero;
        }
    }

    /// <summary>Push the next slot out after being told to slow down.</summary>
    internal void Penalise()
    {
        lock (gate)
        {
            nextAllowed = clock.GetUtcNow() + Backoff;
        }
    }

    private async Task<Outcome<Response>> Send(Request request, CancellationToken cancellation)
    {
        if (!Uri.TryCreate(request.Url, UriKind.Absolute, out var url))
        {
            return new Outcome<Response>.Unusable(new Reason.Network($"{request.Url} is not a URL"));
        }

        using var message = new HttpRequestMessage(request.Method == Method.Post ? HttpMethod.Post : HttpMethod.Get, url);
        string? contentType = null;
        foreach (var (name, value) in request.Headers)
        {
            if (name.Equals("content-type", StringComparison.OrdinalIgnoreCase))
            {
                contentType = value;
            }
            else
            {
                message.Headers.TryAddWithoutValidation(name, value);
            }
        }
        if (request.Method == Method.Post && request.Body is { } body)
        {
            // Taken from the request rather than assumed. The token endpoint
            // posts a form and the journal posts JSON, and a JSON body
            // announced as form-encoded is rejected outright.
            message.Content = new StringContent(body, Encoding.UTF8, contentType ?? "application/x-www-form-urlencoded");
        }

        int status;
        byte[] bytes;
        string? lastModified;
        try
        {
            using var reply = await client.SendAsync(message, HttpCompletionOption.ResponseContentRead, cancellation).ConfigureAwait(false);
            status = (int)reply.StatusCode;
            lastModified = reply.Content.Headers.TryGetValues("Last-Modified", out var stamps) ? stamps.FirstOrDefault() : null;
            bytes = await reply.Content.ReadAsByteArrayAsync(cancellation).ConfigureAwait(false);
        }
        catch (OperationCanceledException) when (!cancellation.IsCancellationRequested)
        {
            // The client's own timeout surfaces as a cancellation. A timed-out
            // sync and an unreachable one read differently to a person.
            return new Outcome<Response>.Unusable(new Reason.Timeout());
        }
        catch (OperationCanceledException)
        {
            return new Outcome<Response>.Unusable(new Reason.Network("cancelled"));
        }
        catch (HttpRequestException error)
        {
            return new Outcome<Response>.Unusable(new Reason.Network(error.Message));
        }

        var outcome = Read(request.Source, status, bytes, lastModified);
        if (status is 429 or 503)
        {
            Penalise();
        }
        return outcome;
    }

    /// <summary>
    /// What a status and a body mean.
    /// </summary>
    /// <remarks>
    /// Everything here about 401, 403 and 404 is true of Blizzard and false
    /// of anywhere else. A 403 is a privacy checkbox <i>there</i>; the journal
    /// server answers one for an ordinary permission problem, and reporting
    /// that as "your Battle.net privacy settings" would be a sentence about
    /// the wrong account entirely. So the one non-Blizzard source reads its
    /// own statuses, and reads the body while it is at it: those are the
    /// errors a person can act on, and the service says which in words.
    /// </remarks>
    public static Outcome<Response> Read(SourceId source, int status, byte[] body, string? lastModified)
    {
        // The whole point of the conditional request: our copy is still good,
        // and this cost one round trip and no bytes.
        if (status == 304)
        {
            return new Outcome<Response>.Unchanged();
        }
        if (status is 429 or 503)
        {
            return new Outcome<Response>.Unusable(new Reason.RateLimited());
        }
        var succeeded = status is >= 200 and < 300;
        if (!source.IsBlizzard() && !succeeded)
        {
            var detail = Journal.ParseError(body) ?? $"HTTP {status}";
            return status == 401
                ? new Outcome<Response>.Unusable(new Reason.Unauthorised(detail))
                : new Outcome<Response>.Unusable(new Reason.Declined(detail));
        }
        return status switch
        {
            401 => new Outcome<Response>.Unusable(new Reason.Unauthorised("the sign-in has expired")),
            // Blizzard answers 403 when the account has third-party data
            // sharing switched off. That is a checkbox on their privacy
            // settings, not a fault here.
            403 => new Outcome<Response>.Unusable(new Reason.SharingDisabled()),
            // A character who has never logged in since the API started
            // tracking them 404s. That is an answer about the character.
            404 => new Outcome<Response>.Empty(),
            _ when !succeeded => new Outcome<Response>.Unusable(new Reason.Http(status)),
            _ when body.Length == 0 => new Outcome<Response>.Empty(),
            _ => new Outcome<Response>.Found(new Response(body, lastModified)),
        };
    }
}
