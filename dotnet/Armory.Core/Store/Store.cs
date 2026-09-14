using System.Globalization;
using Armory.Sharing;
using Microsoft.Data.Sqlite;
using Microsoft.EntityFrameworkCore;

namespace Armory.Store;

/// <summary>
/// What went wrong talking to the database. Storage failing is not an
/// expected outcome of using the application the way a source refusing is,
/// so this is the plain error at the boundary and nothing above it works in
/// exceptions.
/// </summary>
public sealed record StoreError(string Message)
{
    public override string ToString() => Message;
}

/// <summary>
/// Local storage: the roster, the cohort, the response cache, and everything
/// the addon and the API have said. One SQLite file, one context, one
/// thread.
/// </summary>
/// <remarks>
/// <para>The expiry in <see cref="Purge"/> is not an optimisation. Blizzard's
/// API terms require a thirty-day time-to-live on data obtained through the
/// API, so it is an obligation with a deadline.</para>
/// <para>The change log is kept by triggers, generated from
/// <see cref="Tables"/>, and the recording flag those triggers read lives in
/// the file rather than the connection. So exactly one thread ever writes
/// through a <see cref="Store"/>: a second writer applying a pull would
/// silence the first writer's changes for as long as it took and neither
/// would know.</para>
/// <para>Every public operation is one unit of work: it runs against the
/// context, saves, and clears the change tracker, so a context that lives
/// as long as the application does not accumulate the whole account.</para>
/// </remarks>
public sealed partial class Store : IDisposable
{
    /// <summary>How long anything from Blizzard may be kept. Set by their terms, not by us.</summary>
    public const int MaxTtlDays = 30;

    private readonly SqliteConnection connection;

    private Store(SqliteConnection connection, TimeProvider clock)
    {
        this.connection = connection;
        Clock = clock;
        var options = new DbContextOptionsBuilder<ArmoryContext>()
            .UseSqlite(connection)
            .Options;
        Context = new ArmoryContext(options);
    }

    /// <summary>The context every query goes through.</summary>
    public ArmoryContext Context { get; }

    /// <summary>Where "now" comes from, so a test can say what time it is.</summary>
    public TimeProvider Clock { get; }

    /// <summary>Open, or create, the database at <paramref name="path"/>.</summary>
    public static Result<Store, StoreError> Open(string path, TimeProvider? clock = null)
    {
        try
        {
            var builder = new SqliteConnectionStringBuilder { DataSource = path };
            var store = new Store(new SqliteConnection(builder.ToString()), clock ?? TimeProvider.System);
            store.Migrate();
            return Result<Store, StoreError>.Ok(store);
        }
        catch (Exception error) when (error is SqliteException or DbUpdateException or IOException)
        {
            return Result<Store, StoreError>.Err(new StoreError(error.Message));
        }
    }

    /// <summary>An in-memory database, for tests. Failing to make one is a bug, not an outcome.</summary>
    public static Store InMemory(TimeProvider? clock = null)
    {
        var store = new Store(new SqliteConnection("Data Source=:memory:"), clock ?? TimeProvider.System);
        store.Migrate();
        return store;
    }

    public void Dispose()
    {
        Context.Dispose();
        connection.Dispose();
        // Microsoft.Data.Sqlite pools connections, so the file stays open
        // after the last one is disposed. A store that has been closed must
        // release its file: the next open, a copy, or a delete all want it.
        SqliteConnection.ClearPool(connection);
    }

    private void Migrate()
    {
        connection.Open();
        Context.Database.ExecuteSqlRaw("PRAGMA foreign_keys = ON");
        Context.Database.Migrate();
        // The one thing Entity Framework has no model for. The trigger text
        // is generated from the same registry the wire uses, so a table that
        // travels is a table that records, with no third list to keep level.
        Context.Database.ExecuteSqlRaw(Triggers());

        // Recording is only ever off inside `Apply` and `Purge`, both of which
        // put it back. A process that died between the two would otherwise
        // leave an installation that quietly never syncs again.
        SetSetting("recording", "1");
    }

