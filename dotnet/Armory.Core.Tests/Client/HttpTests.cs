using System.Net;
using System.Text;
using Armory.Blizzard;
using Armory.Client.Blizzard;
using Armory.Tests.Store;
using Xunit;

namespace Armory.Tests.Client;

/// <summary>The Blizzard client from <c>src/ui/http.rs</c>, which had no tests of its own: the status readings, the gate, and what goes on the wire.</summary>
public sealed class HttpTests
{
    private static readonly byte[] Nothing = [];

    private static Request Profile() => Request.Get(SourceId.BlizzardProfile, "https://us.api.blizzard.com/profile/user/wow");

    [Fact(DisplayName = "a 304 is unchanged, which is neither an answer nor a failure")]
    public void A_304_is_unchanged()
    {
        Assert.IsType<Outcome<Response>.Unchanged>(Http.Read(SourceId.BlizzardProfile, 304, Nothing, null));
    }

    [Fact(DisplayName = "blizzard's 401, 403 and 404 are read the way blizzard means them")]
    public void Blizzards_statuses_are_read_the_way_blizzard_means_them()
    {
        var expired = Assert.IsType<Outcome<Response>.Unusable>(Http.Read(SourceId.BlizzardProfile, 401, Nothing, null));
        Assert.IsType<Reason.Unauthorised>(expired.Why);
        var sharing = Assert.IsType<Outcome<Response>.Unusable>(Http.Read(SourceId.BlizzardProfile, 403, Nothing, null));
        Assert.IsType<Reason.SharingDisabled>(sharing.Why);
        Assert.IsType<Outcome<Response>.Empty>(Http.Read(SourceId.BlizzardProfile, 404, Nothing, null));
        var other = Assert.IsType<Outcome<Response>.Unusable>(Http.Read(SourceId.BlizzardProfile, 500, Nothing, null));
        Assert.Equal(500, Assert.IsType<Reason.Http>(other.Why).Code);
    }

    [Fact(DisplayName = "the journal reads its own statuses and its own error body")]
    public void The_journal_reads_its_own_statuses_and_its_own_error_body()
    {
        var body = Encoding.UTF8.GetBytes("""{"error":{"message":"invalid x-api-key","type":"authentication_error"}}""");
        var key = Assert.IsType<Outcome<Response>.Unusable>(Http.Read(SourceId.Journal, 401, body, null));
        Assert.Equal("invalid x-api-key", Assert.IsType<Reason.Unauthorised>(key.Why).What);
        var forbidden = Assert.IsType<Outcome<Response>.Unusable>(Http.Read(SourceId.Journal, 403, Nothing, null));
        Assert.Equal("HTTP 403", Assert.IsType<Reason.Declined>(forbidden.Why).What);
        Assert.IsType<Outcome<Response>.Found>(Http.Read(SourceId.Journal, 200, "{}"u8.ToArray(), null));
    }

    [Fact(DisplayName = "a 429 or a 503 is rate limited before anything else is read into it")]
    public void A_429_or_a_503_is_rate_limited()
    {
        Assert.IsType<Reason.RateLimited>(Assert.IsType<Outcome<Response>.Unusable>(Http.Read(SourceId.BlizzardGameData, 429, Nothing, null)).Why);
        Assert.IsType<Reason.RateLimited>(Assert.IsType<Outcome<Response>.Unusable>(Http.Read(SourceId.Journal, 503, Nothing, null)).Why);
    }

    [Fact(DisplayName = "an empty body is empty and a full one carries its last-modified stamp")]
    public void An_empty_body_is_empty_and_a_full_one_carries_its_stamp()
    {
        Assert.IsType<Outcome<Response>.Empty>(Http.Read(SourceId.BlizzardProfile, 200, Nothing, null));
        var found = Assert.IsType<Outcome<Response>.Found>(Http.Read(SourceId.BlizzardProfile, 200, "{}"u8.ToArray(), "Tue, 01 Sep 2026 10:00:00 GMT"));
        Assert.Equal("Tue, 01 Sep 2026 10:00:00 GMT", found.Value.LastModified);
    }

    [Fact(DisplayName = "the gate spaces neighbours and caps the second")]
    public void The_gate_spaces_neighbours_and_caps_the_second()
    {
        var clock = new FixedClock();
        using var http = new Http(new Answering(_ => new HttpResponseMessage(HttpStatusCode.OK)), clock);

        Assert.Equal(TimeSpan.Zero, http.Reserve());
        Assert.Equal(Http.MinInterval, http.Reserve());
        Assert.Equal(Http.MinInterval * 2, http.Reserve());

        // A hundred slots claimed inside one second: the next one waits for the
        // oldest to leave the window, not merely for the interval.
        for (var i = 3; i < Http.MaxPerSecond; i++)
        {
            http.Reserve();
        }
        Assert.True(http.Reserve() >= TimeSpan.FromSeconds(1));
    }

