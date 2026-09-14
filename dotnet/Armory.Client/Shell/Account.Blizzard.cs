using System.Globalization;
using System.Text.Json;
using System.Text.Json.Nodes;
using Armory.Blizzard;
using Armory.Client.Blizzard;
using Armory.Collections;
using Armory.Roster;
using Armory.Run;

namespace Armory.Client.Shell;

/// <summary>
/// The Battle.net half of the account: signing in, and the sync that reads
/// the roster, the run's inputs, the collections, the catalogue, the guide
/// and the artwork through the response cache. The port of the GTK
/// application's <c>sign_in</c>, <c>exchange</c>, <c>sync</c> and the
/// fan-out under it. A sync is one account request, then everything else in
/// parallel, then one re-measure once everything has landed — the GTK build
/// merged each answer as it arrived and re-measured when its counter of
/// outstanding calls reached zero, which is the same thing said with
/// callbacks.
/// </summary>
public sealed partial class Account
{
    /// <summary>How many pieces of artwork to ask for per sync. Two thousand toys is two thousand requests, and nobody is looking at two thousand toys.</summary>
    public const int ArtPerSync = 120;

    /// <summary>How many market item names to ask for per sync.</summary>
    public const int NamesPerSync = 150;

    /// <summary>How many Adventure Guide entries to fill in per sync. Nothing here ever changes once fetched, so this converges.</summary>
    public const int GuidePerSync = 200;

    /// <summary>How many achievement names to fill in per sync. The gaps read as "Achievement 4956" until they fill in, which is ugly and honest.</summary>
    public const int CataloguePerSync = 200;

    /// <summary>How many collectible sources to fill in per sync. The index has names and no sources.</summary>
    public const int DetailsPerSync = 150;

    private Redirect? redirect;
    private bool syncing;

    /// <summary>Render URLs that had to be asked for: a portrait per enrolled character.</summary>
    public Dictionary<CharacterKey, string> Portraits { get; } = [];

    /// <summary>Icon URLs by item id. Toys and decor share the map because both are items.</summary>
    public Dictionary<long, string> ToyArt { get; } = [];

    public Dictionary<long, string> AchievementArt { get; } = [];

    /// <summary>Reputations per enrolled character, with names and tiers. Held rather than stored; the bodies are in the response cache.</summary>
    public Dictionary<CharacterKey, List<FactionStanding>> Reputations { get; } = [];

    /// <summary>Whether a sign-in is waiting on the browser.</summary>
    public bool SigningIn => redirect is not null;

    /// <summary>Whether a sync is in flight.</summary>
    public bool Syncing => syncing;

    /// <summary>Raised with a sentence for the onboarding page, or null to clear it.</summary>
    public event Action<string?>? SignInReported;

    /// <summary>Raised when the shell should show or hide onboarding.</summary>
    public event Action<bool>? OnboardingWanted;

    /// <summary>How the shell opens a URL in the browser. Set by the app; a test captures the URL.</summary>
    public Func<string, Task>? OpenBrowser { get; set; }

    /// <summary>What toy and decor icons the page in front of somebody wants, in the order it wants them. Set by the collection pages.</summary>
    public Func<Kind, int, IReadOnlyList<long>>? ItemArtWanted { get; set; }

    /// <summary>What achievement icons the run page wants.</summary>
    public Func<int, IReadOnlyList<long>>? AchievementArtWanted { get; set; }

    /// <summary>Which market item ids the market page wants names for, filtered and ordered as shown.</summary>
    public Func<int, IReadOnlyList<long>>? NamesWanted { get; set; }

    private IEnumerable<Character> Members => Roster.Characters.Where(character => Cohort.Contains(character.Key));

    // -- the session -----------------------------------------------------------

    /// <summary>The client secret, if the vault is holding one.</summary>
    public string? StoredSecret() => secrets.Get(SecretNames.ClientSecret) is { Length: > 0 } held ? held : null;

    /// <summary>
    /// Keep the access token, so a relaunch inside the day does not need a
    /// browser. Battle.net issues no refresh token, so the only way back from
    /// an expired session is the whole flow; the token lasts a day, and
    /// storing it turns "sign in every launch" into "sign in tomorrow". The
    /// vault rather than the settings file, because a bearer token is a
    /// credential.
    /// </summary>
    public void RememberToken(Token token)
    {
        var until = clock.GetUtcNow() + TimeSpan.FromSeconds(token.ExpiresIn);
        var held = new JsonObject { ["access"] = token.Access, ["until"] = until.ToString("o", CultureInfo.InvariantCulture) };
        secrets.Set(SecretNames.AccessToken, held.ToJsonString());
    }

