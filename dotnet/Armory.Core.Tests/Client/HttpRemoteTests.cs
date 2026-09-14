using System.Net;
using System.Text;
using Armory.Client.Sharing;
using Armory.Sharing;
using Xunit;

namespace Armory.Tests.Client;

/// <summary>Ported from <c>src/ui/sync.rs</c>, plus the calls a fake server can answer in-process.</summary>
public sealed class HttpRemoteTests
{
    [Fact(DisplayName = "a_bare_host_gets_the_servers_port")]
    public void A_bare_host_gets_the_servers_port()
    {
        Assert.Equal("nas.example:8084", HttpRemote.ParseAddress("http://nas.example:8084").Value);
        Assert.Equal("nas.example:8084", HttpRemote.ParseAddress("http://nas.example").Value);
    }

    [Fact(DisplayName = "a_trailing_slash_is_not_part_of_the_address")]
    public void A_trailing_slash_is_not_part_of_the_address()
    {
        Assert.Equal("nas:8084", HttpRemote.ParseAddress("http://nas:8084/").Value);
    }

    [Fact(DisplayName = "https_is_refused_rather_than_quietly_downgraded")]
    public void Https_is_refused_rather_than_quietly_downgraded()
    {
        var refused = HttpRemote.ParseAddress("https://nas:8084");
        Assert.False(refused.IsOk);
        Assert.Contains("plain HTTP", refused.Error.Message, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "an_empty_address_is_refused")]
    public void An_empty_address_is_refused()
    {
        Assert.False(HttpRemote.ParseAddress("").IsOk);
        Assert.False(HttpRemote.ParseAddress("http://").IsOk);
    }

    [Fact(DisplayName = "a_refusal_names_the_thing_somebody_can_change")]
    public void A_refusal_names_the_thing_somebody_can_change()
    {
        var refused = HttpRemote.ReadAnswer(401, "unauthorized"u8.ToArray());
        Assert.False(refused.IsOk);
        Assert.Contains("token", refused.Error.Message, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "a_body_is_taken_from_after_the_blank_line")]
    public void A_body_is_taken_from_after_the_blank_line()
    {
        Assert.Equal("""{"ok":true}"""u8.ToArray(), HttpRemote.ReadAnswer(200, """{"ok":true}"""u8.ToArray()).Value);
    }

    [Fact(DisplayName = "another_status_carries_what_the_server_said")]
    public void Another_status_carries_what_the_server_said()
    {
        var refused = HttpRemote.ReadAnswer(400, "no X-Armory-Machine header"u8.ToArray());
        Assert.Contains("400", refused.Error.Message, StringComparison.Ordinal);
        Assert.Contains("X-Armory-Machine", refused.Error.Message, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "every call carries the token, the machine and the account")]
    public void Every_call_carries_the_token_the_machine_and_the_account()
    {
        var server = new FakeServer(_ => Json("""{"written":1,"removed":0,"kept":0,"unreadable":0,"cursor":7}"""));
        using var remote = HttpRemote.Create("http://nas:8084", " secret ", "machine-a", "matthew", server).Value;

        var applied = remote.Push(new Parcel()).Value;

        Assert.Equal(1, applied.Written);
        var request = Assert.Single(server.Requests);
        Assert.Equal(HttpMethod.Post, request.Method);
        Assert.Equal("http://nas:8084/push", request.Uri);
        Assert.Equal("Bearer secret", request.Headers["Authorization"]);
        Assert.Equal("machine-a", request.Headers["X-Armory-Machine"]);
        Assert.Equal("matthew", request.Headers["X-Armory-Account"]);
        Assert.Equal("application/json", request.ContentType);
        Assert.Equal("""{"rows":[]}""", request.Body);
    }

