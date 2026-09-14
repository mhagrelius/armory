using System.Net;
using Armory.Blizzard;
using Armory.Client.Blizzard;
using Xunit;

namespace Armory.Tests.Client;

/// <summary>The loopback listener the sign-in lands on, driven with a real browser-shaped request. Loopback only; nothing leaves the machine.</summary>
public sealed class RedirectTests
{
    // Ports of their own, away from the registered one: the account tests
    // bind that one, and xunit runs classes in parallel.
    private const int Base = 21460;

    private static async Task<(HttpStatusCode Status, string Body)> Get(int port, string path)
    {
        using var client = new HttpClient();
        using var response = await client.GetAsync(new Uri($"http://127.0.0.1:{port}{path}"), TestContext.Current.CancellationToken);
        return (response.StatusCode, await response.Content.ReadAsStringAsync(TestContext.Current.CancellationToken));
    }

    [Fact(DisplayName = "a matching redirect delivers the code once and tells the browser to close the tab")]
    public async Task A_matching_redirect_delivers_the_code_once()
    {
        var delivered = new List<Result<string, Reason>>();
        var listening = Redirect.Listen("abc123", delivered.Add, Base);
        Assert.True(listening.IsOk, listening.IsOk ? "" : listening.Error);
        using var redirect = listening.Value;

        var (status, body) = await Get(Base, "/callback?code=the-code&state=abc123");
        Assert.Equal(HttpStatusCode.OK, status);
        Assert.Contains("Signed in", body, StringComparison.Ordinal);
        Assert.Contains("close this tab", body, StringComparison.Ordinal);
        var one = Assert.Single(delivered);
        Assert.True(one.IsOk);
        Assert.Equal("the-code", one.Value);

        // A retried or refreshed redirect delivers the same code twice, and
        // answering only once keeps the flow from being driven backwards.
        var (again, page) = await Get(Base, "/callback?code=the-code&state=abc123");
        Assert.Equal(HttpStatusCode.OK, again);
        Assert.Contains("Already done", page, StringComparison.Ordinal);
        Assert.Single(delivered);
    }

    [Fact(DisplayName = "a redirect with the wrong state is refused, because it did not come from the flow we started")]
    public async Task A_redirect_with_the_wrong_state_is_refused()
    {
        var delivered = new List<Result<string, Reason>>();
        using var redirect = Redirect.Listen("abc123", delivered.Add, Base + 1).Value;

        var (_, body) = await Get(Base + 1, "/callback?code=planted&state=somebody-else");
        Assert.Contains("did not come from Armory", body, StringComparison.Ordinal);
        var one = Assert.Single(delivered);
        Assert.False(one.IsOk);
        Assert.IsType<Reason.Unauthorised>(one.Error);
    }

    [Fact(DisplayName = "a cancelled sign-in is reported as the reason Battle.net gave")]
    public async Task A_cancelled_sign_in_is_reported()
    {
        var delivered = new List<Result<string, Reason>>();
        using var redirect = Redirect.Listen("abc123", delivered.Add, Base + 2).Value;

        var (_, body) = await Get(Base + 2, "/callback?error=access_denied&state=abc123");
        Assert.Contains("Sign-in cancelled", body, StringComparison.Ordinal);
        Assert.False(Assert.Single(delivered).IsOk);
    }

    [Fact(DisplayName = "the favicon the browser asks for alongside is not a callback")]
    public async Task The_favicon_is_not_a_callback()
    {
        var delivered = new List<Result<string, Reason>>();
        using var redirect = Redirect.Listen("abc123", delivered.Add, Base + 3).Value;

        var (status, _) = await Get(Base + 3, "/favicon.ico");
        Assert.Equal(HttpStatusCode.NotFound, status);
        Assert.Empty(delivered);
        // And the real one still lands afterwards.
        await Get(Base + 3, "/callback?code=c&state=abc123");
        Assert.Single(delivered);
    }

    [Fact(DisplayName = "disposing unbinds the port, so the next sign-in can have it")]
    public void Disposing_unbinds_the_port()
    {
        var first = Redirect.Listen("s", _ => { }, Base + 4);
        Assert.True(first.IsOk);
        // Bound: a second listener on the same port is refused, with a reason a person can read.
        var second = Redirect.Listen("s", _ => { }, Base + 4);
        Assert.False(second.IsOk);
        Assert.NotEmpty(second.Error);
        first.Value.Dispose();
        var third = Redirect.Listen("s", _ => { }, Base + 4);
        Assert.True(third.IsOk);
        third.Value.Dispose();
    }

    [Fact(DisplayName = "the landing page fetches nothing")]
    public void The_landing_page_fetches_nothing()
    {
        foreach (var landing in Enum.GetValues<Redirect.Landing>())
        {
            var page = Redirect.Page(landing);
            Assert.DoesNotContain("<script", page, StringComparison.OrdinalIgnoreCase);
            Assert.DoesNotContain("<link", page, StringComparison.OrdinalIgnoreCase);
            Assert.DoesNotContain("<img", page, StringComparison.OrdinalIgnoreCase);
            Assert.DoesNotContain("http", page, StringComparison.OrdinalIgnoreCase);
        }
    }
}
