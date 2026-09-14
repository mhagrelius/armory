using Armory.Blizzard;
using Armory.Client.Blizzard;

namespace Armory.Client.Shell;

/// <summary>
/// The account's HTTP clients and the one way a Blizzard body is fetched.
/// Two clients, because a rate gate built for Blizzard's quota has no
/// business slowing a journal entry: the model is generating the whole
/// minute it takes, and a local server has no quota to spend.
/// </summary>
public sealed partial class Account
{
    /// <summary>How long to wait for a language model to write. Tens of seconds is ordinary.</summary>
    public static readonly TimeSpan JournalTimeout = TimeSpan.FromSeconds(180);

    private readonly HttpMessageHandler? handler;
    private Http? blizzard;
    private Http? journalHttp;
    private int outstanding;

    /// <summary>The Blizzard client, made on first use.</summary>
    public Http Blizzard => blizzard ??= new Http(handler);

    /// <summary>The journal's client, with a patience appropriate to a language model.</summary>
    public Http JournalHttp => journalHttp ??= new Http(JournalTimeout, handler);

    /// <summary>The Battle.net token for this session, or none when signed out.</summary>
    public Token? Token { get; private set; }

    /// <summary>
    /// Bumped by anything that makes an earlier sync's answers stale (a sign
    /// out, a fresh sign in, a region change). A fetch started under an older
    /// generation is dropped when it lands rather than written over newer
    /// data.
    /// </summary>
    public long Generation { get; private set; }

    /// <summary>Whether any Blizzard request is in flight. The window's busy indicator.</summary>
    public bool Busy => outstanding > 0;

    /// <summary>Raised when <see cref="Busy"/> changes.</summary>
    public event Action<bool>? BusyChanged;

    /// <summary>The client this person registered, if the vault holds its secret.</summary>
    public ClientCredentials? Credentials()
    {
        var id = Settings.ClientId;
        if (string.IsNullOrWhiteSpace(id))
        {
            return null;
        }
        var secret = secrets.Get(SecretNames.ClientSecret);
        return string.IsNullOrWhiteSpace(secret) ? null : new ClientCredentials { Id = id, Secret = secret };
    }

    /// <summary>
    /// One request, through the response cache: a conditional request when the
    /// store holds a stamp for the URL, the body stored when it comes back
    /// changed, the cached body when it comes back unchanged. Null when there
    /// is no usable body, or when the generation moved while it was in
    /// flight. The port of the GTK application's <c>fetch_bare</c>, with the
    /// continuation as a return value.
    /// </summary>
    public async Task<byte[]?> FetchBare(Token token, long generation, Request request)
    {
        var url = request.Url;
        var stamped = request.Bearer(token.Access);
        var lastModified = await Store.On(store => store.LastModified(url).Match(stamp => stamp, _ => null));
        if (lastModified is not null)
        {
            stamped = stamped.IfModifiedSince(lastModified);
        }

        if (outstanding++ == 0)
        {
            BusyChanged?.Invoke(true);
        }
        Outcome<Response> outcome;
        try
        {
            outcome = await Blizzard.Fetch(stamped);
        }
        finally
        {
            if (--outstanding == 0)
            {
                BusyChanged?.Invoke(false);
            }
        }
        if (Generation != generation)
        {
            return null;
        }

        return outcome switch
        {
            Outcome<Response>.Found found => await Store.On(store =>
            {
                store.StoreResponse(url, found.Value.Body, found.Value.LastModified);
                return found.Value.Body;
            }),
            Outcome<Response>.Unchanged => await Store.On(store =>
            {
                store.TouchResponse(url);
                return store.Response(url, TimeSpan.FromDays(Armory.Store.Store.MaxTtlDays)).Match(body => body, _ => null);
            }),
            _ => null,
        };
    }

    /// <summary>Invalidate everything in flight.</summary>
    public long NextGeneration() => ++Generation;

    /// <summary>Hold a token for this session, or drop it. Sign-in and sign-out are the callers; a test is the other.</summary>
    public void HoldToken(Token? token) => Token = token;
}