    [Fact(DisplayName = "being told to slow down pushes the next slot out")]
    public void Being_told_to_slow_down_pushes_the_next_slot_out()
    {
        var clock = new FixedClock();
        using var http = new Http(new Answering(_ => new HttpResponseMessage(HttpStatusCode.OK)), clock);
        http.Penalise();
        Assert.Equal(Http.Backoff, http.Reserve());
    }

    [Fact(DisplayName = "a request goes out with its headers, its body and the content type it named")]
    public async Task A_request_goes_out_with_its_headers_and_body()
    {
        var server = new Answering(_ => new HttpResponseMessage(HttpStatusCode.OK)
        {
            Content = new ByteArrayContent("""{"access_token":"x"}"""u8.ToArray()),
        });
        using var http = new Http(server);

        var post = Request.Post(SourceId.Journal, "http://127.0.0.1:8080/v1/chat/completions", """{"model":"x"}""")
            .Header("Content-Type", "application/json")
            .Bearer("token");
        var outcome = await http.Fetch(post, TestContext.Current.CancellationToken);

        var found = Assert.IsType<Outcome<Response>.Found>(outcome);
        Assert.Equal("""{"access_token":"x"}""", Encoding.UTF8.GetString(found.Value.Body));
        var seen = Assert.Single(server.Requests);
        Assert.Equal(HttpMethod.Post, seen.Method);
        Assert.Equal("application/json", seen.ContentType);
        Assert.Equal("""{"model":"x"}""", seen.Body);
        Assert.Equal("Bearer token", seen.Headers["Authorization"]);
        Assert.StartsWith("Armory/", seen.Headers["User-Agent"], StringComparison.Ordinal);
    }

    [Fact(DisplayName = "a form post is announced as a form unless the request says otherwise")]
    public async Task A_form_post_is_announced_as_a_form()
    {
        var server = new Answering(_ => new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent("{}") });
        using var http = new Http(server);
        await http.Fetch(Request.Post(SourceId.BattleNetOAuth, "https://oauth.battle.net/token", "grant_type=client_credentials"), TestContext.Current.CancellationToken);
        Assert.Equal("application/x-www-form-urlencoded", Assert.Single(server.Requests).ContentType);
    }

    [Fact(DisplayName = "a refusal on the wire is read like a refusal in the reader, and a bad url never leaves")]
    public async Task A_refusal_on_the_wire_is_read_like_a_refusal_in_the_reader()
    {
        var server = new Answering(_ => new HttpResponseMessage(HttpStatusCode.Forbidden));
        using var http = new Http(server);
        var refused = Assert.IsType<Outcome<Response>.Unusable>(await http.Fetch(Profile(), TestContext.Current.CancellationToken));
        Assert.IsType<Reason.SharingDisabled>(refused.Why);

        var broken = Assert.IsType<Outcome<Response>.Unusable>(await http.Fetch(Request.Get(SourceId.BlizzardProfile, "not a url"), TestContext.Current.CancellationToken));
        Assert.IsType<Reason.Network>(broken.Why);
        Assert.Single(server.Requests);
    }

    [Fact(DisplayName = "an unreachable host is a network reason and not an exception")]
    public async Task An_unreachable_host_is_a_network_reason()
    {
        var server = new Answering(_ => throw new HttpRequestException("No such host is known."));
        using var http = new Http(server);
        var down = Assert.IsType<Outcome<Response>.Unusable>(await http.Fetch(Profile(), TestContext.Current.CancellationToken));
        Assert.Equal("No such host is known.", Assert.IsType<Reason.Network>(down.Why).What);
    }

    internal sealed record Seen(HttpMethod Method, string Url, Dictionary<string, string> Headers, string? ContentType, string? Body);

    /// <summary>A server answered in-process, remembering what it was asked.</summary>
    internal sealed class Answering : HttpMessageHandler
    {
        private readonly Func<Seen, HttpResponseMessage> answer;

        public Answering(Func<Seen, HttpResponseMessage> answer)
        {
            this.answer = answer;
        }

        public List<Seen> Requests { get; } = [];

        protected override HttpResponseMessage Send(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            var headers = request.Headers.ToDictionary(h => h.Key, h => string.Join(",", h.Value), StringComparer.Ordinal);
            var seen = new Seen(
                request.Method,
                request.RequestUri!.ToString(),
                headers,
                request.Content?.Headers.ContentType?.MediaType,
                request.Content?.ReadAsStringAsync(cancellationToken).GetAwaiter().GetResult());
            Requests.Add(seen);
            return answer(seen);
        }

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken) =>
            Task.FromResult(Send(request, cancellationToken));
    }
}
