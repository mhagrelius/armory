using System.Globalization;
using Armory.Addon;
using Armory.Blizzard;
using Armory.Chronicle;
using Armory.Client.Sharing;
using Armory.Collections;
using Armory.Roster;
using Armory.Run;
using Armory.Sharing;
using Armory.Store;

namespace Armory.Client.Shell;

/// <summary>What one pass did, as the sync page reports it.</summary>
public sealed record PassOutcome
{
    public DateTimeOffset At { get; init; }
    public int Sent { get; init; }
    public int Landed { get; init; }
    public int Removed { get; init; }

    /// <summary>Rows the other end could not read. A number climbing here is one machine running an older build.</summary>
    public int Unreadable { get; init; }

    /// <summary>Null when the pass finished.</summary>
    public string? Failed { get; init; }
}

/// <summary>Everything the Account &amp; Sharing dialog draws.</summary>
public sealed record SyncState
{
    public string Server { get; init; } = "";
    public bool TokenHeld { get; init; }
    public string Machine { get; init; } = "";
    public bool Passing { get; init; }
    public List<(string Scope, int Count)> Queued { get; init; } = [];
    public string? QueuedSince { get; init; }
    public PassOutcome? Last { get; init; }
    public int Failures { get; init; }
    public string Account { get; init; } = "";
    public List<(string Name, long Rows)>? Held { get; init; }
    public List<string> GameAccounts { get; init; } = [];
    public string GameAccount { get; init; } = "";
}

/// <summary>What the dialog hands back when the person presses Save.</summary>
public sealed record Chosen(string Address, string? Token, string Account, string? GameAccount);

/// <summary>
/// The account, as the shell holds it: the port of the GTK application object.
/// </summary>
/// <remarks>
/// <para>Widgets report what a person did; this is the only object that
/// mutates state or asks a source anything. It is single-threaded by
/// contract: every public method is called from the shell's one thread and
/// every continuation returns to it through <c>dispatch</c>. The store lives
/// on its own thread behind <see cref="StoreWorker"/>, and the network runs
/// on the pool and is handed a parcel and gives back an answer.</para>
/// <para>Nothing here knows what a window is. The pages subscribe to
/// <see cref="Changed"/> and read the properties; a toast or a notice is an
/// event with a sentence in it.</para>
/// </remarks>
public sealed partial class Account : IDisposable
{
    /// <summary>How often a pass runs even when nothing has said to. A backstop, not the mechanism.</summary>
    public static readonly TimeSpan PassEvery = TimeSpan.FromSeconds(300);

    /// <summary>How long to wait after a write before pushing, restarted on each one, so a burst is one push.</summary>
    public static readonly TimeSpan PassAfterWrite = TimeSpan.FromSeconds(3);

    /// <summary>How many rows go in one batch. The server clamps to its own ceiling.</summary>
    public const int PassBatch = 2_000;

    /// <summary>How many passes must fail in a row before saying so.</summary>
    public const int FailuresBeforeSayingSo = 3;

    private readonly ISecrets secrets;
    private readonly Action<Func<Task>> dispatch;
    private readonly Func<string, string, string, string, Result<IRemote, SyncError>> connect;
    private readonly TimeProvider clock;
    private readonly string settingsPath;
    private CancellationTokenSource? passDue;
    private CancellationTokenSource? backstop;
    private AddonWatch? watch;
    private bool passing;
    private bool parked;

    public Account(
        StoreWorker store,
        ISecrets secrets,
        string settingsPath,
        Action<Func<Task>> dispatch,
        Func<string, string, string, string, Result<IRemote, SyncError>>? connect = null,
        TimeProvider? clock = null,
        HttpMessageHandler? handler = null,
        Func<Command, Task<Result<byte[], Reason>>>? run = null)
    {
        this.handler = handler;
        Store = store;
        this.secrets = secrets;
        this.settingsPath = settingsPath;
        this.dispatch = dispatch;
        this.connect = connect ?? ((url, token, machine, account) =>
            HttpRemote.Create(url, token, machine, account).Map(remote => (IRemote)remote));
        this.clock = clock ?? TimeProvider.System;
        this.run = run ?? (command => ClaudeCode.Run(command, Paths.JournalDir, JournalTimeout));
        Settings = Armory.Settings.Settings.Load(settingsPath);
    }