    /// <summary>The stored token, if there is one and it has not expired.</summary>
    public void RestoreToken()
    {
        var held = secrets.Get(SecretNames.AccessToken);
        if (string.IsNullOrWhiteSpace(held))
        {
            return;
        }
        JsonNode? value;
        try
        {
            value = JsonNode.Parse(held);
        }
        catch (JsonException)
        {
            return;
        }
        var access = value.At("access").Str();
        var untilText = value.At("until").Str();
        if (access is null || untilText is null || !DateTimeOffset.TryParse(untilText, CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind, out var until))
        {
            return;
        }

        // A minute of margin. A token that expires mid-sync fails every call
        // after it does, and the failures read as Blizzard being down.
        var remaining = until - clock.GetUtcNow();
        if (remaining < TimeSpan.FromMinutes(1))
        {
            secrets.Clear(SecretNames.AccessToken);
            return;
        }
        HoldToken(new Token { Access = access, ExpiresIn = (long)remaining.TotalSeconds, Refresh = null });
    }

    /// <summary>Forget the session, on the way out of one.</summary>
    public void ForgetToken()
    {
        HoldToken(null);
        secrets.Clear(SecretNames.AccessToken);
    }

    /// <summary>
    /// Begin the authorization code flow. A blank secret means "use the one
    /// you already have": the field cannot be pre-filled without reading the
    /// secret out of the vault to display it, so an empty one falls back
    /// rather than refusing. The id and region are saved now, so a failed
    /// sign-in does not make someone paste them again; the secret goes to the
    /// vault only once Battle.net has confirmed it works.
    /// </summary>
    public async Task SignIn(string clientId, string typedSecret, Region region)
    {
        var secret = typedSecret.Trim().Length == 0 ? StoredSecret() ?? "" : typedSecret.Trim();
        var client = new ClientCredentials { Id = clientId.Trim(), Secret = secret };
        if (client.Id.Length == 0)
        {
            SignInReported?.Invoke("Fill in the client ID first.");
            return;
        }
        if (client.Secret.Length == 0)
        {
            SignInReported?.Invoke("Fill in the client secret. Armory has none saved for this client.");
            return;
        }
        SaveSettings(Settings with { ClientId = client.Id, Region = region });

        // Not a cryptographic nonce and not pretending to be one: with no PKCE
        // available this only has to be unpredictable enough that a redirect
        // Armory did not start does not match one it did.
        var state = Guid.NewGuid().ToString("N") + clock.GetUtcNow().ToUnixTimeMilliseconds().ToString("x", CultureInfo.InvariantCulture);

        redirect?.Dispose();
        var listening = Redirect.Listen(state, result => dispatch(() => CameBack(client, result)));
        if (!listening.IsOk)
        {
            SignInReported?.Invoke($"Armory could not listen on port {OAuth.RedirectPort} for Battle.net to send you back ({listening.Error}). Close whatever is using that port and try again.");
            return;
        }
        redirect = listening.Value;
        SignInReported?.Invoke("Armory opened Battle.net in your browser. Sign in there, and this window will carry on by itself.");
        if (OpenBrowser is { } open)
        {
            await open(OAuth.AuthorizeUrl(client, state));
        }
    }

    /// <summary>The browser came back, with a code or a refusal.</summary>
    private async Task CameBack(ClientCredentials client, Result<string, Reason> result)
    {
        if (result.IsOk)
        {
            await Exchange(client, result.Value);
            return;
        }
        redirect?.Dispose();
        redirect = null;
        SignInReported?.Invoke($"Sign-in did not finish: {result.Error}");
    }

    /// <summary>Trade the code for a token, and remember the secret if it worked.</summary>
    public async Task Exchange(ClientCredentials client, string code)
    {
        var outcome = await Blizzard.Fetch(OAuth.Exchange(client, code));
        redirect?.Dispose();
        redirect = null;

        if (outcome is not Outcome<Response>.Found found)
        {
            var reason = outcome.Gap() ?? new Reason.Malformed("no answer from Battle.net");
            SignInReported?.Invoke($"Battle.net refused: {reason}");
            return;
        }
        var parsed = OAuth.ParseToken(found.Value.Body);
        if (parsed is not Outcome<Token>.Found token)
        {
            var reason = parsed.Gap() ?? new Reason.Malformed("an empty token");
            SignInReported?.Invoke($"Battle.net refused: {reason}");
            return;
        }

        // Only now is the secret known to work, which is the right moment to keep it.
        secrets.Set(SecretNames.ClientSecret, client.Secret);
        RememberToken(token.Value);
        HoldToken(token.Value);
        SignInReported?.Invoke(null);
        OnboardingWanted?.Invoke(false);
        await Sync();
    }

