using Armory.Addon;
using Armory.Client.Shell;
using Armory.Roster;
using Armory.Run;
using Armory.Sharing;
using Armory.Tests.Support;
using Xunit;

namespace Armory.Tests.Client;

/// <summary>The orchestrator, driven the way the pages drive it, against real stores and an in-process server.</summary>
public sealed class AccountTests : IDisposable
{
    private readonly string directory = Directory.CreateTempSubdirectory("armory-account-").FullName;
    private readonly List<Account> accounts = [];
    private readonly Armory.Store.Store server = Armory.Store.Store.InMemory();

    public AccountTests()
    {
        server.SetMachine("server");
    }

    private static readonly CharacterKey Somechar = new("emerald-dream", "Somechar");

    private Account Make(string machine, MemorySecrets? secrets = null, string? syncUrl = null)
    {
        secrets ??= new MemorySecrets();
        var store = Armory.Store.Store.InMemory();
        store.SetMachine(machine);
        var settings = Path.Combine(directory, machine, "settings.json");
        if (syncUrl is not null)
        {
            new Armory.Settings.Settings { SyncUrl = syncUrl, SyncAccount = "test" }.Save(settings);
            secrets.Set(SecretNames.SyncToken, "token");
        }
        // Dispatch runs inline: the tests are the one thread.
        var account = new Account(
            new StoreWorker(store),
            secrets,
            settings,
            work => work().GetAwaiter().GetResult(),
            (_, _, who, _) => Result<IRemote, SyncError>.Ok(new InProcessRemote(server, who)));
        accounts.Add(account);
        return account;
    }

    private static Collected ACollectedFile()
    {
        var collected = new Collected();
        collected.EarnedBy[4956] = new CharacterKey("mannoroth", "Aeltor");
        collected.Completed[4956] = DateTimeOffset.FromUnixTimeSeconds(1_457_000_000);
        collected.Tree[4956] = [12345, 12346];
        collected.Criteria[12345] = CriterionKind.Quest(5000);
        collected.Criteria[12346] = CriterionKind.Quest(5001);
        collected.Catalogue[4956] = new Armory.Blizzard.Achievement { Id = 4956, Name = "Loremaster of Kalimdor", Category = "Quests", Points = 50 };
        return collected;
    }

    private static CollectedCharacter ACharacterFile(params long[] quests) => new()
    {
        Character = new Character { Key = Somechar, DisplayName = "Somechar", RealmName = "Emerald Dream", Level = 80, Class = "Druid", Race = "Tauren", Faction = Faction.Horde },
        Detail = new Detail { ItemLevel = 640, Spec = "Restoration" },
        Quests = [.. quests],
    };

    [Fact(DisplayName = "an addon read builds the roster, the inputs and the store with no API at all")]
    public async Task An_addon_read_builds_the_roster_the_inputs_and_the_store()
    {
        var account = Make("one");
        var changes = 0;
        account.Changed += () => changes++;

        await account.Collected(Result<Dump, ReadError>.Ok(new Dump { Collected = ACollectedFile(), Characters = [ACharacterFile(5000)] }));

        Assert.Equal("Somechar", Assert.Single(account.Roster.Characters).DisplayName);
        Assert.Equal(640, account.Details[Somechar].ItemLevel);
        Assert.Equal(new CharacterKey("mannoroth", "Aeltor"), account.Inputs.Attributions[4956]);
        Assert.Single(account.Inputs.Progress);
        Assert.Contains(5000L, account.Inputs.Primary[Somechar].Quests);
        Assert.True(changes > 0);
        // And it is all in the store, so a relaunch reads it back.
        var again = Make("one-again");
        Assert.Empty(again.Roster.Characters);
        Assert.Equal(1, await account.Store.On(store => store.RosterHeld().Value.Count));
    }

    [Fact(DisplayName = "a second read absorbs details rather than assigning them")]
    public async Task A_second_read_absorbs_details_rather_than_assigning_them()
    {
        var account = Make("one");
        await account.Collected(Result<Dump, ReadError>.Ok(new Dump { Collected = new Collected(), Characters = [ACharacterFile()] }));
        var withMoney = ACharacterFile() with { Detail = new Detail { Money = 1_234 } };
        await account.Collected(Result<Dump, ReadError>.Ok(new Dump { Collected = new Collected(), Characters = [withMoney] }));
        Assert.Equal(640, account.Details[Somechar].ItemLevel);
        Assert.Equal(1_234, account.Details[Somechar].Money);
    }

