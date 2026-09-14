using System.Net;
using System.Text;
using Armory.Blizzard;
using Armory.Client.Shell;
using Armory.Collections;
using Armory.Roster;
using Armory.Sharing;
using Armory.Tests.Store;
using Armory.Tests.Support;
using Xunit;

namespace Armory.Tests.Client;

/// <summary>
/// The Battle.net half of the orchestrator, against a fake Blizzard answered
/// in-process: sign-in, the token's life, and the sync. Nothing here touches
/// the network; the one loopback request is the sign-in redirect.
/// </summary>
public sealed class AccountBlizzardTests : IDisposable
{
    private readonly string directory = Directory.CreateTempSubdirectory("armory-blizzard-").FullName;
    private readonly List<Account> accounts = [];
    private readonly Armory.Store.Store server = Armory.Store.Store.InMemory();

    private static readonly CharacterKey Somechar = new("emerald-dream", "Somechar");

    public AccountBlizzardTests()
    {
        server.SetMachine("server");
    }

    // -- a fake Blizzard ---------------------------------------------------------

    private const string AccountBody = """
        {"id": 1, "wow_accounts": [{"id": 11, "characters": [
          {"id": 5, "name": "Somechar", "level": 80,
           "playable_class": {"name": "Druid"}, "playable_race": {"name": "Tauren"},
           "faction": {"type": "HORDE"},
           "realm": {"id": 61, "name": "Emerald Dream", "slug": "emerald-dream"}}]}]}
        """;

    private const string SummaryBody = """
        {"id": 5, "name": "Somechar", "level": 80, "average_item_level": 642, "equipped_item_level": 639,
         "achievement_points": 21450, "last_login_timestamp": 1785000000000,
         "active_spec": {"name": "Restoration"}, "guild": {"name": "Dream Team"}}
        """;

    private const string AchievementsBody = """
        {"achievements":[{"id": 4956, "achievement": {"id": 4956, "name": "Loremaster of Kalimdor"},
         "criteria": {"id": 10, "amount": 0, "is_completed": false}}]}
        """;

    private const string ReputationsBody = """
        {"reputations":[{"faction": {"id": 69, "name": "Darnassus"},
         "standing": {"raw": 21000, "value": 5000, "max": 21000, "name": "Revered"}}]}
        """;

    /// <summary>The routes a sync asks, answered by path. Anything else 404s, which Blizzard means as "nothing here".</summary>
    private sealed class Blizzard
    {
        public Dictionary<string, Func<HttpResponseMessage>> Routes { get; } = new(StringComparer.Ordinal)
        {
            ["/profile/user/wow?"] = () => Json(AccountBody),
            ["/profile/wow/character/emerald-dream/somechar?"] = () => Json(SummaryBody),
            ["/profile/wow/character/emerald-dream/somechar/achievements?"] = () => Json(AchievementsBody),
            ["/profile/wow/character/emerald-dream/somechar/quests/completed?"] = () => Json("""{"quests":[{"id":100},{"id":200}]}"""),
            ["/profile/wow/character/emerald-dream/somechar/reputations?"] = () => Json(ReputationsBody),
            ["/profile/user/wow/collections/mounts?"] = () => Json("""{"mounts":[{"mount":{"id":6}}]}"""),
            ["/data/wow/mount/index?"] = () => Json("""{"mounts":[{"id":6,"name":"Brown Horse"},{"id":7,"name":"Grey Ram"}]}"""),
            ["/data/wow/mount/7?"] = () => Json("""{"id":7,"name":"Grey Ram","source":{"type":"VENDOR"},"source_spell":{"id":459}}"""),
            ["/data/wow/achievement/4956?"] = () => Json("""{"id": 4956, "name": "Loremaster of Kalimdor", "points": 50, "category": {"id": 97, "name": "Quests"}}"""),
            ["oauth.battle.net/token"] = () => Json("""{"access_token":"tok-1","expires_in":86399,"token_type":"bearer"}"""),
        };

        public List<HttpTests.Seen> Asked { get; } = [];

        public HttpTests.Answering Handler => new(seen =>
        {
            Asked.Add(seen);
            foreach (var (path, answer) in Routes)
            {
                if (seen.Url.Contains(path, StringComparison.Ordinal))
                {
                    return answer();
                }
            }
            return new HttpResponseMessage(HttpStatusCode.NotFound);
        });

