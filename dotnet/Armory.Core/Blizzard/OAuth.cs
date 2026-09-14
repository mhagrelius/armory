using System.Globalization;
using System.Text;
using System.Text.Json.Serialization;

namespace Armory.Blizzard;

/// <summary>A registered API client: what the user creates and pastes in.</summary>
public sealed record ClientCredentials
{
    [JsonPropertyName("id")]
    public string Id { get; init; } = "";

    [JsonPropertyName("secret")]
    public string Secret { get; init; } = "";

    /// <summary>HTTP Basic, which is how the token endpoint wants the client identified, on both the exchange and the refresh.</summary>
    internal string Basic() => "Basic " + Convert.ToBase64String(Encoding.UTF8.GetBytes($"{Id}:{Secret}"));
}

/// <summary>A token, and when it stops working.</summary>
public sealed record Token
{
    [JsonPropertyName("access")]
    public string Access { get; init; } = "";

    /// <summary>Seconds from issue. Blizzard documents 24 hours and mostly honours it.</summary>
    [JsonPropertyName("expires_in")]
    public long ExpiresIn { get; init; }

    /// <summary>Kept when present and never relied upon: Battle.net's staff say these are not issued.</summary>
    [JsonPropertyName("refresh")]
    public string? Refresh { get; init; }
}

/// <summary>
/// Getting a token out of Battle.net.
/// </summary>
/// <remarks>
/// Battle.net does not support PKCE and has no public-client mode, so a
/// client secret is mandatory and Armory ships none: the user registers their
/// own client and Armory holds the credentials on their behalf. The redirect
/// is a loopback listener on a fixed port, because Blizzard matches the
/// registered string exactly and rejects custom schemes.
/// </remarks>
public static class OAuth
{
    private const string Authorize = "https://oauth.battle.net/authorize";
    private const string TokenEndpoint = "https://oauth.battle.net/token";

    /// <summary>The port the loopback listener binds, and the one the user registers.</summary>
    public const int RedirectPort = 21451;

    private const string Scope = "wow.profile";

    /// <summary>The redirect URI to register, and to send.</summary>
    public static string RedirectUri => string.Create(CultureInfo.InvariantCulture, $"http://127.0.0.1:{RedirectPort}/callback");

    /// <summary>The URL to open in a browser to begin signing in. The state is checked when the redirect lands.</summary>
    public static string AuthorizeUrl(ClientCredentials client, string state) =>
        $"{Authorize}?client_id={Api.Encode(client.Id)}&scope={Api.Encode(Scope)}&state={Api.Encode(state)}&redirect_uri={Api.Encode(RedirectUri)}&response_type=code";

    /// <summary>Exchange the code the redirect delivered for a token.</summary>
    public static Request Exchange(ClientCredentials client, string code) =>
        Request.Post(SourceId.BattleNetOAuth, TokenEndpoint, $"grant_type=authorization_code&code={Api.Encode(code)}&redirect_uri={Api.Encode(RedirectUri)}")
            .Header("Authorization", client.Basic());

    /// <summary>Trade a refresh token for a fresh access token. Its failure is not an error; it is a prompt to sign in again.</summary>
    public static Request Refresh(ClientCredentials client, string refreshToken) =>
        Request.Post(SourceId.BattleNetOAuth, TokenEndpoint, $"grant_type=refresh_token&refresh_token={Api.Encode(refreshToken)}")
            .Header("Authorization", client.Basic());

    /// <summary>Read a token out of the token endpoint's answer.</summary>
    public static Outcome<Token> ParseToken(byte[] body)
    {
        var parsed = Outcomes.ParseJson<Token>(SourceId.BattleNetOAuth, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        var value = parsed.Value;
        // The endpoint reports a bad secret as a 400 with a JSON error body
        // rather than a status the transport would catch.
        if (value.At("error").Str() is { } error)
        {
            return new Outcome<Token>.Unusable(new Reason.Unauthorised(value.At("error_description").Str() ?? error));
        }
        if (value.At("access_token").Str() is not { } access)
        {
            return new Outcome<Token>.Stale(new Reason.Malformed("the token response carried no access_token"));
        }
        return new Outcome<Token>.Found(new Token
        {
            Access = access,
            // A response with no lifetime is treated as an hour rather than
            // as forever. Being wrong in this direction costs one sign-in.
            ExpiresIn = value.At("expires_in").Int() ?? 3600,
            Refresh = value.At("refresh_token").Str(),
        });
    }

    /// <summary>
    /// Pull the code and state out of the query string the redirect carried.
    /// The user may also have refused, which is a decision, not a fault.
    /// </summary>
    public static Result<(string Code, string State), Reason> ParseCallback(string query)
    {
        string? code = null, state = null, error = null;
        foreach (var pair in query.TrimStart('?').Split('&'))
        {
            var equals = pair.IndexOf('=', StringComparison.Ordinal);
            if (equals < 0)
            {
                continue;
            }
            var name = pair[..equals];
            var value = Api.Decode(pair[(equals + 1)..]);
            switch (name)
            {
                case "code":
                    code = value;
                    break;
                case "state":
                    state = value;
                    break;
                case "error_description":
                    error = value;
                    break;
                case "error":
                    error ??= value;
                    break;
                default:
                    break;
            }
        }
        if (error is not null)
        {
            return Result<(string, string), Reason>.Err(new Reason.Unauthorised(error));
        }
        return code is not null && state is not null
            ? Result<(string, string), Reason>.Ok((code, state))
            : Result<(string, string), Reason>.Err(new Reason.Malformed("the sign-in redirect carried no code"));
    }
}