    /// <summary>
    /// The triggers that keep the change log, one set a table, generated from
    /// <see cref="Tables"/>.
    /// </summary>
    /// <remarks>
    /// Triggers rather than a call at the end of every write: there are two
    /// dozen ways to write to this store and the one added next year would
    /// not remember. Two things fall out for free. <c>WHEN old.c IS NOT
    /// new.c</c> means an upsert that writes the values already there logs
    /// nothing. And <c>json_array</c> builds the key in exactly the encoding
    /// the wire uses, so the replica reads it back with no agreement to
    /// maintain. Keys are never updated in this schema, so the update trigger
    /// logs the new key and does not chase an old one.
    /// </remarks>
    internal static string Triggers()
    {
        // NULL when the row is absent, which `IS NOT '0'` reads as on. A store
        // that has never been told is a store that records.
        const string on = "(SELECT value FROM sync_state WHERE name = 'recording') IS NOT '0'";
        const string who = "COALESCE((SELECT value FROM sync_state WHERE name = 'machine'), '')";
        const string now = "strftime('%Y-%m-%dT%H:%M:%SZ', 'now')";

        var sql = new System.Text.StringBuilder();
        foreach (var table in Tables.All)
        {
            var name = table.Name;
            string KeyOf(string side) => "json_array(" + string.Join(", ", table.Key.Select(column => $"{side}.{column}")) + ")";

            void Record(string suffix, string evt, string side, int gone, string extra)
            {
                var key = KeyOf(side);
                sql.Append(CultureInfo.InvariantCulture, $"""
                    DROP TRIGGER IF EXISTS log_{name}_{suffix};
                    CREATE TRIGGER log_{name}_{suffix} AFTER {evt} ON {name}
                    WHEN {on}{extra}
                    BEGIN
                      DELETE FROM change WHERE scope = '{name}' AND key = {key};
                      INSERT INTO change (scope, key, gone, at, machine)
                        VALUES ('{name}', {key}, {gone}, {now}, {who});
                    END;

                    """);
            }

            Record("ins", "INSERT", "new", 0, "");
            Record("del", "DELETE", "old", 1, "");

            // A table that is nothing but its key (`enrolment`) has no update
            // to notice. Being there is the whole record.
            var differs = table.Columns
                .Where(column => column.Rule != Rule.Stamp)
                .Select(column => $"old.{column.Name} IS NOT new.{column.Name}")
                .ToList();
            if (differs.Count > 0)
            {
                Record("upd", "UPDATE", "new", 0, " AND (" + string.Join(" OR ", differs) + ")");
            }
        }
        return sql.ToString();
    }

    // -- one unit of work -------------------------------------------------------

    /// <summary>
    /// The seam: run against the context, save, and let go of everything
    /// tracked. SQLite's exceptions become the typed error once, here.
    /// </summary>
    internal Result<T, StoreError> Work<T>(Func<T> work)
    {
        try
        {
            var value = work();
            Context.SaveChanges();
            return Result<T, StoreError>.Ok(value);
        }
        catch (Exception error) when (error is SqliteException or DbUpdateException or FormatException or InvalidOperationException)
        {
            return Result<T, StoreError>.Err(new StoreError(error.Message));
        }
        finally
        {
            Context.ChangeTracker.Clear();
        }
    }

    internal Result<Unit, StoreError> Work(Action work) => Work(() =>
    {
        work();
        return Unit.Value;
    });

    /// <summary>"Now", as every stamp in this store is written.</summary>
    internal string Now() => Stamp(Clock.GetUtcNow());

    /// <summary>RFC 3339 in UTC, which sorts as text in the order it happened.</summary>
    internal static string Stamp(DateTimeOffset at) => at.ToUniversalTime().ToString("o", CultureInfo.InvariantCulture);

    /// <summary>
    /// An RFC 3339 column back as a time, or nothing. A row whose stamp will
    /// not parse is skipped by every caller rather than defaulted to the
    /// epoch: a journal entry dated 1970 is worse than one briefly missing.
    /// </summary>
    public static DateTimeOffset? ParseStamp(string? text) =>
        DateTimeOffset.TryParse(text, CultureInfo.InvariantCulture, DateTimeStyles.AssumeUniversal | DateTimeStyles.AdjustToUniversal, out var at)
            ? at
            : null;