    /// <summary>
    /// Go on without a Battle.net client. Recorded so the choice survives a
    /// restart and onboarding does not reappear asking a question already
    /// answered.
    /// </summary>
    public void SkipApi()
    {
        SaveSettings(Settings with { AddonOnly = true });
        OnboardingWanted?.Invoke(false);
        if (watch is null)
        {
            Toasted?.Invoke("Install the collector addon and log in once — Armory has nothing to read yet.");
        }
        Changed?.Invoke();
    }

    /// <summary>Show the setup page again. Nothing is cleared: somebody adding a client has not asked to lose their run.</summary>
    public void ShowSetup()
    {
        SignInReported?.Invoke(null);
        OnboardingWanted?.Invoke(true);
    }

    /// <summary>Sign out: the token, the secret and the client id go; the characters and enrolments stay.</summary>
    public void SignOut()
    {
        ForgetToken();
        secrets.Clear(SecretNames.ClientSecret);
        // `AddonOnly` off again, or the next launch hides setup and there is
        // no way back to it.
        SaveSettings(Settings with { ClientId = "", AddonOnly = false });
        NextGeneration();
        SignInReported?.Invoke(null);
        OnboardingWanted?.Invoke(true);
        Toasted?.Invoke("Signed out. Your characters and enrolments are still here.");
        Changed?.Invoke();
    }

    // -- restoring ---------------------------------------------------------------

    /// <summary>
    /// Re-read reputations out of the response cache. Profile data is a
    /// logout snapshot, so the body from the last sync is still Blizzard's
    /// answer until somebody plays, and parsing it back is cheaper than a
    /// second schema.
    /// </summary>
    public async Task RestoreReputations()
    {
        var region = Settings.Region;
        var members = Members.ToList();
        var read = await Store.On(store => members
            .Select(character => (character, body: store.Response(Profile.ReputationsOf(region, character.Key).Url, TimeSpan.FromDays(Armory.Store.Store.MaxTtlDays)).Match(body => body, _ => null)))
            .Where(pair => pair.body is not null)
            .ToList());
        foreach (var (character, body) in read)
        {
            if (Profile.ParseReputations(body!, character.Level) is Outcome<Reputations>.Found found)
            {
                Reputations[character.Key] = found.Value.Detail;
            }
        }
    }

    /// <summary>
    /// Re-read the artwork URLs out of the response cache. The art maps are
    /// memory, and without this every launch would start with no pictures
    /// and spend its whole per-sync budget re-earning URLs it already had
    /// the bodies for.
    /// </summary>
    public async Task RestoreArt()
    {
        var region = Settings.Region;
        var ttl = TimeSpan.FromDays(Armory.Store.Store.MaxTtlDays);
        var members = Members.ToList();
        var read = await Store.On(store => (
            items: store.ResponsesMatching(Media.ItemMedia, ttl).Match(bodies => bodies, _ => []),
            achievements: store.ResponsesMatching(Media.AchievementMedia, ttl).Match(bodies => bodies, _ => []),
            portraits: members
                .Select(character => (character.Key, body: store.Response(Media.Character(region, character.Key).Url, ttl).Match(body => body, _ => null)))
                .Where(pair => pair.body is not null)
                .ToList()));

        // Which id a media body describes is not in the body, so it is read
        // back off the URL it is filed under.
        foreach (var (needle, art, bodies) in new[] { (Media.ItemMedia, ToyArt, read.items), (Media.AchievementMedia, AchievementArt, read.achievements) })
        {
            foreach (var (url, body) in bodies)
            {
                if (Media.MediaId(url, needle) is { } id && Media.ParseIcon(body) is Outcome<string>.Found icon)
                {
                    art[id] = icon.Value;
                }
            }
        }
        foreach (var (key, body) in read.portraits)
        {
            if (Media.ParsePortrait(body!, Portrait.Avatar) is Outcome<string>.Found portrait)
            {
                Portraits[key] = portrait.Value;
            }
        }
    }

    // -- syncing -----------------------------------------------------------------

