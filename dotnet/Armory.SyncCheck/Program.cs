// Two machines against one server, driven by the real client code.
//
// A sync that has never been contradicted has not been tested. Everything in
// the replica is a pure function with unit tests, and the server has its own,
// but between them sit a wire format, a transport, and the question of
// whether the two ends agree about what a row is. One machine can never ask
// that question. So: two stores, one server, the real `HttpRemote`.
//
// Run it against a throwaway server, or against the NAS: it works in an
// account of its own (`sync-check`) and deletes that account afterwards
// unless SYNC_CHECK_KEEP is set. It never touches any other account.
//
//   SYNC_CHECK_URL    http://nas.example.ts.net:8084
//   SYNC_CHECK_TOKEN  the server's ARMORY_TOKEN
//   SYNC_CHECK_A      a directory for machine A's store
//   SYNC_CHECK_B      a directory for machine B's store
//
// The port of `examples/sync-check.rs`. The checks that need slices not yet
// ported (sessions, the roster, tallies, runs) are marked below and come
// across with those slices.

using Armory.Client.Sharing;
using Armory.Store;

string Need(string name) =>
    Environment.GetEnvironmentVariable(name) is { Length: > 0 } value
        ? value
        : throw new InvalidOperationException($"set {name}");

var url = Need("SYNC_CHECK_URL");
var token = Need("SYNC_CHECK_TOKEN");

var a = Machine.Open("machine-a", Need("SYNC_CHECK_A"), url, token);
var b = Machine.Open("machine-b", Need("SYNC_CHECK_B"), url, token);

var failures = 0;
void Check(string name, bool held)
{
    Console.WriteLine(held ? $"  ok   {name}" : $"  FAIL {name}");
    if (!held)
    {
        failures++;
    }
}

Console.WriteLine($"sync-check: {a.Remote.Address}, account {Machine.Account}");
var reachable = a.Remote.Reachable();
Check("the server answers /health", reachable.IsOk);
if (!reachable.IsOk)
{
    Console.WriteLine($"       {reachable.Error}");
    return 1;
}

// -- something recorded on one machine reaches the other --------------------

a.Store.WatchItem(4306, "Silk Cloth");
a.Store.WatchRealm(61, "Emerald Dream");
a.Store.SaveRoster(Machine.ARoster());
var evening = Machine.Evening("Somechar", 1);
a.Store.SaveSessions([evening]);
var sent = a.Pass();
Check("a machine with something to say sends it", sent.Sent > 0);

var got = b.Pass();
Check("and the other machine gets it", got.Landed > 0);
Check("a watch travels whole", b.Store.WatchedItems().Value.Count == 1);
Check("and so does a realm", b.Store.WatchedRealmList().Value.Count == 1);
Check("and so does the roster", b.Store.RosterHeld().Value.Count == 1);

Check("an evening travels whole", b.Store.SessionsHeld(10).Value.Count == 1);

// -- a quiet pass is quiet ------------------------------------------------

// The failure this catches: a pass that is never empty means something is
// re-uploading the account on a timer.
Check("a pass with nothing to do does nothing", a.Pass().IsEmpty);
Check("on both machines", b.Pass().IsEmpty);

// -- nothing bounces back --------------------------------------------------

Check("what arrived is not sent straight back", b.Store.Queued().Value.Count == 0);

// -- the same evening again is not a second evening ------------------------

b.Store.SaveSessions([evening]);
b.Pass();
a.Pass();
Check("an evening both machines saw is one evening", a.Store.SessionsHeld(10).Value.Count == 1);

// -- the same thing again is not news --------------------------------------

b.Store.WatchItem(4306, "Silk Cloth");
Check("a row both machines already agree on enqueues nothing", b.Store.Queued().Value.Count == 0);
b.Pass();
a.Pass();
Check("and both still hold one watch", a.Store.WatchedItems().Value.Count == 1);

// -- a counter never goes backwards ----------------------------------------

// The rule the lifetime counters exist under, across the wire this time.
a.Store.SaveCollected(Machine.Tallied(412));
a.Pass();
b.Pass();

b.Store.SaveCollected(Machine.Tallied(1));
b.Pass();
a.Pass();
b.Pass();

Check("a machine that was behind cannot take a counter back", Machine.Counted(a.Store) == 412);
Check("and both machines agree on the larger one", Machine.Counted(b.Store) == 412);

// -- a deletion travels, and only when it is meant --------------------------

a.Store.UnwatchItem(4306);
a.Pass();
b.Pass();
Check("taking a watch off travels", b.Store.WatchedItems().Value.Count == 0);

// -- a sweep is not a deletion ----------------------------------------------

a.Store.StoreResponse("https://example.test/kept", "body"u8.ToArray(), null);
a.Pass();
b.Pass();
Check("a cached body travels", b.Store.Response("https://example.test/kept", TimeSpan.FromDays(30)).Value is not null);
a.Store.Purge();
a.Pass();
b.Pass();
Check(
    "an expiry on one machine is not a deletion on the other",
    b.Store.Response("https://example.test/kept", TimeSpan.FromDays(30)).Value is not null);

// -- a run, its goals, and the ids that mean nothing elsewhere ---------------

a.Store.SaveRun(null, Machine.ARun());
a.Pass();
b.Pass();
var held = b.Store.CurrentRun().Value;
Check("a run travels", held is not null);
Check("and its goals come with it, hung off the right run", held?.Run.Goals.Count == 1);