    private static TimeSpan Bounded(TimeSpan ttl) =>
        ttl < TimeSpan.FromDays(MaxTtlDays) ? ttl : TimeSpan.FromDays(MaxTtlDays);

    // -- the small key/value table ----------------------------------------------

    public string? Setting(string name) =>
        Context.SyncState.AsNoTracking().Where(row => row.Name == name).Select(row => row.Value).FirstOrDefault();

    /// <summary>Saved on its own, because the triggers read it mid-operation.</summary>
    public void SetSetting(string name, string value)
    {
        var held = Context.SyncState.Find(name);
        if (held is null)
        {
            Context.SyncState.Add(new SyncStateRow { Name = name, Value = value });
        }
        else
        {
            held.Value = value;
        }
        Context.SaveChanges();
        Context.ChangeTracker.Clear();
    }

    // -- reconcile ------------------------------------------------------------

    /// <summary>
    /// Write a whole set of rows, and remove whatever is no longer among them.
    /// </summary>
    /// <remarks>
    /// The shape every table that is replaced rather than merged takes, and
    /// the reason is the change log rather than performance: a delete and
    /// rewrite tells the log that every row moved. The change tracker only
    /// issues an update for a row whose values differ, so a repeat write
    /// reaches the triggers for none of them. <paramref name="within"/> is
    /// the slice being replaced: one realm, one recipe, one kind. A snapshot
    /// replaces a realm and must not delete the other realms while it is at
    /// it.
    /// </remarks>
    internal void Reconcile<TRow, TKey>(
        IQueryable<TRow> within,
        IReadOnlyList<TRow> rows,
        Func<TRow, TKey> keyOf,
        Action<TRow, TRow> assign)
        where TRow : class
        where TKey : notnull
    {
        var held = within.ToDictionary(keyOf);
        foreach (var row in rows)
        {
            var key = keyOf(row);
            if (held.Remove(key, out var existing))
            {
                assign(existing, row);
            }
            else
            {
                Context.Add(row);
            }
        }
        Context.RemoveRange(held.Values);
    }

    // -- the response cache ---------------------------------------------------

    public Result<Unit, StoreError> StoreResponse(string url, byte[] body, string? lastModified) => Work(() =>
    {
        var held = Context.Responses.Find(url);
        if (held is null)
        {
            Context.Responses.Add(new ResponseRow { Url = url, Body = body, LastModified = lastModified, FetchedAt = Now() });
        }
        else
        {
            held.Body = body;
            held.LastModified = lastModified;
            held.FetchedAt = Now();
        }
    });

    /// <summary>A stored body, if it is there and still inside its time-to-live.</summary>
    public Result<byte[]?, StoreError> Response(string url, TimeSpan ttl) => Work(() =>
    {
        var held = Context.Responses.AsNoTracking()
            .Where(row => row.Url == url)
            .Select(row => new { row.Body, row.FetchedAt })
            .FirstOrDefault();
        if (held is null || ParseStamp(held.FetchedAt) is not { } fetchedAt)
        {
            return null;
        }
        // Never longer than the terms allow, whatever the caller asked for.
        return Clock.GetUtcNow() - fetchedAt > Bounded(ttl) ? null : held.Body;
    });

    /// <summary>
    /// Every stored body whose URL contains <paramref name="needle"/> and is
    /// still inside <paramref name="ttl"/>. For reading a whole family of
    /// small responses back at once, the way restoring artwork does.
    /// </summary>
    public Result<List<(string Url, byte[] Body)>, StoreError> ResponsesMatching(string needle, TimeSpan ttl) => Work(() =>
    {
        var cutoff = Stamp(Clock.GetUtcNow() - Bounded(ttl));
        return Context.Responses.AsNoTracking()
            .Where(row => row.Url.Contains(needle) && row.FetchedAt.CompareTo(cutoff) > 0)
            .Select(row => new { row.Url, row.Body })
            .AsEnumerable()
            .Select(row => (row.Url, row.Body))
            .ToList();
    });