    /// <summary>
    /// Sync with Battle.net: the roster first, because it says who is
    /// enrolled, then everything that hangs off it. Nothing without a client
    /// and a token; a lapsed session says so and shows setup again, with the
    /// secret still held.
    /// </summary>
    public async Task Sync()
    {
        if (syncing)
        {
            return;
        }
        if (Credentials() is null)
        {
            Toasted?.Invoke("Set up a Battle.net client first, under Setup.");
            return;
        }
        if (Token is not { } token)
        {
            Toasted?.Invoke("Sign in again to sync — Battle.net sessions last a day. Your secret is still saved, so the sign-in button is all it takes.");
            ShowSetup();
            return;
        }

        syncing = true;
        var generation = NextGeneration();
        try
        {
            var region = Settings.Region;
            var request = Profile.Account(region).Bearer(token.Access);
            var url = request.Url;
            var stamp = await Store.On(store => store.LastModified(url).Match(stamp => stamp, _ => null));
            if (stamp is not null)
            {
                request = request.IfModifiedSince(stamp);
            }
            var outcome = await Blizzard.Fetch(request);
            if (Generation != generation)
            {
                return;
            }
            syncing = false;
            await FinishAccountSync(url, outcome);

            if (Token is not { } still || Generation != generation)
            {
                return;
            }
            await Task.WhenAll(
                SyncCohort(still, generation),
                // The catalogue after the inputs, because the inputs are what
                // say which names are wanted: the GTK build dispatched both
                // at once and the first sync of a fresh account named nothing.
                InputsThenCatalogue(still, generation),
                SyncCollections(still, generation),
                SyncMarket(still, generation),
                // Artwork last and capped: it is the part of a sync nothing
                // depends on, and a page that draws with placeholders is still
                // a page that works.
                SyncMedia(still, generation, ArtPerSync),
                SyncItemNames(still, generation, NamesPerSync),
                SyncGuide(still, generation, GuidePerSync));
        }
        finally
        {
            syncing = false;
        }
        Changed?.Invoke();
    }

    private async Task InputsThenCatalogue(Token token, long generation)
    {
        await SyncRunInputs(token, generation);
        await SyncCatalogue(token, generation);
    }

    private async Task FinishAccountSync(string url, Outcome<Response> outcome)
    {
        switch (outcome)
        {
            case Outcome<Response>.Unchanged:
                await Store.On(store => store.TouchResponse(url));
                Toasted?.Invoke("Nothing has changed since the last sync.");
                break;
            case Outcome<Response>.Found found:
                var parsed = Profile.ParseAccount(found.Value.Body);
                if (parsed is Outcome<Armory.Roster.Roster>.Found roster)
                {
                    var cohort = new Cohort(Cohort.Members);
                    cohort.Prune(roster.Value);
                    await Store.On(store =>
                    {
                        store.StoreResponse(url, found.Value.Body, found.Value.LastModified);
                        store.SaveRoster(roster.Value);
                        store.SaveCohort(cohort);
                    });
                    Roster = roster.Value;
                    Cohort = cohort;
                    Changed?.Invoke();
                    Toasted?.Invoke($"Synced {roster.Value.Count} characters.");
                }
                else
                {
                    var reason = parsed.Gap() ?? new Reason.Malformed("Blizzard sent an account with no characters");
                    Toasted?.Invoke($"Could not read the account: {reason}");
                }
                break;
            case Outcome<Response>.Empty:
                Toasted?.Invoke("Battle.net has no characters for this account yet.");
                break;
            default:
                if (outcome.Gap() is { } gap)
                {
                    Toasted?.Invoke($"Sync failed: {gap}");
                    if (gap is Reason.Unauthorised)
                    {
                        // The stored copy is the same dead token. Leaving it
                        // there would restore it at the next launch and fail
                        // again in the same way.
                        ForgetToken();
                    }
                }
                break;
        }
    }