        public static HttpResponseMessage Json(string body)
        {
            var response = new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent(body, Encoding.UTF8, "application/json") };
            // Stamped, so the next ask can be conditional.
            response.Content.Headers.LastModified = new DateTimeOffset(2026, 9, 1, 0, 0, 0, TimeSpan.Zero);
            return response;
        }
    }

    private Account Make(string machine, Blizzard blizzard, MemorySecrets? secrets = null, bool registered = true, TimeProvider? clock = null, string? storePath = null)
    {
        secrets ??= new MemorySecrets();
        var store = storePath is null ? Armory.Store.Store.InMemory() : Armory.Store.Store.Open(storePath).Value;
        store.SetMachine(machine);
        var settings = Path.Combine(directory, machine, "settings.json");
        if (registered)
        {
            new Armory.Settings.Settings { ClientId = "client-id" }.Save(settings);
            secrets.Set(SecretNames.ClientSecret, "client-secret");
        }
        var account = new Account(
            new StoreWorker(store),
            secrets,
            settings,
            work => work().GetAwaiter().GetResult(),
            (_, _, who, _) => Result<IRemote, SyncError>.Ok(new InProcessRemote(server, who)),
            clock,
            blizzard.Handler);
        accounts.Add(account);
        return account;
    }

    private static Token ATokenForADay() => new() { Access = "tok-1", ExpiresIn = 86_399 };

    // -- the session -------------------------------------------------------------

    [Fact(DisplayName = "an exchange keeps the token and the secret, and a relaunch inside the day restores the session")]
    public async Task An_exchange_keeps_the_token_and_a_relaunch_restores_it()
    {
        var blizzard = new Blizzard();
        var secrets = new MemorySecrets();
        var account = Make("one", blizzard, secrets, registered: false);
        var reported = new List<string?>();
        account.SignInReported += reported.Add;
        var onboarding = new List<bool>();
        account.OnboardingWanted += onboarding.Add;

        await account.Exchange(new ClientCredentials { Id = "client-id", Secret = "client-secret" }, "the-code");

        Assert.Equal("tok-1", account.Token?.Access);
        // Only now is the secret known to work, which is the moment to keep it.
        Assert.Equal("client-secret", secrets.Get(SecretNames.ClientSecret));
        Assert.NotNull(secrets.Get(SecretNames.AccessToken));
        Assert.Contains(false, onboarding);
        Assert.Contains(null, reported);
        // The exchange carried the client as HTTP basic auth, and the code.
        var exchange = Assert.Single(blizzard.Asked, seen => seen.Url.Contains("oauth.battle.net/token", StringComparison.Ordinal));
        Assert.StartsWith("Basic ", exchange.Headers["Authorization"], StringComparison.Ordinal);
        Assert.Contains("code=the-code", exchange.Body, StringComparison.Ordinal);

        // A relaunch with the same vault picks the session back up.
        var again = Make("one-again", new Blizzard(), secrets);
        await again.Restore();
        Assert.Equal("tok-1", again.Token?.Access);
    }

    [Fact(DisplayName = "an expired token is not restored, and is cleared so it cannot be tried again tomorrow either")]
    public void An_expired_token_is_not_restored()
    {
        var secrets = new MemorySecrets();
        var clock = new FixedClock { Now = new DateTimeOffset(2026, 9, 14, 12, 0, 0, TimeSpan.Zero) };
        var first = Make("one", new Blizzard(), secrets, clock: clock);
        first.RememberToken(ATokenForADay());

        // Tomorrow, with half a minute left: inside the minute of margin.
        clock.Now += TimeSpan.FromSeconds(86_399 - 30);
        var later = Make("two", new Blizzard(), secrets, clock: clock);
        later.RestoreToken();
        Assert.Null(later.Token);
        Assert.Null(secrets.Get(SecretNames.AccessToken));
    }

    [Fact(DisplayName = "a bad secret is refused by Battle.net and nothing is kept")]
    public async Task A_bad_secret_is_refused_and_nothing_is_kept()
    {
        var blizzard = new Blizzard();
        blizzard.Routes["oauth.battle.net/token"] = () => new HttpResponseMessage(HttpStatusCode.BadRequest)
        {
            Content = new StringContent("""{"error":"unauthorized","error_description":"Invalid client secret"}""", Encoding.UTF8, "application/json"),
        };
        var secrets = new MemorySecrets();
        var account = Make("one", blizzard, secrets, registered: false);
        var reported = new List<string?>();
        account.SignInReported += reported.Add;

        await account.Exchange(new ClientCredentials { Id = "client-id", Secret = "wrong" }, "the-code");

        Assert.Null(account.Token);
        Assert.Null(secrets.Get(SecretNames.ClientSecret));
        Assert.Contains(reported, text => text is not null && text.Contains("Battle.net refused", StringComparison.Ordinal));
    }

    [Fact(DisplayName = "signing in opens the browser at the authorize URL, and the redirect coming back completes the exchange")]
    public async Task Signing_in_opens_the_browser_and_the_redirect_completes_it()
    {
        var blizzard = new Blizzard();
        var secrets = new MemorySecrets();
        var account = Make("one", blizzard, secrets, registered: false);
        string? opened = null;
        account.OpenBrowser = url =>
        {
            opened = url;
            return Task.CompletedTask;
        };

        await account.SignIn("client-id", "client-secret", Region.Eu);

        Assert.True(account.SigningIn);
        Assert.NotNull(opened);
        Assert.StartsWith("https://oauth.battle.net/authorize?", opened, StringComparison.Ordinal);
        Assert.Contains("client_id=client-id", opened, StringComparison.Ordinal);
        // The id and region are saved now, so a failed sign-in does not make
        // someone paste them again; the secret waits for Battle.net's word.
        Assert.Equal("client-id", account.Settings.ClientId);
        Assert.Equal(Region.Eu, account.Settings.Region);
        Assert.Null(secrets.Get(SecretNames.ClientSecret));

        var state = opened!.Split("state=")[1].Split('&')[0];
        using (var browser = new HttpClient())
        {
            var landing = await browser.GetStringAsync(new Uri($"{OAuth.RedirectUri}?code=the-code&state={state}"), TestContext.Current.CancellationToken);
            Assert.Contains("Signed in", landing, StringComparison.Ordinal);
        }
        // The redirect is delivered through the dispatcher, which runs inline here.
        await WaitFor(() => account.Token is not null);
        Assert.Equal("tok-1", account.Token?.Access);
        Assert.False(account.SigningIn, "the port is released once the code is in");
        Assert.Equal("client-secret", secrets.Get(SecretNames.ClientSecret));
    }

    [Fact(DisplayName = "a blank secret means use the stored one, and a missing one is asked for")]
    public async Task A_blank_secret_means_use_the_stored_one()
    {
        var secrets = new MemorySecrets();
        var account = Make("one", new Blizzard(), secrets, registered: false);
        var reported = new List<string?>();
        account.SignInReported += reported.Add;
        account.OpenBrowser = _ => Task.CompletedTask;

        await account.SignIn("client-id", "", Region.Us);
        Assert.Contains(reported, text => text is not null && text.Contains("none saved", StringComparison.Ordinal));
        Assert.False(account.SigningIn);

        secrets.Set(SecretNames.ClientSecret, "held");
        await account.SignIn("client-id", "   ", Region.Us);
        Assert.True(account.SigningIn);
    }

    [Fact(DisplayName = "signing out forgets the token, the secret and the client, and shows setup again")]
    public void Signing_out_forgets_the_session_and_shows_setup()
    {
        var secrets = new MemorySecrets();
        var account = Make("one", new Blizzard(), secrets);
        account.HoldToken(ATokenForADay());
        account.RememberToken(ATokenForADay());
        var onboarding = new List<bool>();
        account.OnboardingWanted += onboarding.Add;

        account.SignOut();

        Assert.Null(account.Token);
        Assert.Null(secrets.Get(SecretNames.AccessToken));
        Assert.Null(secrets.Get(SecretNames.ClientSecret));
        Assert.Equal("", account.Settings.ClientId);
        // Or the next launch hides setup and there is no way back to it.
        Assert.False(account.Settings.AddonOnly);
        Assert.Equal([true], onboarding);
    }

    // -- the sync ------------------------------------------------------------------

    [Fact(DisplayName = "a sync fills the roster, and once somebody is enrolled the details, inputs, collections, catalogue and reputations")]
    public async Task A_sync_fills_the_roster_then_everything_that_hangs_off_the_cohort()
    {
        var blizzard = new Blizzard();
        var account = Make("one", blizzard);
        account.HoldToken(ATokenForADay());
        var toasts = new List<string>();
        account.Toasted += toasts.Add;

        await account.Sync();
        Assert.Equal("Somechar", Assert.Single(account.Roster.Characters).DisplayName);
        Assert.Contains(toasts, toast => toast.Contains("Synced 1 characters", StringComparison.Ordinal));
        // Nobody enrolled yet, so nothing per character was asked.
        Assert.DoesNotContain(blizzard.Asked, seen => seen.Url.Contains("/profile/wow/character/", StringComparison.Ordinal));

        await account.ToggleEnrolment(Somechar);
        await account.Sync();

        var detail = account.Details[Somechar];
        Assert.Equal(642, detail.ItemLevel);
        Assert.Equal("Restoration", detail.Spec);
        Assert.Equal(4956, Assert.Single(account.Inputs.Progress).Id);
        Assert.Contains(200L, account.Inputs.Primary[Somechar].Quests);
        Assert.Equal(21000, account.Inputs.Primary[Somechar].Reputations[69]);
        Assert.Equal("Revered", Assert.Single(account.Reputations[Somechar]).Tier);
        Assert.Contains(6L, account.Inputs.Owned);
        Assert.Equal("Loremaster of Kalimdor", account.Inputs.Catalogue[4956].Name);

        var (catalogue, owned) = await account.Store.On(store => store.CollectiblesHeld(Kind.Mount).Value);
        Assert.Equal(2, catalogue.Count);
        Assert.Equal([6L], owned);
        // The index has names and no sources; the source came from the detail call for the one still unknown.
        Assert.Equal(Source.Vendor, catalogue.Single(entry => entry.Id == 7).Source);
        // And it is all in the store, so a relaunch reads it back.
        Assert.Equal(1, await account.Store.On(store => store.RosterHeld().Value.Count));
        Assert.Equal(642, (await account.Store.On(store => store.Details().Value))[Somechar].ItemLevel);
    }

    [Fact(DisplayName = "an unchanged answer keeps the cached body, so a character who has not played costs a round trip and no bytes")]
    public async Task An_unchanged_answer_keeps_the_cached_body()
    {
        var blizzard = new Blizzard();
        var account = Make("one", blizzard);
        account.HoldToken(ATokenForADay());
        var toasts = new List<string>();
        account.Toasted += toasts.Add;
        await account.Sync();
        await account.ToggleEnrolment(Somechar);
        await account.Sync();
        Assert.Equal(642, account.Details[Somechar].ItemLevel);

        // Blizzard now says nothing changed, for the account and the character both.
        blizzard.Routes["/profile/user/wow?"] = () => new HttpResponseMessage(HttpStatusCode.NotModified);
        blizzard.Routes["/profile/wow/character/emerald-dream/somechar?"] = () => new HttpResponseMessage(HttpStatusCode.NotModified);
        blizzard.Asked.Clear();
        await account.Sync();

        Assert.Contains("Nothing has changed since the last sync.", toasts);
        Assert.Equal(642, account.Details[Somechar].ItemLevel);
        Assert.Equal("Somechar", Assert.Single(account.Roster.Characters).DisplayName);
        // Conditional on the stamp the store holds: the request said so.
        var summary = blizzard.Asked.First(seen => seen.Url.Contains("/somechar?", StringComparison.Ordinal));
        Assert.True(summary.Headers.ContainsKey("If-Modified-Since"));
    }

    [Fact(DisplayName = "a sign-out mid-sync drops what lands afterwards rather than writing it into the signed-out account")]
    public async Task A_sign_out_mid_sync_drops_what_lands_afterwards()
    {
        var blizzard = new Blizzard();
        var account = Make("one", blizzard);
        account.HoldToken(ATokenForADay());
        await account.Sync();
        await account.ToggleEnrolment(Somechar);

        // The summary answers only after the person has signed out.
        blizzard.Routes["/profile/wow/character/emerald-dream/somechar?"] = () =>
        {
            account.NextGeneration();
            return Blizzard.Json(SummaryBody);
        };
        await account.Sync();

        Assert.False(account.Details.TryGetValue(Somechar, out var detail) && detail.ItemLevel is not null, "a stale generation's answer was written");
    }

    [Fact(DisplayName = "an expired session is said out loud, forgotten, and setup is shown with the secret still held")]
    public async Task An_expired_session_is_forgotten_and_setup_shown()
    {
        var blizzard = new Blizzard();
        blizzard.Routes["/profile/user/wow?"] = () => new HttpResponseMessage(HttpStatusCode.Unauthorized);
        var secrets = new MemorySecrets();
        var account = Make("one", blizzard, secrets);
        account.HoldToken(ATokenForADay());
        account.RememberToken(ATokenForADay());
        var toasts = new List<string>();
        account.Toasted += toasts.Add;
        var onboarding = new List<bool>();
        account.OnboardingWanted += onboarding.Add;

        await account.Sync();
        Assert.Contains(toasts, toast => toast.StartsWith("Sync failed", StringComparison.Ordinal));
        // The stored copy is the same dead token.
        Assert.Null(account.Token);
        Assert.Null(secrets.Get(SecretNames.AccessToken));

        await account.Sync();
        Assert.Contains(toasts, toast => toast.StartsWith("Sign in again to sync", StringComparison.Ordinal));
        Assert.Equal([true], onboarding);
        // The secret is what makes the sign-in button all it takes.
        Assert.Equal("client-secret", secrets.Get(SecretNames.ClientSecret));
    }

    [Fact(DisplayName = "addon-only, or no client at all, asks Blizzard nothing")]
    public async Task Addon_only_asks_blizzard_nothing()
    {
        var blizzard = new Blizzard();
        var account = Make("one", blizzard, registered: false);
        var toasts = new List<string>();
        account.Toasted += toasts.Add;
        var onboarding = new List<bool>();
        account.OnboardingWanted += onboarding.Add;

        account.SkipApi();
        Assert.True(account.Settings.AddonOnly);
        Assert.False(account.NeedsOnboarding);
        Assert.Equal([false], onboarding);

        await account.Sync();
        Assert.Contains(toasts, toast => toast.StartsWith("Set up a Battle.net client first", StringComparison.Ordinal));
        Assert.Empty(blizzard.Asked);
    }

    [Fact(DisplayName = "reputations and artwork are read back out of the response cache at launch")]
    public async Task Reputations_and_artwork_are_restored_from_the_cache()
    {
        var blizzard = new Blizzard();
        blizzard.Routes["/data/wow/media/achievement/4956?"] = () => Blizzard.Json("""{"assets":[{"key":"icon","value":"https://render.worldofwarcraft.com/us/icons/56/achievement_zone_kalimdor_01.jpg"}]}""");
        blizzard.Routes["/profile/wow/character/emerald-dream/somechar/character-media?"] = () => Blizzard.Json("""{"assets":[{"key":"avatar","value":"https://render.worldofwarcraft.com/us/character/emerald-dream/5/5-avatar.jpg"}]}""");
        var path = Path.Combine(directory, "one.db");
        var account = Make("one", blizzard, storePath: path);
        account.HoldToken(ATokenForADay());
        account.AchievementArtWanted = _ => [4956];
        await account.Sync();
        await account.ToggleEnrolment(Somechar);
        await account.Sync();
        Assert.Single(account.AchievementArt);
        Assert.Single(account.Portraits);
        Assert.Single(account.Reputations);
        account.Dispose();
        accounts.Remove(account);

        // A relaunch over the same database: the maps are memory, and this
        // is what makes them survive.
        var again = Make("one", new Blizzard(), storePath: path);
        await again.Restore();
        Assert.Equal("https://render.worldofwarcraft.com/us/icons/56/achievement_zone_kalimdor_01.jpg", again.AchievementArt[4956]);
        Assert.EndsWith("5-avatar.jpg", again.Portraits[Somechar], StringComparison.Ordinal);
        Assert.Equal("Revered", Assert.Single(again.Reputations[Somechar]).Tier);
    }

    private static async Task WaitFor(Func<bool> condition)
    {
        for (var i = 0; i < 200 && !condition(); i++)
        {
            await Task.Delay(25, TestContext.Current.CancellationToken);
        }
    }

    public void Dispose()
    {
        foreach (var account in accounts)
        {
            account.Dispose();
        }
        server.Dispose();
        try
        {
            Directory.Delete(directory, recursive: true);
        }
        catch (IOException)
        {
        }
    }
}