    // -- what the pages read ----------------------------------------------------

    public StoreWorker Store { get; }

    public Armory.Settings.Settings Settings { get; private set; }

    public Armory.Roster.Roster Roster { get; private set; } = new();

    public Cohort Cohort { get; private set; } = new();

    /// <summary>The expensive half, for every character the store knows.</summary>
    public Dictionary<CharacterKey, Detail> Details { get; private set; } = [];

    /// <summary>The current run, and its row id.</summary>
    public (long Id, Run.Run Run)? Run { get; private set; }

    /// <summary>Everything the planner needs, accumulated across a sync.</summary>
    public Inputs Inputs { get; private set; } = new();

    /// <summary>When the addon last wrote, as it saw the clock.</summary>
    public DateTimeOffset? CollectedAt { get; private set; }

    /// <summary>The <c>WTF/Account/NAME</c> folders this install has.</summary>
    public List<string> GameAccounts { get; private set; } = [];

    public PassOutcome? LastPass { get; private set; }

    public int Failures { get; private set; }

    public bool Passing => passing;

    /// <summary>What the server last said it was holding. Null until asked.</summary>
    public List<(string Name, long Rows)>? Held { get; private set; }

    /// <summary>Something a page shows changed. The pages redraw from the properties.</summary>
    public event Action? Changed;

    /// <summary>A sentence for the person, briefly.</summary>
    public event Action<string>? Toasted;

    /// <summary>A standing condition, or null when it has cleared.</summary>
    public event Action<string?>? Noticed;

    /// <summary>The sharing dialog's numbers moved.</summary>
    public event Action? SyncStateChanged;

    // -- launch -----------------------------------------------------------------

    /// <summary>What the GTK build does at startup and after every pass that landed something: read the account back.</summary>
    public async Task Restore()
    {
        var read = await Store.On(store =>
        {
            var roster = store.RosterHeld().Match(r => r, _ => new Armory.Roster.Roster());
            var cohort = store.CohortHeld().Match(c => c, _ => new Cohort());
            cohort.Prune(roster);
            return (
                roster,
                cohort,
                details: store.Details().Match(d => d, _ => []),
                attributions: store.Attributions().Match(a => a, _ => []),
                catalogue: store.AchievementsHeld().Match(a => a, _ => []),
                criteria: store.CriteriaKinds().Match(c => c, _ => []),
                provenance: store.ProvenanceHeld().Match(p => p, _ => []),
                run: store.CurrentRun().Match(r => r, _ => null));
        });
        Roster = read.roster;
        Cohort = read.cohort;
        Details = read.details;
        Inputs = Inputs with
        {
            Attributions = read.attributions,
            Catalogue = read.catalogue,
            Criteria = read.criteria,
            Provenance = read.provenance,
        };
        Run = read.run;
        await RestoreReputations();
        await RestoreArt();
        RestoreToken();
        StartWatching();
        Changed?.Invoke();
        // Whether a journal server is answering, once. Not awaited: a machine
        // with no llama-server should not hold the window on a timeout.
        _ = IdentifyJournal();
    }

    /// <summary>Whether onboarding has to be shown: no client, and no decision to go without one.</summary>
    public bool NeedsOnboarding => !Settings.IsRegistered && !Settings.AddonOnly;

    /// <summary>Whether a secret is in the vault. The field cannot be pre-filled, so the page says whether one is held instead.</summary>
    public bool SecretHeld(string name) => !string.IsNullOrWhiteSpace(secrets.Get(name));

    /// <summary>Put a secret in the vault, or leave the held one alone when the field was blank.</summary>
    public void RememberSecret(string name, string? value)
    {
        if (!string.IsNullOrWhiteSpace(value))
        {
            secrets.Set(name, value.Trim());
        }
    }

    public void ForgetSecret(string name) => secrets.Clear(name);

    public void SaveSettings(Armory.Settings.Settings settings)
    {
        Settings = settings;
        var saved = settings.Save(settingsPath);
        if (!saved.IsOk)
        {
            Toasted?.Invoke($"Could not write settings: {saved.Error}");
        }
    }

