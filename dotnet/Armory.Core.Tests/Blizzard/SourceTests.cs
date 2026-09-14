using System.Text;
using Armory.Blizzard;
using Xunit;

namespace Armory.Tests.Blizzard;

/// <summary>Ported from <c>core/src/source/mod.rs</c>, <c>source/blizzard/mod.rs</c> and <c>oauth.rs</c>.</summary>
public sealed class SourceTests
{
    private static byte[] B(string text) => Encoding.UTF8.GetBytes(text);

    [Fact(DisplayName = "empty_and_unchanged_are_answers_and_stale_is_a_gap")]
    public void Empty_and_unchanged_are_answers_and_stale_is_a_gap()
    {
        Assert.Null(new Outcome<byte>.Empty().Gap());
        Assert.Null(new Outcome<byte>.Unchanged().Gap());
        Assert.Null(new Outcome<byte>.Found(1).Gap());
        Assert.Contains("timed out", new Outcome<byte>.Unusable(new Reason.Timeout()).Gap()!.ToString(), StringComparison.Ordinal);
        Assert.Contains("check failed", new Outcome<byte>.Stale(new Reason.Malformed("no criteria")).Gap()!.ToString(), StringComparison.Ordinal);
    }

    [Fact(DisplayName = "sharing_disabled_reads_as_a_setting_not_a_status_code")]
    public void Sharing_disabled_reads_as_a_setting_not_a_status_code()
    {
        var text = new Reason.SharingDisabled().ToString();
        Assert.Contains("privacy", text, StringComparison.Ordinal);
        Assert.DoesNotContain("403", text, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "a_non_json_body_is_stale_rather_than_empty")]
    public void A_non_json_body_is_stale_rather_than_empty()
    {
        var parsed = Outcomes.ParseJson<byte>(SourceId.BlizzardProfile, B("<html>nope</html>"));
        Assert.False(parsed.IsOk);
        Assert.IsType<Outcome<byte>.Stale>(parsed.Error);
    }

    [Fact(DisplayName = "a_collection_of_nothing_is_empty")]
    public void A_collection_of_nothing_is_empty()
    {
        Assert.IsType<Outcome<List<byte>>.Empty>(Outcomes.OfCollection(new List<byte>()));
        Assert.Equal([1], Assert.IsType<Outcome<List<byte>>.Found>(Outcomes.OfCollection(new List<byte> { 1 })).Value);
    }

    [Fact(DisplayName = "a_request_keys_its_cache_entry_on_the_url_not_the_token")]
    public void A_request_keys_its_cache_entry_on_the_url_not_the_token()
    {
        var a = Request.Get(SourceId.BlizzardProfile, "https://example/x").Bearer("one");
        var b = Request.Get(SourceId.BlizzardProfile, "https://example/x").Bearer("two");
        Assert.Equal(a.CacheKey, b.CacheKey);
    }

    [Fact(DisplayName = "a_post_carries_a_form_body_and_says_so")]
    public void A_post_carries_a_form_body_and_says_so()
    {
        var request = Request.Post(SourceId.BattleNetOAuth, "https://example/token", "a=b");
        Assert.Equal(Method.Post, request.Method);
        Assert.Equal("a=b", request.Body);
        Assert.Contains(request.Headers, header => header.Name == "Content-Type" && header.Value.Contains("form-urlencoded", StringComparison.Ordinal));
    }

    [Fact(DisplayName = "a_url_always_carries_its_namespace_and_locale")]
    public void A_url_always_carries_its_namespace_and_locale()
    {
        var built = Api.Url(Region.Us, Namespace.Profile, "/profile/user/wow");
        Assert.StartsWith("https://us.api.blizzard.com/profile/user/wow?", built, StringComparison.Ordinal);
        Assert.Contains("namespace=profile-us", built, StringComparison.Ordinal);
        Assert.Contains("locale=en_US", built, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "namespaces_are_unversioned")]
    public void Namespaces_are_unversioned()
    {
        Assert.Equal("static-eu", Namespace.Static.Qualified(Region.Eu));
        Assert.Equal("dynamic-us", Namespace.Dynamic.Qualified(Region.Us));
    }

    [Fact(DisplayName = "realm_names_slug_the_way_the_api_spells_them")]
    public void Realm_names_slug_the_way_the_api_spells_them()
    {
        Assert.Equal("emerald-dream", Slug.RealmSlug("Emerald Dream"));
        Assert.Equal("mannoroth", Slug.RealmSlug("Mannoroth"));
        Assert.Equal("zuljin", Slug.RealmSlug("Zul'jin"));
        Assert.Equal("aerie-peak", Slug.RealmSlug("Aerie Peak"));
    }

    [Fact(DisplayName = "parameters_are_encoded")]
    public void Parameters_are_encoded()
    {
        var built = Api.Url(Region.Us, Namespace.Static, "/data/wow/search/item", ("name.en_US", "Reins of the Onyxian Drake"));
        Assert.Contains("name.en_US=Reins%20of%20the%20Onyxian%20Drake", built, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "a_region_round_trips_through_its_code")]
    public void A_region_round_trips_through_its_code()
    {
        foreach (var region in RegionExtensions.All)
        {
            Assert.Equal(region, RegionExtensions.FromCode(region.Code()));
        }
        Assert.Equal(Region.Us, RegionExtensions.FromCode("US"));
        Assert.Null(RegionExtensions.FromCode("cn"));
    }