    [Fact(DisplayName = "a pull asks from the cursor and reads the server's shape")]
    public void A_pull_asks_from_the_cursor_and_reads_the_servers_shape()
    {
        var server = new FakeServer(_ => Json("""{"parcel":{"rows":[{"scope":"watched","key":[4306],"fields":["Silk Cloth"]}]},"cursor":42,"more":false}"""));
        using var remote = HttpRemote.Create("http://nas", "t", "m", "a", server).Value;

        var pulled = remote.Pull(41, 500).Value;

        Assert.Equal("http://nas:8084/pull?since=41&limit=500", Assert.Single(server.Requests).Uri);
        Assert.Equal(42, pulled.Cursor);
        Assert.False(pulled.More);
        Assert.Equal("watched", Assert.Single(pulled.Parcel.Rows).Scope);
    }

    [Fact(DisplayName = "a wait answers whether anything changed")]
    public void A_wait_answers_whether_anything_changed()
    {
        var server = new FakeServer(_ => Json("""{"changed":true}"""));
        using var remote = HttpRemote.Create("http://nas", "t", "m", "a", server).Value;
        Assert.True(remote.Wait(3).Value);
        Assert.Equal("http://nas:8084/wait?since=3", Assert.Single(server.Requests).Uri);
    }

    [Fact(DisplayName = "the accounts a server holds come back with their sizes")]
    public void The_accounts_a_server_holds_come_back_with_their_sizes()
    {
        var server = new FakeServer(_ => Json("""[{"name":"matthew","rows":51234},{"name":"sync-check","rows":9}]"""));
        using var remote = HttpRemote.Create("http://nas", "t", "m", "a", server).Value;
        var held = remote.Accounts().Value;
        Assert.Equal(2, held.Count);
        Assert.Equal("matthew", held[0].Name);
        Assert.Equal(51234, held[0].Rows);
    }

    [Fact(DisplayName = "forgetting an account confirms it by name")]
    public void Forgetting_an_account_confirms_it_by_name()
    {
        var server = new FakeServer(_ => new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent("gone") });
        using var remote = HttpRemote.Create("http://nas", "t", "m", "a", server).Value;
        Assert.True(remote.ForgetAccount("sync-check").IsOk);
        var request = Assert.Single(server.Requests);
        Assert.Equal(HttpMethod.Delete, request.Method);
        Assert.Equal("http://nas:8084/accounts/sync-check?confirm=sync-check", request.Uri);
    }

    [Fact(DisplayName = "a server that cannot be reached is an error rather than an exception")]
    public void A_server_that_cannot_be_reached_is_an_error_rather_than_an_exception()
    {
        var server = new FakeServer(_ => throw new HttpRequestException("connection refused"));
        using var remote = HttpRemote.Create("http://nas", "t", "m", "a", server).Value;
        var answer = remote.Reachable();
        Assert.False(answer.IsOk);
        Assert.Contains("could not reach nas:8084", answer.Error.Message, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "an answer that is not the shape asked for is an error rather than a panic")]
    public void An_answer_that_is_not_the_shape_asked_for_is_an_error_rather_than_a_panic()
    {
        var server = new FakeServer(_ => new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent("garbage") });
        using var remote = HttpRemote.Create("http://nas", "t", "m", "a", server).Value;
        var answer = remote.Pull(0, 10);
        Assert.False(answer.IsOk);
        Assert.Contains("could not read the answer", answer.Error.Message, StringComparison.Ordinal);
    }

    private static HttpResponseMessage Json(string body) =>
        new(HttpStatusCode.OK) { Content = new StringContent(body, Encoding.UTF8, "application/json") };

    /// <summary>What one request looked like on the wire, as far as a handler can see.</summary>
    internal sealed record Seen(HttpMethod Method, string Uri, Dictionary<string, string> Headers, string? ContentType, string? Body);

    /// <summary>The external seam, answered in-process. The one place a test double is the right tool.</summary>
    internal sealed class FakeServer : HttpMessageHandler
    {
        private readonly Func<Seen, HttpResponseMessage> answer;

        public FakeServer(Func<Seen, HttpResponseMessage> answer)
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