    // -- the addon --------------------------------------------------------------

    /// <summary>Find the install, choose the account folder, and watch the file.</summary>
    public void StartWatching()
    {
        var wow = Settings.WowPath ?? Armory.Settings.Settings.FindWow(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile));
        if (wow is null)
        {
            return;
        }
        if (Settings.WowPath is null)
        {
            SaveSettings(Settings with { WowPath = wow });
        }
        RefreshChances();

        // Which Battle.net login's folder to read. One is the normal case; a
        // second holds a different account entirely, so the choice is
        // remembered rather than guessed again every launch.
        GameAccounts = Files.Accounts(wow);
        string account;
        if (Settings.WowAccount is { } named && GameAccounts.Contains(named))
        {
            account = named;
        }
        else
        {
            if (GameAccounts.Count == 0)
            {
                return;
            }
            account = GameAccounts[0];
            SaveSettings(Settings with { WowAccount = account });
        }

        watch?.Dispose();
        var opened = AddonWatch.Open(wow, account, result => dispatch(() => Collected(result)));
        if (opened.IsOk)
        {
            watch = opened.Value;
        }
        else
        {
            Toasted?.Invoke($"Could not watch the addon file: {opened.Error}");
        }
    }

    /// <summary>
    /// The addon wrote. Store what it said and re-plan. This is the whole
    /// application when there is no API client.
    /// </summary>
    public async Task Collected(Result<Dump, ReadError> result)
    {
        if (!result.IsOk)
        {
            // The folder holds every addon's file; "not ours" is not a fault.
            if (result.Error is not ReadError.NotCollectorData)
            {
                Toasted?.Invoke($"The collector addon's file: {result.Error}");
            }
            return;
        }
        var dump = result.Value;
        var collected = dump.Collected;
        CollectedAt = collected.WrittenAt;

        // The evenings first: the only thing here the addon is the sole record of.
        await KeepSessions(dump.Sessions);

        // The roster, with no web API involved. Merged rather than replacing:
        // a sync may have found characters this player has never logged in
        // on, and the addon knowing nothing about them is not evidence they
        // are gone. Details are absorbed, not assigned.
        if (dump.Characters.Count > 0)
        {
            var characters = Roster.Characters.ToList();
            var details = new Dictionary<CharacterKey, Detail>(Details);
            foreach (var read in dump.Characters)
            {
                characters.RemoveAll(existing => existing.Key == read.Character.Key);
                characters.Add(read.Character);
                details[read.Character.Key] = (details.GetValueOrDefault(read.Character.Key) ?? new Detail()).Absorb(read.Detail);
            }
            Roster = new Armory.Roster.Roster(characters);
            Details = details;
            var roster = Roster;
            await Store.On(store =>
            {
                store.SaveRoster(roster);
                foreach (var (key, detail) in details)
                {
                    store.SaveDetail(key, detail);
                }
            });
        }

        if (collected.Collectibles.Count > 0)
        {
            var described = collected.Collectibles.Select(entry => entry.Kind).ToHashSet();
            await Store.On(store =>
            {
                store.SaveCollectibles(collected.Collectibles);
                // Only the kinds this file described: `SaveOwned` is wholesale,
                // and an empty set for a kind the addon did not scan would
                // un-own everything the web API had said was owned.
                foreach (var kind in described)
                {
                    store.SaveOwned(kind, collected.Owned.Where(owned => owned.Kind == kind).Select(owned => owned.Id).ToHashSet());
                }
            });
        }

        var catalogue = new Dictionary<long, Achievement>(Inputs.Catalogue);
        foreach (var (id, achievement) in collected.Catalogue)
        {
            catalogue[id] = achievement;
        }
        var primary = new Dictionary<CharacterKey, PrimaryData>(Inputs.Primary);
        foreach (var read in dump.Characters)
        {
            primary[read.Character.Key] = (primary.GetValueOrDefault(read.Character.Key) ?? new PrimaryData()) with { Quests = [.. read.Quests] };
        }
        Inputs = Inputs with
        {
            Attributions = new Dictionary<long, CharacterKey>(collected.EarnedBy),
            Criteria = new Dictionary<long, CriterionKind>(collected.Criteria),
            Catalogue = catalogue,
            // Only when the API has not already supplied a richer list: its
            // trees are nested where the addon's are one level deep.
            Progress = Inputs.Progress.Count == 0 ? collected.Progress() : Inputs.Progress,
            Primary = primary,
        };

        var merged = await Store.On(store =>
        {
            if (collected.Catalogue.Count > 0)
            {
                store.SaveAchievements(collected.Catalogue.Values);
            }
            store.SaveCollected(collected);
            // Read back rather than taken from the dump: the store merges by
            // taking the larger count, so a reinstalled addon that started
            // from zero does not erase a year of a character's work.
            return collected.Earned.Count > 0 ? store.ProvenanceHeld().Match<Armory.Provenance.Earnings?>(p => p, _ => null) : null;
        });
        if (merged is not null)
        {
            Inputs = Inputs with { Provenance = merged };
        }

        // Attribution decides poisoning, so a fresh set changes the standing
        // of every already-earned goal. That is a re-plan, not a re-measure.
        await Replan();
    }

    // -- the roster -------------------------------------------------------------

    /// <summary>Enrol a character, or withdraw them.</summary>
    public async Task ToggleEnrolment(CharacterKey key)
    {
        var cohort = new Cohort(Cohort.Keys);
        cohort.Toggle(key);
        Cohort = cohort;
        await Store.On(store => store.SaveCohort(cohort));
        Changed?.Invoke();
        ShareSoon();
    }

    // -- the run ----------------------------------------------------------------

    /// <summary>Start a run: freeze what the account has now, and plan from it.</summary>
    public async Task StartRun()
    {
        if (Cohort.IsEmpty)
        {
            Toasted?.Invoke("Enrol at least one character on Roster first.");
            return;
        }
        if (Inputs.Progress.Count == 0)
        {
            Toasted?.Invoke("Sync first — a baseline needs to know what the account has.");
            return;
        }
        var baseline = Planner.TakeBaseline(Inputs.Progress, Inputs.Owned, clock.GetUtcNow());
        var run = new Run.Run { Name = "Fresh start", Baseline = baseline, Cohort = Cohort, Goals = Planner.Plan(baseline, Cohort, Inputs) };
        var saved = await Store.On(store => store.SaveRun(null, run));
        if (!saved.IsOk)
        {
            Toasted?.Invoke($"Could not save the run: {saved.Error}");
            return;
        }
        Run = (saved.Value, run);
        Toasted?.Invoke("Run started. Everything is measured from now.");
        Changed?.Invoke();
        ShareSoon();
    }

    /// <summary>Throw the current run away and start another from now.</summary>
    public async Task StartOver()
    {
        if (Run is { } held)
        {
            await Store.On(store => store.ForgetRun(held.Id));
            Run = null;
        }
        await StartRun();
    }

    /// <summary>
    /// Rebuild the current run's goals from scratch. Used when attribution
    /// changes, because that changes standing, which remeasuring will not
    /// touch. Decisions the person made are carried across.
    /// </summary>
    public async Task Replan()
    {
        if (Run is not { } held)
        {
            Changed?.Invoke();
            return;
        }
        var existing = held.Run;
        var attestations = existing.Goals.Where(goal => goal.Attestation is not null).ToDictionary(goal => goal.AchievementId, goal => goal.Attestation!);
        Inputs = Inputs with
        {
            ExcludedByHand = existing.Goals.Where(goal => goal.Bucket is Bucket.Excluded { Why: Exclusion.ByHand }).Select(goal => goal.AchievementId).ToHashSet(),
        };
        var goals = Planner.Plan(existing.Baseline, existing.Cohort, Inputs)
            .Select(goal => attestations.TryGetValue(goal.AchievementId, out var attestation) ? goal with { Attestation = attestation } : goal)
            .ToList();
        await SaveRun(held.Id, existing with { Goals = goals });
    }

    /// <summary>Re-measure the observable goals after fresh primary data lands.</summary>
    public async Task Remeasure()
    {
        if (Run is not { } held)
        {
            return;
        }
        await SaveRun(held.Id, Planner.Remeasure(held.Run, Inputs));
    }

    /// <summary>Mark a goal done by hand, or take the mark back.</summary>
    public async Task Attest(long achievementId, CharacterKey? character)
    {
        if (Run is not { } held)
        {
            return;
        }
        var goals = held.Run.Goals.Select(goal => goal.AchievementId == achievementId
            ? goal with { Attestation = character is null ? null : new Attestation { Character = character, At = clock.GetUtcNow() } }
            : goal).ToList();
        await SaveRun(held.Id, held.Run with { Goals = goals });
    }

    /// <summary>Take a goal out of the run, or put it back where the classifier would have placed it.</summary>
    public async Task SetExcluded(long achievementId, bool excluded)
    {
        if (Run is not { } held)
        {
            return;
        }
        var goals = held.Run.Goals.Select(goal =>
        {
            if (goal.AchievementId != achievementId)
            {
                return goal;
            }
            if (excluded)
            {
                return goal with { Bucket = new Bucket.Excluded(Exclusion.ByHand) };
            }
            var progress = Inputs.Progress.FirstOrDefault(progress => progress.Id == achievementId);
            if (progress is null)
            {
                return goal;
            }
            var without = new Inputs { Catalogue = Inputs.Catalogue, Criteria = Inputs.Criteria, Owned = Inputs.Owned };
            return goal with { Bucket = Planner.Classify(progress, without) };
        }).ToList();
        await SaveRun(held.Id, held.Run with { Goals = goals });
    }

    private async Task SaveRun(long id, Run.Run run)
    {
        await Store.On(store => store.SaveRun(id, run));
        Run = (id, run);
        Changed?.Invoke();
        ShareSoon();
    }

    // -- sharing this account with the other machines ------------------------------

    /// <summary>This installation's name in the change log: made once and kept in the database beside the cursor, never in settings.</summary>
    public async Task<string> Machine()
    {
        var held = await Store.On(store => store.Machine());
        if (held.Length > 0)
        {
            return held;
        }
        var made = Guid.NewGuid().ToString();
        await Store.On(store => store.SetMachine(made));
        return made;
    }

    /// <summary>The server, if this machine has been given one. Both or neither: an address with no token cannot authenticate.</summary>
    public async Task<IRemote?> SyncTarget()
    {
        var url = Settings.SyncUrl.Trim();
        if (url.Length == 0)
        {
            return null;
        }
        var token = secrets.Get(SecretNames.SyncToken);
        if (string.IsNullOrWhiteSpace(token))
        {
            return null;
        }
        var made = connect(url, token, await Machine(), Settings.SyncAccount.Trim());
        return made.IsOk ? made.Value : null;
    }

    /// <summary>Begin sharing, if there is anywhere to share to: one pass now, and the backstop timer.</summary>
    public async Task StartSharing()
    {
        if (await SyncTarget() is null)
        {
            return;
        }
        await ShareNow();
        backstop?.Cancel();
        backstop = new CancellationTokenSource();
        var token = backstop.Token;
        _ = Task.Run(async () =>
        {
            while (!token.IsCancellationRequested)
            {
                try
                {
                    await Task.Delay(PassEvery, token);
                }
                catch (OperationCanceledException)
                {
                    return;
                }
                dispatch(ShareNow);
            }
        }, token);
    }

    /// <summary>Push and pull after the next quiet moment. Called from everything that writes; restarting the clock is the point.</summary>
    public void ShareSoon()
    {
        if (Settings.SyncUrl.Trim().Length == 0 || string.IsNullOrWhiteSpace(secrets.Get(SecretNames.SyncToken)))
        {
            return;
        }
        passDue?.Cancel();
        passDue = new CancellationTokenSource();
        var token = passDue.Token;
        _ = Task.Run(async () =>
        {
            try
            {
                await Task.Delay(PassAfterWrite, token);
            }
            catch (OperationCanceledException)
            {
                return;
            }
            dispatch(ShareNow);
        }, token);
    }

    /// <summary>One pass: everything waiting up, everything new down.</summary>
    public async Task ShareNow()
    {
        if (passing)
        {
            return;
        }
        var remote = await SyncTarget();
        if (remote is null)
        {
            return;
        }
        // Everything this machine held before sharing was set up, offered up once.
        await Store.On(store => store.SeedLog());
        passing = true;
        SyncStateChanged?.Invoke();
        var outcome = await Pass(remote);
        await FinishPass(outcome);
        Park(remote);
    }

    /// <summary>
    /// The pass itself: the core's three steps with an await between them.
    /// The store's thread does every read and write; the network goes to the
    /// pool. Anything that has to be decided is in the core.
    /// </summary>
    private async Task<Result<PassOutcome, SyncError>> Pass(IRemote remote)
    {
        var report = new Report();
        while (true)
        {
            var next = await Store.On(store => store.NextStep(PassBatch));
            if (!next.IsOk)
            {
                return Result<PassOutcome, SyncError>.Err(new SyncError(next.Error.Message));
            }
            switch (next.Value)
            {
                case Step.Push push:
                    {
                        var applied = await Task.Run(() => remote.Push(push.Parcel));
                        if (!applied.IsOk)
                        {
                            return Result<PassOutcome, SyncError>.Err(applied.Error);
                        }
                        var absorbed = await Store.On(store => store.AbsorbPush(push.Through, push.Parcel.Rows.Count, applied.Value, report));
                        if (!absorbed.IsOk)
                        {
                            return Result<PassOutcome, SyncError>.Err(new SyncError(absorbed.Error.Message));
                        }
                        break;
                    }
                case Step.Drain drain:
                    await Store.On(store => store.Drain(drain.Through));
                    break;
                case Step.Pull pull:
                    {
                        var pulled = await Task.Run(() => remote.Pull(pull.Since, PassBatch));
                        if (!pulled.IsOk)
                        {
                            return Result<PassOutcome, SyncError>.Err(pulled.Error);
                        }
                        var more = await Store.On(store => store.AbsorbPull(pulled.Value, report));
                        if (!more.IsOk)
                        {
                            return Result<PassOutcome, SyncError>.Err(new SyncError(more.Error.Message));
                        }
                        if (!more.Value)
                        {
                            return Result<PassOutcome, SyncError>.Ok(new PassOutcome
                            {
                                At = clock.GetUtcNow(),
                                Sent = report.Sent,
                                Landed = report.Landed,
                                Removed = report.Removed,
                                Unreadable = report.Unreadable,
                            });
                        }
                        break;
                    }
            }
        }
    }

    private async Task FinishPass(Result<PassOutcome, SyncError> outcome)
    {
        passing = false;
        if (outcome.IsOk)
        {
            if (Failures >= FailuresBeforeSayingSo)
            {
                Noticed?.Invoke(null);
            }
            Failures = 0;
            LastPass = outcome.Value;
            // Only when something actually arrived: redrawing ten pages after
            // a pass that agreed with the server is work nobody asked for.
            if (outcome.Value.Landed + outcome.Value.Removed > 0)
            {
                await Restore();
            }
        }
        else
        {
            Failures++;
            // A banner only once it has stopped being ordinary. A NAS asleep
            // and a suspended laptop each produce one failed pass.
            if (Failures >= FailuresBeforeSayingSo)
            {
                Noticed?.Invoke($"Not sharing — {Failures} passes in a row have failed. {outcome.Error}");
            }
            LastPass = new PassOutcome { At = clock.GetUtcNow(), Failed = outcome.Error.Message };
        }
        SyncStateChanged?.Invoke();
    }

    /// <summary>Park on the server until another machine writes something. Not while failing: a server that is not answering is asked on the timer.</summary>
    private void Park(IRemote remote)
    {
        if (parked || Failures > 0)
        {
            return;
        }
        parked = true;
        _ = Task.Run(async () =>
        {
            var since = await Store.On(store => store.Cursor(Armory.Store.Store.Pulled));
            var woken = remote.Wait(since);
            dispatch(() =>
            {
                parked = false;
                return woken is { IsOk: true, Value: true } ? ShareNow() : Task.CompletedTask;
            });
        });
    }

    /// <summary>The Account &amp; Sharing dialog's numbers.</summary>
    public async Task<SyncState> SyncStateNow()
    {
        var now = clock.GetUtcNow();
        var (machine, queued, since) = await Store.On(store => (
            store.Machine(),
            store.Queued().Match(q => q, _ => []),
            store.QueuedSince().Match(s => s, _ => null)));
        return new SyncState
        {
            Server = Settings.SyncUrl.Trim(),
            TokenHeld = !string.IsNullOrWhiteSpace(secrets.Get(SecretNames.SyncToken)),
            Machine = machine,
            Passing = passing,
            Queued = queued,
            QueuedSince = since is null ? null : Armory.Store.Store.ParseStamp(since) is { } at ? When(at, now) : since,
            Last = LastPass,
            Failures = Failures,
            Account = Settings.SyncAccount.Trim(),
            Held = Held,
            GameAccounts = GameAccounts,
            GameAccount = Settings.WowAccount ?? "",
        };
    }

    /// <summary>Offer the whole account up again, after the server has been emptied.</summary>
    public async Task Resend()
    {
        await Store.On(store => store.ForgetServer());
        Failures = 0;
        await ShareNow();
    }

    /// <summary>Ask the server what it is holding.</summary>
    public async Task RefreshAccounts()
    {
        if (await SyncTarget() is not HttpRemote remote)
        {
            return;
        }
        var held = await Task.Run(() => remote.Accounts());
        if (held.IsOk)
        {
            Held = held.Value.Select(account => (account.Name, account.Rows)).ToList();
            SyncStateChanged?.Invoke();
        }
    }

    /// <summary>Delete an account off the server. The one thing here that destroys somebody else's copy.</summary>
    public async Task<Result<Unit, SyncError>> ForgetAccount(string name)
    {
        if (await SyncTarget() is not HttpRemote remote)
        {
            return Result<Unit, SyncError>.Err(new SyncError("not sharing"));
        }
        var gone = await Task.Run(() => remote.ForgetAccount(name));
        if (gone.IsOk)
        {
            await RefreshAccounts();
        }
        return gone;
    }

    /// <summary>Remember where to share to and which account to read, and start or stop sharing.</summary>
    public async Task SaveSyncTarget(Chosen chosen)
    {
        var moved = chosen.GameAccount is { } named && Settings.WowAccount != named;
        SaveSettings(Settings with
        {
            SyncUrl = chosen.Address.Trim(),
            SyncAccount = chosen.Account.Trim(),
            WowAccount = chosen.GameAccount ?? Settings.WowAccount,
        });
        if (moved)
        {
            StartWatching();
        }
        if (chosen.Token is { } token)
        {
            secrets.Set(SecretNames.SyncToken, token);
        }
        // An emptied address is the deliberate way to stop sharing, and it
        // takes the token with it.
        if (Settings.SyncUrl.Length == 0)
        {
            secrets.Clear(SecretNames.SyncToken);
            return;
        }
        Failures = 0;
        await ShareNow();
    }

    /// <summary>An instant, as somebody would say it. The shell's wording, not the core's.</summary>
    public static string When(DateTimeOffset at, DateTimeOffset now)
    {
        var minutes = (long)(now - at).TotalMinutes;
        return minutes switch
        {
            <= 0 => "just now",
            1 => "a minute ago",
            <= 59 => string.Create(CultureInfo.InvariantCulture, $"{minutes} minutes ago"),
            <= 119 => "an hour ago",
            <= 1439 => string.Create(CultureInfo.InvariantCulture, $"{minutes / 60} hours ago"),
            <= 2879 => "yesterday",
            _ => string.Create(CultureInfo.InvariantCulture, $"{minutes / 1440} days ago"),
        };
    }

    /// <summary>The thirty-day sweep, on the way out. A first paint should not wait on it.</summary>
    public async Task Shutdown()
    {
        await Store.On(store => store.Purge());
        PurgeArt();
    }

    public void Dispose()
    {
        // The redirect port is fixed and registered; leaking it makes the next
        // sign-in fail with something that reads like a Blizzard problem.
        redirect?.Dispose();
        blizzard?.Dispose();
        journalHttp?.Dispose();
        passDue?.Cancel();
        backstop?.Cancel();
        watch?.Dispose();
        Store.Dispose();
    }
}