    /// <summary>
    /// Fetch the expensive half for every enrolled character. Every request
    /// is conditional on the stamp already held, so a character who has not
    /// logged out since the last sync answers 304 and almost no bytes. Each
    /// field is optional and merged rather than assigned: a character whose
    /// professions time out still gets an item level.
    /// </summary>
    public async Task SyncCohort(Token token, long generation)
    {
        var region = Settings.Region;
        var members = Members.ToList();
        var fetches = new List<Task<(CharacterKey Key, Func<Detail, Detail> Merge)?>>();
        foreach (var character in members)
        {
            var key = character.Key;
            fetches.Add(FetchDetail(token, generation, key, Profile.Summary(region, key), body =>
                Profile.ParseSummary(body) is Outcome<Detail>.Found summary
                    ? detail => detail with
                    {
                        ItemLevel = summary.Value.ItemLevel,
                        EquippedItemLevel = summary.Value.EquippedItemLevel,
                        Spec = summary.Value.Spec,
                        Guild = summary.Value.Guild,
                        AchievementPoints = summary.Value.AchievementPoints,
                        LastLogin = summary.Value.LastLogin,
                    }
                    : null));
            fetches.Add(FetchDetail(token, generation, key, Profile.Professions(region, key), body =>
                Profile.ParseProfessions(body) is Outcome<List<Profession>>.Found professions
                    // Merged, not assigned: the API has the expansion tier
                    // and the addon has the specialisation trees and the
                    // knowledge spent, and neither knows the other's half.
                    ? detail => detail with
                    {
                        Professions = professions.Value.Select(fetched =>
                            detail.Professions.FirstOrDefault(held => held.Name == fetched.Name) is { } known
                                ? fetched with { Specialisations = known.Specialisations, Knowledge = known.Knowledge }
                                : fetched).ToList(),
                    }
                    : null));
            // Assigned from the answer, and `Empty` is an answer: no keys this
            // season is no rating, not last season's.
            fetches.Add(FetchDetail(token, generation, key, Profile.MythicKeystone(region, key), body => Profile.ParseMythicKeystone(body) switch
            {
                Outcome<long>.Found rating => detail => detail with { MythicRating = rating.Value },
                Outcome<long>.Empty => detail => detail with { MythicRating = null },
                _ => null,
            }));
            // Assigned rather than merged: the addon and the API answer this
            // one in the same shape, so whichever looked last is the more
            // recent look. `Empty` is left alone on purpose — a character who
            // came back naked is a real answer and a broken parser is not.
            fetches.Add(FetchDetail(token, generation, key, Profile.Equipment(region, key), body =>
                Profile.ParseEquipment(body) is Outcome<List<Equipped>>.Found worn ? detail => detail with { Equipment = worn.Value } : null));
            // Lifetime raid progress. The addon cannot answer this one.
            fetches.Add(FetchDetail(token, generation, key, Profile.RaidEncounters(region, key), body =>
                Profile.ParseRaids(body) is Outcome<List<RaidTier>>.Found raids ? detail => detail with { Raids = raids.Value } : null));
            fetches.Add(FetchDetail(token, generation, key, Profile.ReputationsOf(region, key), body =>
            {
                try
                {
                    return JsonNode.Parse(body) is { } value ? detail => detail with { Renown = Profile.HighestRenown(value) } : null;
                }
                catch (JsonException)
                {
                    return null;
                }
            }));
            // Gold, from the one endpoint that has it.
            fetches.Add(FetchDetail(token, generation, key, Profile.ProtectedCharacter(region, character), body => Profile.ParseProtected(body) switch
            {
                Outcome<long>.Found money => detail => detail with { Money = money.Value },
                Outcome<long>.Empty => detail => detail with { Money = null },
                _ => null,
            }));
        }

        var landed = await Task.WhenAll(fetches);
        if (Generation != generation)
        {
            return;
        }
        var details = new Dictionary<CharacterKey, Detail>(Details);
        var touched = new HashSet<CharacterKey>();
        foreach (var merge in landed)
        {
            if (merge is not var (key, apply))
            {
                continue;
            }
            details[key] = apply(details.GetValueOrDefault(key) ?? new Detail());
            touched.Add(key);
        }
        Details = details;
        if (touched.Count > 0)
        {
            await Store.On(store =>
            {
                foreach (var key in touched)
                {
                    store.SaveDetail(key, details[key]);
                }
            });
            Changed?.Invoke();
        }
    }

    /// <summary>One per-character call, answering what to do to that character's detail once its body is in. Null when nothing landed.</summary>
    private async Task<(CharacterKey Key, Func<Detail, Detail> Merge)?> FetchDetail(Token token, long generation, CharacterKey key, Request request, Func<byte[], Func<Detail, Detail>?> read)
    {
        var body = await FetchBare(token, generation, request);
        if (body is null)
        {
            return null;
        }
        return read(body) is { } merge ? (key, merge) : null;
    }