    /// <summary>
    /// The Last-Modified a URL last answered with. Separate from
    /// <see cref="Response"/>: the stamp outlives the caller's freshness
    /// window, because a stale body is still the one the server confirms
    /// with a 304.
    /// </summary>
    public Result<string?, StoreError> LastModified(string url) => Work(() =>
        Context.Responses.AsNoTracking().Where(row => row.Url == url).Select(row => row.LastModified).FirstOrDefault());

    /// <summary>Mark a URL as confirmed current without rewriting its body. What a 304 means.</summary>
    public Result<Unit, StoreError> TouchResponse(string url) => Work(() =>
    {
        var now = Now();
        Context.Responses.Where(row => row.Url == url).ExecuteUpdate(set => set.SetProperty(row => row.FetchedAt, now));
    });

    // -- the watch list -------------------------------------------------------

    public Result<List<(long ItemId, string Name)>, StoreError> WatchedItems() => Work(() =>
        Context.Watched.AsNoTracking()
            .OrderBy(row => row.Name).ThenBy(row => row.ItemId)
            .Select(row => new { row.ItemId, row.Name })
            .AsEnumerable()
            .Select(row => (row.ItemId, row.Name))
            .ToList());

    public Result<Unit, StoreError> WatchItem(long itemId, string name) => Work(() =>
    {
        var held = Context.Watched.Find(itemId);
        if (held is null)
        {
            Context.Watched.Add(new WatchedRow { ItemId = itemId, Name = name });
        }
        else
        {
            held.Name = name;
        }
    });

    public Result<Unit, StoreError> UnwatchItem(long itemId) => Work(() =>
    {
        Context.Watched.Where(row => row.ItemId == itemId).ExecuteDelete();
    });

    public Result<List<(long RealmId, string Name)>, StoreError> WatchedRealmList() => Work(() =>
        Context.WatchedRealms.AsNoTracking()
            .OrderBy(row => row.Name).ThenBy(row => row.RealmId)
            .Select(row => new { row.RealmId, row.Name })
            .AsEnumerable()
            .Select(row => (row.RealmId, row.Name))
            .ToList());

    public Result<Unit, StoreError> WatchRealm(long realmId, string name) => Work(() =>
    {
        var held = Context.WatchedRealms.Find(realmId);
        if (held is null)
        {
            Context.WatchedRealms.Add(new WatchedRealmRow { RealmId = realmId, Name = name });
        }
        else
        {
            held.Name = name;
        }
    });

    public Result<Unit, StoreError> UnwatchRealm(long realmId) => Work(() =>
    {
        Context.WatchedRealms.Where(row => row.RealmId == realmId).ExecuteDelete();
    });

    // -- expiry ---------------------------------------------------------------

    /// <summary>
    /// Sweep what is past the thirty-day term. Returns how many rows went.
    /// </summary>
    /// <remarks>
    /// <c>session</c> and <c>entry</c> are deliberately not here: the term is
    /// a condition on data obtained through Blizzard's API, and the journal
    /// came off the addon. And the sweep is not a statement that the data is
    /// gone: recording is off for the whole of it, so a deletion here stays
    /// on this machine. With it on, one laptop's expiry would travel and a
    /// machine switched off for a month would come back and take the last
    /// month off everything else, looking exactly like the sweep working.
    /// </remarks>
    public Result<int, StoreError> Purge() => Work(() =>
    {
        SetSetting("recording", "0");
        try
        {
            var cutoff = Stamp(Clock.GetUtcNow() - TimeSpan.FromDays(MaxTtlDays));
            var responses = Context.Responses.Where(row => row.FetchedAt.CompareTo(cutoff) < 0).ExecuteDelete();
            var prices = Context.Prices.Where(row => row.SeenAt.CompareTo(cutoff) < 0).ExecuteDelete();
            return responses + prices;
        }
        finally
        {
            SetSetting("recording", "1");
        }
    });
}