// -- and at the end of it the two machines hold the same thing ---------------

a.Pass();
b.Pass();
a.Pass();
Check("the two machines end up holding the same account", Machine.Same(a.Store, b.Store));

// -- the server's own view --------------------------------------------------

var accounts = a.Remote.Accounts();
Check("the server lists the account this ran in", accounts.IsOk && accounts.Value.Any(held => held.Name == Machine.Account));

if (Environment.GetEnvironmentVariable("SYNC_CHECK_KEEP") is null)
{
    var forgotten = a.Remote.ForgetAccount(Machine.Account);
    Check("and forgets it when asked, by name", forgotten.IsOk);
}

Console.WriteLine();
if (failures == 0)
{
    Console.WriteLine("sync-check: all good.");
    return 0;
}
Console.WriteLine($"sync-check: {failures} failed.");
return 1;

/// <summary>What the application holds: a store and a way to the server.</summary>
internal sealed class Machine : IDisposable
{
    public const int Batch = 500;
    public const string Account = "sync-check";

    private Machine(string name, Store store, HttpRemote remote)
    {
        Name = name;
        Store = store;
        Remote = remote;
    }

    public string Name { get; }
    public Store Store { get; }
    public HttpRemote Remote { get; }

    public static Machine Open(string name, string directory, string url, string token)
    {
        Directory.CreateDirectory(directory);
        var store = Store.Open(Path.Combine(directory, "armory.db")).Match(
            store => store,
            error => throw new InvalidOperationException($"{name}: {error}"));
        store.SetMachine(name);
        var remote = HttpRemote.Create(url, token, name, Account).Match(
            remote => remote,
            error => throw new InvalidOperationException($"{name}: {error}"));
        return new Machine(name, store, remote);
    }

    public Report Pass() => Store.Pass(Remote, Batch).Match(
        report => report,
        error => throw new InvalidOperationException($"{Name}: {error}"));

    /// <summary>Compare the two stores on everything a person would notice.</summary>
    public static bool Same(Store one, Store two) =>
        one.SessionsHeld(500).Value.SequenceEqual(two.SessionsHeld(500).Value)
        && one.WatchedItems().Value.SequenceEqual(two.WatchedItems().Value)
        && one.WatchedRealmList().Value.SequenceEqual(two.WatchedRealmList().Value)
        && one.RosterHeld().Value.Count == two.RosterHeld().Value.Count
        && Counted(one) == Counted(two)
        && one.CurrentRun().Value?.Run.Name == two.CurrentRun().Value?.Run.Name;

    public static Armory.Roster.CharacterKey Who => new("emerald-dream", "Somechar");

    public static Armory.Roster.Roster ARoster() => new(
    [
        new Armory.Roster.Character
        {
            Key = Who,
            Id = 1,
            RealmId = 1,
            DisplayName = "Somechar",
            RealmName = "Emerald Dream",
            Level = 80,
            Class = "Shaman",
            Race = "Orc",
            Faction = Armory.Roster.Faction.Horde,
            WowAccountId = 7,
        },
    ]);

    public static Armory.Addon.Collected Tallied(long count)
    {
        var collected = new Armory.Addon.Collected();
        collected.Tallies[Who] = [new Armory.Tally.Tally { Kind = Armory.Tally.Counting.Recipe, Key = "371637", Label = "Flask of Alchemical Chaos", Count = count }];
        return collected;
    }

    public static Armory.Run.Run ARun() => new()
    {
        Name = "The Second Time",
        Baseline = new Armory.Run.Baseline { TakenAt = DateTimeOffset.FromUnixTimeSeconds(1_700_000_000) },
        Cohort = new Armory.Roster.Cohort([Who]),
        Goals = [new Armory.Run.Goal { AchievementId = 1234, Standing = new Armory.Run.Standing.Unearned(), Bucket = new Armory.Run.Bucket.Observable() }],
    };

    /// <summary>A fixed moment rather than now, so a run is the same run twice: an evening is keyed by when it started, and a clock-derived one would make every run of this a new evening on a server somebody kept.</summary>
    public static Armory.Chronicle.Session Evening(string name, long day)
    {
        var startedAt = DateTimeOffset.FromUnixTimeSeconds(1_700_000_000 + (day * 86_400));
        return new Armory.Chronicle.Session
        {
            Character = new Armory.Roster.CharacterKey("emerald-dream", name),
            DisplayName = name,
            RealmName = "Emerald Dream",
            Class = "Shaman",
            Race = "Orc",
            Faction = Armory.Roster.Faction.Horde,
            StartedAt = startedAt,
            EndedAt = startedAt + TimeSpan.FromHours(3),
            StartLevel = 80,
            EndLevel = 80,
            StartMoney = 100,
            EndMoney = 200,
            StartItemLevel = 600,
            EndItemLevel = 600,
            Moments = [new Armory.Chronicle.Moment { At = 0, What = new Armory.Chronicle.Happening.Arrived("Nagrand", null, null) }],
        };
    }

    public static long Counted(Store store) =>
        store.TalliesHeld().Value.TryGetValue(Who, out var counted)
            ? counted.FirstOrDefault(tally => tally.Key == "371637")?.Count ?? 0
            : 0;

    public void Dispose()
    {
        Store.Dispose();
        Remote.Dispose();
    }
}