    /// <summary>
    /// Fetch the achievement progress and primary data a run is measured
    /// from. Achievements come from one enrolled character: the response is
    /// account-wide, so asking every character would be twenty-three copies
    /// of the same answer. Primary data is genuinely per character.
    /// </summary>
    public async Task SyncRunInputs(Token token, long generation)
    {
        var region = Settings.Region;
        var members = Members.ToList();
        if (members.Count == 0)
        {
            return;
        }
        var first = members[0].Key;
        var fetches = new List<Task<Func<Inputs, Inputs>?>>
        {
            FetchInput(token, generation, Profile.Achievements(region, first), body =>
                Profile.ParseAchievements(body) is Outcome<List<AchievementProgress>>.Found progress ? inputs => inputs with { Progress = progress.Value } : null),
        };
        foreach (var character in members)
        {
            var key = character.Key;
            var level = character.Level;
            fetches.Add(FetchInput(token, generation, Profile.CompletedQuests(region, key), body =>
                Profile.ParseCompletedQuests(body) is Outcome<HashSet<long>>.Found quests
                    ? inputs => WithPrimary(inputs, key, primary => primary with { Quests = quests.Value })
                    : null));
            fetches.Add(FetchInput(token, generation, Profile.Statistics(region, key), body =>
                Profile.ParseStatistics(body) is Outcome<Dictionary<long, double>>.Found statistics
                    ? inputs => WithPrimary(inputs, key, primary => primary with { Statistics = statistics.Value })
                    : null));
            fetches.Add(FetchInput(token, generation, Profile.ReputationsOf(region, key), body =>
                Profile.ParseReputations(body, level) is Outcome<Reputations>.Found reputations
                    ? inputs =>
                    {
                        Reputations[key] = reputations.Value.Detail;
                        return WithPrimary(inputs, key, primary => primary with { Reputations = reputations.Value.Standings, InheritedReputations = reputations.Value.Inherited });
                    }
            : null));
            // Dungeons and raids land in the same set: a criterion refers to
            // an encounter by id and does not care which endpoint it came from.
            foreach (var request in new[] { Profile.DungeonEncounters(region, key), Profile.RaidEncounters(region, key) })
            {
                fetches.Add(FetchInput(token, generation, request, body =>
                    Profile.ParseEncounters(body) is Outcome<HashSet<long>>.Found encounters
                        ? inputs => WithPrimary(inputs, key, primary => primary with { Encounters = [.. primary.Encounters, .. encounters.Value] })
                        : null));
            }
        }

        var landed = await Task.WhenAll(fetches);
        if (Generation != generation)
        {
            return;
        }
        var inputs = Inputs;
        var any = false;
        foreach (var merge in landed)
        {
            if (merge is null)
            {
                continue;
            }
            inputs = merge(inputs);
            any = true;
        }
        if (!any)
        {
            return;
        }
        Inputs = inputs;
        // Re-measure only when everything has landed. Doing it per response
        // would show half-computed progress on the way.
        await Remeasure();
    }

    private static Inputs WithPrimary(Inputs inputs, CharacterKey key, Func<PrimaryData, PrimaryData> change)
    {
        var primary = new Dictionary<CharacterKey, PrimaryData>(inputs.Primary);
        primary[key] = change(primary.GetValueOrDefault(key) ?? new PrimaryData());
        return inputs with { Primary = primary };
    }

    /// <summary>One call whose answer feeds the planner rather than a roster row.</summary>
    private async Task<Func<Inputs, Inputs>?> FetchInput(Token token, long generation, Request request, Func<byte[], Func<Inputs, Inputs>?> read)
    {
        var body = await FetchBare(token, generation, request);
        return body is null ? null : read(body);
    }

    /// <summary>
    /// Fill in achievement names, a batch at a time. The catalogue is one
    /// call per achievement and there are several thousand, so this asks
    /// about the ones the run is actually going to show, capped per sync.
    /// Names never change, so what lands is kept and never asked for again.
    /// </summary>
    public async Task SyncCatalogue(Token token, long generation)
    {
        var region = Settings.Region;
        var known = Inputs.Catalogue.Keys.ToHashSet();
        // The run's own goals first, because those are the rows on screen.
        var wanted = Run is { } held
            ? held.Run.Goals.Select(goal => goal.AchievementId)
            : Inputs.Progress.Select(progress => progress.Id);
        var missing = wanted.Where(id => !known.Contains(id)).Distinct().Take(CataloguePerSync).ToList();
        if (missing.Count == 0)
        {
            return;
        }

        var landed = await Task.WhenAll(missing.Select(async id =>
        {
            var body = await FetchBare(token, generation, GameData.AchievementOf(region, id));
            return body is not null && GameData.ParseAchievement(body) is Outcome<Achievement>.Found found ? found.Value : null;
        }));
        if (Generation != generation)
        {
            return;
        }
        var achievements = landed.Where(achievement => achievement is not null).Select(achievement => achievement!).ToList();
        if (achievements.Count == 0)
        {
            return;
        }
        await Store.On(store => store.SaveAchievements(achievements));
        var catalogue = new Dictionary<long, Achievement>(Inputs.Catalogue);
        foreach (var achievement in achievements)
        {
            catalogue[achievement.Id] = achievement;
        }
        Inputs = Inputs with { Catalogue = catalogue };
        // A name arriving can turn a goal from unrepeatable to ordinary or the
        // reverse, which is a classification and not a measurement — so this
        // is a re-plan.
        await Replan();
    }