    private static ClientCredentials Client() => new() { Id = "abc123", Secret = "shhh" };

    [Fact(DisplayName = "the_redirect_is_loopback_on_a_fixed_port")]
    public void The_redirect_is_loopback_on_a_fixed_port()
    {
        var uri = OAuth.RedirectUri;
        Assert.StartsWith("http://127.0.0.1:", uri, StringComparison.Ordinal);
        Assert.EndsWith("/callback", uri, StringComparison.Ordinal);
        Assert.Equal(21451, OAuth.RedirectPort);
    }

    [Fact(DisplayName = "the_authorize_url_carries_the_encoded_redirect_and_the_state")]
    public void The_authorize_url_carries_the_encoded_redirect_and_the_state()
    {
        var url = OAuth.AuthorizeUrl(Client(), "nonce-42");
        Assert.StartsWith("https://oauth.battle.net/authorize", url, StringComparison.Ordinal);
        Assert.Contains("client_id=abc123", url, StringComparison.Ordinal);
        Assert.Contains("scope=wow.profile", url, StringComparison.Ordinal);
        Assert.Contains("state=nonce-42", url, StringComparison.Ordinal);
        Assert.Contains("response_type=code", url, StringComparison.Ordinal);
        Assert.Contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A", url, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "the_secret_never_reaches_the_url")]
    public void The_secret_never_reaches_the_url()
    {
        Assert.DoesNotContain("shhh", OAuth.AuthorizeUrl(Client(), "nonce"), StringComparison.Ordinal);
        var request = OAuth.Exchange(Client(), "the-code");
        Assert.DoesNotContain("shhh", request.Url, StringComparison.Ordinal);
        Assert.DoesNotContain("shhh", request.Body ?? "", StringComparison.Ordinal);
        Assert.Contains(request.Headers, header => header.Name == "Authorization" && header.Value.StartsWith("Basic ", StringComparison.Ordinal));
    }

    [Fact(DisplayName = "basic_auth_encodes_the_pair_the_way_the_endpoint_expects")]
    public void Basic_auth_encodes_the_pair_the_way_the_endpoint_expects()
    {
        Assert.Equal("Basic YWJjMTIzOnNoaGg=", Client().Basic());
    }

    [Fact(DisplayName = "a_token_response_is_read_whole")]
    public void A_token_response_is_read_whole()
    {
        var token = Assert.IsType<Outcome<Token>.Found>(OAuth.ParseToken(B("""{"access_token":"tok","token_type":"bearer","expires_in":86399}"""))).Value;
        Assert.Equal("tok", token.Access);
        Assert.Equal(86399, token.ExpiresIn);
        Assert.Null(token.Refresh);
    }

    [Fact(DisplayName = "a_refresh_token_is_kept_when_offered")]
    public void A_refresh_token_is_kept_when_offered()
    {
        var token = Assert.IsType<Outcome<Token>.Found>(OAuth.ParseToken(B("""{"access_token":"tok","expires_in":100,"refresh_token":"r"}"""))).Value;
        Assert.Equal("r", token.Refresh);
    }

    [Fact(DisplayName = "a_missing_lifetime_expires_soon_rather_than_never")]
    public void A_missing_lifetime_expires_soon_rather_than_never()
    {
        Assert.Equal(3600, Assert.IsType<Outcome<Token>.Found>(OAuth.ParseToken(B("""{"access_token":"tok"}"""))).Value.ExpiresIn);
    }

    [Fact(DisplayName = "a_bad_secret_reads_as_a_sign_in_problem_not_a_broken_parser")]
    public void A_bad_secret_reads_as_a_sign_in_problem_not_a_broken_parser()
    {
        var outcome = Assert.IsType<Outcome<Token>.Unusable>(OAuth.ParseToken(B("""{"error":"invalid_client","error_description":"bad secret"}""")));
        Assert.Equal("bad secret", Assert.IsType<Reason.Unauthorised>(outcome.Why).What);
    }

    [Fact(DisplayName = "a_body_that_is_not_json_is_stale")]
    public void A_body_that_is_not_json_is_stale()
    {
        Assert.IsType<Outcome<Token>.Stale>(OAuth.ParseToken(B("<html>maintenance</html>")));
    }

    [Fact(DisplayName = "the_callback_yields_the_code_and_the_state")]
    public void The_callback_yields_the_code_and_the_state()
    {
        var (code, state) = OAuth.ParseCallback("?code=abc&state=nonce-42").Value;
        Assert.Equal("abc", code);
        Assert.Equal("nonce-42", state);
    }

    [Fact(DisplayName = "a_refusal_at_the_sign_in_page_is_a_decision_not_a_fault")]
    public void A_refusal_at_the_sign_in_page_is_a_decision_not_a_fault()
    {
        var error = OAuth.ParseCallback("?error=access_denied&error_description=the%20user%20said%20no").Error;
        Assert.IsType<Reason.Unauthorised>(error);
        Assert.Contains("the user said no", error.ToString(), StringComparison.Ordinal);
    }

    [Fact(DisplayName = "a_redirect_with_no_code_is_malformed")]
    public void A_redirect_with_no_code_is_malformed()
    {
        Assert.IsType<Reason.Malformed>(OAuth.ParseCallback("?state=nonce").Error);
    }
}