    [Fact(DisplayName = "a run cannot start with nobody enrolled or nothing synced, and starts once both are true")]
    public async Task A_run_starts_once_somebody_is_enrolled_and_something_is_known()
    {
        var account = Make("one");
        var toasts = new List<string>();
        account.Toasted += toasts.Add;

        await account.StartRun();
        Assert.Contains(toasts, toast => toast.Contains("Enrol", StringComparison.Ordinal));
        Assert.Null(account.Run);

        await account.Collected(Result<Dump, ReadError>.Ok(new Dump { Collected = ACollectedFile(), Characters = [ACharacterFile(5000)] }));
        await account.ToggleEnrolment(Somechar);
        await account.StartRun();

        var (_, run) = account.Run!.Value;
        Assert.Equal("Fresh start", run.Name);
        var goal = Assert.Single(run.Goals);
        // Aeltor earned it in 2016 and is not enrolled, so it is poisoned and
        // measured from Somechar's own quests: one of two.
        Assert.True(goal.Standing.IsPoisoned);
        Assert.Equal(1, goal.Evaluation!.Value.Progress);
        Assert.Equal(2, goal.Evaluation.Value.Required);
    }

    [Fact(DisplayName = "attesting and excluding change a goal and survive a replan")]
    public async Task Attesting_and_excluding_change_a_goal_and_survive_a_replan()
    {
        var account = Make("one");
        // The catalogue names no criterion kinds, so the tree cannot be
        // measured and the goal lands in attestation: a person's word is the
        // only thing that can finish it.
        var unmeasurable = ACollectedFile();
        unmeasurable.Criteria.Clear();
        await account.Collected(Result<Dump, ReadError>.Ok(new Dump { Collected = unmeasurable, Characters = [ACharacterFile()] }));
        await account.ToggleEnrolment(Somechar);
        await account.StartRun();
        Assert.IsType<Bucket.Attestable>(account.Run!.Value.Run.Goals[0].Bucket);

        await account.SetExcluded(4956, true);
        Assert.Equal(new Bucket.Excluded(Exclusion.ByHand), account.Run!.Value.Run.Goals[0].Bucket);
        await account.Replan();
        Assert.Equal(new Bucket.Excluded(Exclusion.ByHand), account.Run!.Value.Run.Goals[0].Bucket);

        // Put back where the classifier would have placed it, not assumed observable.
        await account.SetExcluded(4956, false);
        Assert.IsType<Bucket.Attestable>(account.Run!.Value.Run.Goals[0].Bucket);
        Assert.False(account.Run!.Value.Run.Goals[0].IsDone);

        await account.Attest(4956, Somechar);
        Assert.True(account.Run!.Value.Run.Goals[0].IsDone);
        await account.Replan();
        Assert.True(account.Run!.Value.Run.Goals[0].Attestation is not null, "a person's word survives a replan");
        Assert.True(account.Run!.Value.Run.Goals[0].IsDone);

        // An observable goal is done by measurement, never by a word.
        var measured = Make("two");
        await measured.Collected(Result<Dump, ReadError>.Ok(new Dump { Collected = ACollectedFile(), Characters = [ACharacterFile()] }));
        await measured.ToggleEnrolment(Somechar);
        await measured.StartRun();
        Assert.IsType<Bucket.Observable>(measured.Run!.Value.Run.Goals[0].Bucket);
        await measured.Attest(4956, Somechar);
        Assert.False(measured.Run!.Value.Run.Goals[0].IsDone);
    }

    [Fact(DisplayName = "a pass shares an addon read with another machine through the server")]
    public async Task A_pass_shares_an_addon_read_with_another_machine()
    {
        var one = Make("one", syncUrl: "http://server");
        var two = Make("two", syncUrl: "http://server");
        await one.Collected(Result<Dump, ReadError>.Ok(new Dump { Collected = ACollectedFile(), Characters = [ACharacterFile(5000)] }));

        await one.ShareNow();
        Assert.NotNull(one.LastPass);
        Assert.Null(one.LastPass!.Failed);
        Assert.True(one.LastPass.Sent > 0);

        await two.ShareNow();
        Assert.True(two.LastPass!.Landed > 0);
        Assert.Equal("Somechar", Assert.Single(two.Roster.Characters).DisplayName);
        Assert.Equal(new CharacterKey("mannoroth", "Aeltor"), two.Inputs.Attributions[4956]);

        // A quiet pass is quiet.
        await one.ShareNow();
        Assert.Equal(0, one.LastPass!.Sent + one.LastPass.Landed);
    }