    /// <summary>
    /// Sync mounts, pets, toys and decor: what exists, and what the account
    /// has. The owned lists are account-wide, so one call each. The index
    /// comes back with names and no sources, so sources fill in a batch at a
    /// time the same way achievement names do.
    /// </summary>
    public async Task SyncCollections(Token token, long generation)
    {
        var region = Settings.Region;
        await Task.WhenAll(Links.AllKinds.Select(kind => SyncCollection(token, generation, region, kind)));
    }

    private async Task SyncCollection(Token token, long generation, Region region, Kind kind)
    {
        var ownedTask = FetchBare(token, generation, CollectionsApi.Collected(region, kind));
        var indexTask = FetchBare(token, generation, CollectionsApi.Index(region, kind));
        var ownedBody = await ownedTask;
        if (ownedBody is not null && CollectionsApi.ParseCollected(ownedBody, kind) is Outcome<HashSet<long>>.Found owned)
        {
            await Store.On(store => store.SaveOwned(kind, owned.Value));
            // The baseline's idea of what is already spent. A goal for a
            // mount the account owns is excluded, and this is where that set
            // comes from.
            Inputs = Inputs with { Owned = [.. Inputs.Owned, .. owned.Value] };
            Changed?.Invoke();
        }
        var indexBody = await indexTask;
        if (indexBody is null || CollectionsApi.ParseIndex(indexBody, kind) is not Outcome<List<Collectible>>.Found catalogue)
        {
            return;
        }
        var wanted = await Store.On(store =>
        {
            store.SaveCollectibles(catalogue.Value);
            // Sources, for the entries that do not have one yet.
            return store.CollectiblesHeld(kind).Match(
                held => held.Catalogue.Where(entry => entry.Source == Source.Unknown && entry.LinkId == entry.Id).Select(entry => entry.Id).Take(DetailsPerSync).ToList(),
                _ => []);
        });
        Changed?.Invoke();
        var details = await Task.WhenAll(wanted.Select(async id =>
        {
            var body = await FetchBare(token, generation, CollectionsApi.Detail(region, kind, id));
            return body is not null && CollectionsApi.ParseDetail(body, kind) is Outcome<Collectible>.Found entry ? entry.Value : null;
        }));
        var found = details.Where(entry => entry is not null).Select(entry => entry!).ToList();
        if (found.Count > 0 && Generation == generation)
        {
            await Store.On(store => store.SaveCollectibles(found));
            Changed?.Invoke();
        }
    }

    /// <summary>
    /// Fetch the render URLs that cannot be worked out locally: a portrait
    /// per enrolled character, a toy's or decor's icon, an achievement's icon.
    /// Capped per sync; the ones on screen come first, and the rest arrive
    /// over following syncs or all at once from the menu.
    /// </summary>
    public async Task SyncMedia(Token token, long generation, int budget)
    {
        var region = Settings.Region;
        var fetches = new List<Task>();
        foreach (var character in Members.Where(character => !Portraits.ContainsKey(character.Key)).ToList())
        {
            fetches.Add(FetchPortrait(token, generation, region, character.Key));
        }

        // Toys and decor. Both are items, and neither has a creature display
        // to be drawn from for nothing. The page is asked rather than the
        // store, because the page knows what is actually in front of somebody.
        foreach (var kind in new[] { Kind.Toy, Kind.Decor })
        {
            foreach (var id in ItemArtWanted?.Invoke(kind, budget) ?? [])
            {
                fetches.Add(FetchArt(token, generation, Media.Item(region, id), ToyArt, id));
            }
        }
        foreach (var id in AchievementArtWanted?.Invoke(budget) ?? [])
        {
            fetches.Add(FetchArt(token, generation, Media.Achievement(region, id), AchievementArt, id));
        }
        await Task.WhenAll(fetches);
        if (fetches.Count > 0 && Generation == generation)
        {
            Changed?.Invoke();
        }
    }

    private async Task FetchPortrait(Token token, long generation, Region region, CharacterKey key)
    {
        var body = await FetchBare(token, generation, Media.Character(region, key));
        if (body is not null && Media.ParsePortrait(body, Portrait.Avatar) is Outcome<string>.Found url && Generation == generation)
        {
            Portraits[key] = url.Value;
        }
    }

    private async Task FetchArt(Token token, long generation, Request request, Dictionary<long, string> art, long id)
    {
        var body = await FetchBare(token, generation, request);
        if (body is not null && Media.ParseIcon(body) is Outcome<string>.Found url && Generation == generation)
        {
            art[id] = url.Value;
        }
    }

    /// <summary>
    /// Fetch every missing picture, rather than a sync's worth. Thousands of
    /// requests, small and cached forever after, but it is the person's quota
    /// and spending it is their call.
    /// </summary>
    public async Task FetchAllArt()
    {
        if (Token is not { } token)
        {
            Toasted?.Invoke("Sign in to fetch artwork — the render URLs come from Blizzard.");
            return;
        }
        Toasted?.Invoke("Fetching artwork. It is cached once it lands, so this happens once.");
        await SyncMedia(token, Generation, int.MaxValue);
    }

    /// <summary>
    /// Turn a name somebody typed in the market browser into item ids, and
    /// keep them. The names are written and the bindings are not: a search
    /// result carries no binding, so the item stays unknown to the spoils
    /// until the full record is fetched.
    /// </summary>
    public async Task LookUpItem(string name)
    {
        if (Token is not { } token)
        {
            Toasted?.Invoke("Sign in to search by name — the catalogue is Blizzard's.");
            return;
        }
        var region = Settings.Region;
        var body = await FetchBare(token, Generation, GameData.ItemSearch(region, name));
        if (body is null || GameData.ParseItemSearch(body, region.DefaultLocale()) is not Outcome<List<(long Id, string Name)>>.Found found || found.Value.Count == 0)
        {
            return;
        }
        await Store.On(store =>
        {
            foreach (var (id, itemName) in found.Value)
            {
                store.NameFoundItem(id, itemName);
            }
        });
        Changed?.Invoke();
    }

    /// <summary>Put names to the ids in the market snapshot, a budget at a time. One call per item; the one call also answers whether it can be sold.</summary>
    public async Task SyncItemNames(Token token, long generation, int budget)
    {
        var region = Settings.Region;
        var wanted = NamesWanted?.Invoke(budget) ?? [];
        if (wanted.Count == 0)
        {
            return;
        }
        var landed = await Task.WhenAll(wanted.Select(async id =>
        {
            var body = await FetchBare(token, generation, GameData.ItemOf(region, id));
            return body is not null && GameData.ParseItem(body) is Outcome<Item>.Found item ? (id, item.Value) : ((long, Item)?)null;
        }));
        var named = landed.Where(pair => pair is not null).Select(pair => pair!.Value).ToList();
        if (named.Count == 0 || Generation != generation)
        {
            return;
        }
        await Store.On(store =>
        {
            foreach (var (id, item) in named)
            {
                store.NameItem(id, item);
            }
        });
        Changed?.Invoke();
    }

    /// <summary>Fill in the Adventure Guide, a few at a time. Instances first: an encounter id is only known because an instance listed it.</summary>
    public async Task SyncGuide(Token token, long generation, int budget)
    {
        var region = Settings.Region;
        var body = await FetchBare(token, generation, GameData.InstanceIndex(region));
        if (body is null || GameData.ParseInstanceIndex(body) is not Outcome<List<(long Id, string Name)>>.Found known)
        {
            return;
        }
        var gaps = await Store.On(store => store.GuideGaps(known.Value).Match(gaps => gaps, _ => ([], [])));
        await FillGuide(token, generation, region, gaps.Instances, gaps.Encounters, budget);
    }

    private async Task FillGuide(Token token, long generation, Region region, List<long> instances, List<long> encounters, int budget)
    {
        var left = budget;
        var fetches = new List<Task>();
        foreach (var id in instances.Take(left))
        {
            left--;
            fetches.Add(FillInstance(token, generation, region, id));
        }
        foreach (var id in encounters.Take(Math.Max(left, 0)))
        {
            fetches.Add(FillEncounter(token, generation, region, id));
        }
        await Task.WhenAll(fetches);
        if (fetches.Count > 0 && Generation == generation)
        {
            Changed?.Invoke();
        }
    }

    private async Task FillInstance(Token token, long generation, Region region, long id)
    {
        var body = await FetchBare(token, generation, GameData.InstanceOf(region, id));
        if (body is not null && GameData.ParseInstance(body) is Outcome<Armory.Zones.Instance>.Found instance)
        {
            await Store.On(store => store.SaveInstance(instance.Value));
        }
    }

    private async Task FillEncounter(Token token, long generation, Region region, long id)
    {
        var body = await FetchBare(token, generation, GameData.EncounterOf(region, id));
        if (body is not null && GameData.ParseEncounter(body) is Outcome<Armory.Zones.Encounter>.Found encounter)
        {
            await Store.On(store => store.SaveEncounter(encounter.Value));
        }
    }
}