    [Fact(DisplayName = "failures are counted and only said out loud after three in a row")]
    public async Task Failures_are_counted_and_only_said_out_loud_after_three()
    {
        var secrets = new MemorySecrets();
        secrets.Set(SecretNames.SyncToken, "token");
        var settings = Path.Combine(directory, "failing", "settings.json");
        new Armory.Settings.Settings { SyncUrl = "http://nowhere" }.Save(settings);
        var store = Armory.Store.Store.InMemory();
        var account = new Account(new StoreWorker(store), secrets, settings, work => work().GetAwaiter().GetResult(),
            (_, _, _, _) => Result<IRemote, SyncError>.Ok(new Refusing()));
        accounts.Add(account);
        string? notice = null;
        account.Noticed += said => notice = said;

        await account.ShareNow();
        await account.ShareNow();
        Assert.Null(notice);
        await account.ShareNow();
        Assert.NotNull(notice);
        Assert.Contains("3 passes", notice, StringComparison.Ordinal);
        Assert.Equal(3, account.Failures);
    }

    [Fact(DisplayName = "the sharing dialog's state says what is held and what is waiting")]
    public async Task The_sharing_dialogs_state_says_what_is_held_and_what_is_waiting()
    {
        var account = Make("one", syncUrl: "http://server");
        await account.Collected(Result<Dump, ReadError>.Ok(new Dump { Collected = new Collected(), Characters = [ACharacterFile()] }));
        var state = await account.SyncStateNow();
        Assert.True(state.TokenHeld);
        Assert.Equal("http://server", state.Server);
        Assert.Contains(state.Queued, queued => queued.Scope == "character");
        Assert.Equal("just now", state.QueuedSince);
        Assert.Equal("test", state.Account);
    }

    [Fact(DisplayName = "an emptied address stops sharing and takes the token with it")]
    public async Task An_emptied_address_stops_sharing_and_takes_the_token_with_it()
    {
        var secrets = new MemorySecrets();
        var account = Make("one", secrets, "http://server");
        await account.SaveSyncTarget(new Chosen("", null, "", null));
        Assert.Null(secrets.Get(SecretNames.SyncToken));
        Assert.Null(await account.SyncTarget());
    }

    [Fact(DisplayName = "a relative time reads the way a person says it")]
    public void A_relative_time_reads_the_way_a_person_says_it()
    {
        var now = new DateTimeOffset(2026, 9, 14, 12, 0, 0, TimeSpan.Zero);
        Assert.Equal("just now", Account.When(now, now));
        Assert.Equal("a minute ago", Account.When(now.AddMinutes(-1), now));
        Assert.Equal("5 minutes ago", Account.When(now.AddMinutes(-5), now));
        Assert.Equal("an hour ago", Account.When(now.AddMinutes(-90), now));
        Assert.Equal("3 hours ago", Account.When(now.AddHours(-3), now));
        Assert.Equal("yesterday", Account.When(now.AddHours(-30), now));
        Assert.Equal("4 days ago", Account.When(now.AddDays(-4), now));
    }

    public void Dispose()
    {
        foreach (var account in accounts)
        {
            account.Dispose();
        }
        server.Dispose();
        Directory.Delete(directory, recursive: true);
    }

    /// <summary>A server that refuses everything: what a NAS asleep looks like.</summary>
    private sealed class Refusing : IRemote
    {
        public Result<Applied, SyncError> Push(Parcel parcel) => Result<Applied, SyncError>.Err(new SyncError("could not reach nowhere"));

        public Result<Pulled, SyncError> Pull(long since, int limit) => Result<Pulled, SyncError>.Err(new SyncError("could not reach nowhere"));

        public Result<bool, SyncError> Wait(long since) => Result<bool, SyncError>.Err(new SyncError("could not reach nowhere"));
    }
}
