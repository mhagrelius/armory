using System.Globalization;
using System.Linq.Expressions;
using System.Text.Json;
using System.Text.Json.Nodes;
using Armory.Sharing;
using Microsoft.Data.Sqlite;
using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.ChangeTracking;
using Microsoft.EntityFrameworkCore.Metadata;

namespace Armory.Store;

/// <summary>
/// Whether writing a row should also put it in the log, and under whose name.
/// A client applying what it pulled must not enqueue it for pushing back; a
/// server applying what a client pushed must, or no other machine hears.
/// </summary>
public readonly record struct Recording(string? Machine)
{
    public static Recording Off => new((string?)null);

    public static Recording As(string machine) => new(machine);
}

/// <summary>What one pass moved.</summary>
public sealed record Report
{
    public int Sent { get; set; }
    public int Landed { get; set; }
    public int Removed { get; set; }

    /// <summary>Rows one end or the other could not read. Climbing here is what an older build looks like.</summary>
    public int Unreadable { get; set; }

    public bool IsEmpty => Sent == 0 && Landed == 0 && Removed == 0 && Unreadable == 0;
}

/// <summary>
/// The next thing a pass should do. A pass is a loop of these, and they are
/// here rather than in the caller because there are two callers: the shell,
/// which awaits between steps so the window keeps drawing, and the check
/// tool, which runs them straight through.
/// </summary>
public abstract record Step
{
    /// <summary>Send this, then hand the answer to <see cref="Store.AbsorbPush"/>.</summary>
    public sealed record Push(Parcel Parcel, long Through) : Step;

    /// <summary>Entries whose rows are no longer here. A sweep took them, and sweeps are not logged.</summary>
    public sealed record Drain(long Through) : Step;

    /// <summary>Ask for everything above this, then hand it to <see cref="Store.AbsorbPull"/>.</summary>
    public sealed record Pull(long Since) : Step;
}

/// <summary>
/// The store as one of several copies of itself: what two Armories say to
/// each other, kept apart from what one asks its database. Both ends run
/// this. The client's <c>change</c> table is an outbox; the server's is a
/// log. One table, two lifecycles, and the difference is who calls
/// <see cref="Drain"/>.
/// </summary>
/// <remarks>
/// A row on the wire is positional over <see cref="Tables"/>, and the same
/// code reads and writes twenty-seven tables. It does so through the model's
/// metadata rather than through twenty-seven copies of itself: the entity
/// type is found by its table name, its properties by their column names, and
/// the values go through the change tracker like any other write.
/// </remarks>
public sealed partial class Store
{
    /// <summary>Where the pull cursor is kept. The push side needs none: the outbox is the queue.</summary>
    public const string Pulled = "pulled";

    private readonly Dictionary<Scope, Shape> shapes = [];

    private enum Outcome
    {
        Written,
        Removed,
        Kept,
        Unreadable,
    }

    /// <summary>One table, as the model sees it: the entity, its key in wire order, its columns in wire order.</summary>
    private sealed record Shape(Table Table, IEntityType Entity, IReadOnlyList<IProperty> Key, IReadOnlyList<IProperty> Columns);

    // -- identity ---------------------------------------------------------------

    /// <summary>This installation's name in the log.</summary>
    public string Machine() => Setting("machine") ?? "";

    /// <summary>
    /// Name this installation, once, at startup. An id rather than a hostname:
    /// the only question this answers is "did I write this row".
    /// </summary>
    public void SetMachine(string id) => SetSetting("machine", id);

    public long Cursor(string name) =>
        long.TryParse(Setting(name), NumberStyles.Integer, CultureInfo.InvariantCulture, out var value) ? value : 0;

    public void SetCursor(string name, long value) => SetSetting(name, value.ToString(CultureInfo.InvariantCulture));

    // -- the log ----------------------------------------------------------------

    /// <summary>Turn the change log off, or back on. Off for exactly two things: applying a pull, and the purge.</summary>
    public void Record(bool on) => SetSetting("recording", on ? "1" : "0");

    /// <summary>Whether writes are being logged.</summary>
    public bool IsRecording() => Setting("recording") != "0";

    /// <summary>How much is waiting to go up, by scope. Ordered so the page does not reshuffle.</summary>
    public Result<List<(string Scope, int Count)>, StoreError> Queued() => Work(() =>
        Context.Changes.AsNoTracking()
            .GroupBy(change => change.Scope)
            .Select(group => new { Scope = group.Key, Count = group.Count() })
            .OrderByDescending(group => group.Count).ThenBy(group => group.Scope)
            .AsEnumerable()
            .Select(group => (group.Scope, group.Count))
            .ToList());

    /// <summary>The oldest thing waiting, if anything is.</summary>
    public Result<string?, StoreError> QueuedSince() => Work(() =>
        Context.Changes.AsNoTracking().Select(change => (string?)change.At).Min());

    /// <summary>
    /// Enqueue everything this machine already holds, once.
    /// </summary>
    /// <remarks>
    /// The triggers record writes, and an account here before sharing was set
    /// up was written before the triggers existed, so its log starts empty and
    /// a decade of play never leaves the machine. An entry already there was
    /// made by a trigger, which means the row has moved since, and its place
    /// in the queue is the newer fact, so it is left alone.
    /// </remarks>
    public Result<int, StoreError> SeedLog() => Work(() =>
    {
        if (Setting("seeded") == "1")
        {
            return 0;
        }
        var machine = Machine();
        var at = Clock.GetUtcNow().ToString("yyyy-MM-dd'T'HH:mm:ss'Z'", CultureInfo.InvariantCulture);
        var seeded = 0;
        foreach (var table in Tables.All)
        {
            var shape = ShapeOf(table.Scope);
            var already = Context.Changes.AsNoTracking()
                .Where(change => change.Scope == table.Name)
                .Select(change => change.Key)
                .ToHashSet(StringComparer.Ordinal);
            foreach (var entity in Rows(shape))
            {
                var key = KeyJson(shape, Context.Entry(entity));
                if (already.Add(key))
                {
                    Context.Changes.Add(new ChangeRow { Scope = table.Name, Key = key, Gone = 0, At = at, Machine = machine });
                    seeded++;
                }
            }
        }
        Context.SaveChanges();
        SetSetting("seeded", "1");
        return seeded;
    });

    /// <summary>
    /// Forget everything this machine believes about the server, for the one
    /// case where the server has been wiped. Clears the cursor and the seed
    /// mark; touches nothing in the account itself.
    /// </summary>
    public void ForgetServer()
    {
        SetCursor(Pulled, 0);
        SetSetting("seeded", "0");
    }

    public bool Seeded() => Setting("seeded") == "1";

    // -- reading rows out -------------------------------------------------------

    /// <summary>
    /// The next batch to push, and the highest <c>seq</c> in it. A row whose
    /// entry says it is there but which is not (a purge took it) is dropped
    /// from the batch and its entry with it; so is a body over the ceiling.
    /// </summary>
    public Result<(Parcel Parcel, long Through), StoreError> Outbox(int limit) => Work(() =>
    {
        var entries = Context.Changes.AsNoTracking().OrderBy(change => change.Seq).Take(limit).ToList();
        var parcel = new Parcel();
        long through = 0;
        var carrying = 0;
        foreach (var entry in entries)
        {
            // Full enough. The entries above stay in the queue for the next
            // batch; `through` is not advanced past them, which is the whole
            // of what stops a cached body wedging the pass.
            if (carrying >= Wire.MaxParcel && parcel.Rows.Count > 0)
            {
                break;
            }
            through = entry.Seq;
            if (Encode(entry) is { } row)
            {
                carrying += Wire.Weight(row);
                parcel.Rows.Add(row);
            }
        }
        return (parcel, through);
    });

    /// <summary>Forget everything the server has taken.</summary>
    public Result<int, StoreError> Drain(long through) => Work(() =>
        Context.Changes.Where(change => change.Seq <= through).ExecuteDelete());

    /// <summary>The server's side of a pull: everything above <paramref name="since"/> that <paramref name="machine"/> did not write.</summary>
    public Result<Pulled, StoreError> LogSince(long since, string machine, int limit) => Work(() =>
    {
        var entries = Context.Changes.AsNoTracking()
            .Where(change => change.Seq > since && change.Machine != machine)
            .OrderBy(change => change.Seq)
            .Take(limit)
            .ToList();
        var parcel = new Parcel();
        var cursor = since;
        var carrying = 0;
        var cutShort = false;
        foreach (var entry in entries)
        {
            if (carrying >= Wire.MaxParcel && parcel.Rows.Count > 0)
            {
                cutShort = true;
                break;
            }
            cursor = entry.Seq;
            if (Encode(entry) is { } row)
            {
                carrying += Wire.Weight(row);
                parcel.Rows.Add(row);
            }
        }
        // `more` is about the log, not the parcel: a batch that dropped every
        // row it read still moved the cursor.
        return new Pulled { Parcel = parcel, Cursor = cursor, More = cutShort || entries.Count == limit };
    });

    /// <summary>The highest <c>seq</c> the log holds.</summary>
    public long HighWater() => Context.Changes.AsNoTracking().Select(change => (long?)change.Seq).Max() ?? 0;

    /// <summary>Whether anything above <paramref name="since"/> was written by somebody other than <paramref name="machine"/>.</summary>
    public bool AnythingSince(long since, string machine) =>
        Context.Changes.AsNoTracking().Any(change => change.Seq > since && change.Machine != machine);

    /// <summary>One log entry as a wire row, or null when it cannot be sent.</summary>
    private Row? Encode(ChangeRow entry)
    {
        if (Tables.Named(entry.Scope) is not { } scope)
        {
            return null;
        }
        List<JsonNode?> key;
        try
        {
            if (JsonNode.Parse(entry.Key) is not JsonArray parsed)
            {
                return null;
            }
            key = [.. parsed.Select(node => node?.DeepClone())];
        }
        catch (JsonException)
        {
            return null;
        }

        if (entry.Gone != 0)
        {
            // A goal whose run is gone cannot be named on the wire.
            return OutwardKey(scope, key) is { } outward
                ? new Row { Scope = entry.Scope, Key = outward, Fields = null }
                : null;
        }
        if (ReadRow(scope, key) is not { } fields || OutwardKey(scope, key) is not { } sendable)
        {
            return null;
        }
        return new Row { Scope = entry.Scope, Key = sendable, Fields = fields };
    }

    /// <summary>The value columns of one row, in table order.</summary>
    private List<JsonNode?>? ReadRow(Scope scope, List<JsonNode?> key)
    {
        var shape = ShapeOf(scope);
        if (key.Count != shape.Key.Count)
        {
            return null;
        }

        // Asked before the body is read, so an auction dump is never pulled
        // into memory only to be dropped. The ceiling is on the raw bytes.
        if (scope == Scope.Response && key[0] is JsonValue urlValue && urlValue.TryGetValue<string>(out var url))
        {
            var size = Context.Responses.AsNoTracking().Where(row => row.Url == url).Select(row => (int?)row.Body.Length).FirstOrDefault();
            if (size > Wire.MaxBody)
            {
                return null;
            }
        }

        if (Find(shape, key) is not { } held)
        {
            return null;
        }
        var entry = Context.Entry(held);
        return shape.Columns.Select(column => Cell(entry.Property(column).CurrentValue, RuleOf(shape, column))).ToList();
    }

    /// <summary>
    /// The key as another machine should see it: unchanged for every table
    /// but <c>goal</c>, whose first key column is a local run id and travels
    /// as the run's stable key. Null when the run it names is gone, which
    /// makes the goal unsendable rather than sendable-and-wrong.
    /// </summary>
    private List<JsonNode?>? OutwardKey(Scope scope, List<JsonNode?> key)
    {
        var table = Tables.Of(scope);
        if (table.LocalId is not { } local)
        {
            return key;
        }
        if (key.ElementAtOrDefault(local.Position) is not JsonValue value || !value.TryGetValue<long>(out var id))
        {
            return null;
        }
        var runKey = Context.Runs.AsNoTracking().Where(run => run.Id == id).Select(run => run.Key).FirstOrDefault();
        if (string.IsNullOrEmpty(runKey))
        {
            return null;
        }
        var outward = new List<JsonNode?>(key.Select(node => node?.DeepClone()));
        outward[local.Position] = JsonValue.Create(runKey);
        return outward;
    }

    /// <summary>The key as this machine holds it.</summary>
    private List<JsonNode?>? InwardKey(Scope scope, List<JsonNode?> key)
    {
        var table = Tables.Of(scope);
        if (table.LocalId is not { } local)
        {
            return key;
        }
        if (key.ElementAtOrDefault(local.Position) is not JsonValue value || !value.TryGetValue<string>(out var runKey))
        {
            return null;
        }
        var id = Context.Runs.AsNoTracking().Where(run => run.Key == runKey).Select(run => (long?)run.Id).FirstOrDefault();
        if (id is null)
        {
            return null;
        }
        var inward = new List<JsonNode?>(key.Select(node => node?.DeepClone()));
        inward[local.Position] = JsonValue.Create(id.Value);
        return inward;
    }

    // -- writing rows in --------------------------------------------------------

    /// <summary>
    /// Apply a batch, in the order it arrived. Order matters for exactly one
    /// pair: a goal names its run, so the run has to land first, and the log
    /// preserves that.
    /// </summary>
    public Result<Applied, StoreError> Apply(Parcel parcel, Recording recording) => Work(() =>
    {
        // Whose name the triggers write, and whether they write at all. On a
        // client this is off. On the server it is on and under the pushing
        // machine's name, so the log tells every other machine and that one
        // nothing.
        var mine = Machine();
        if (recording.Machine is { } machine)
        {
            Record(true);
            SetSetting("machine", machine);
        }
        else
        {
            Record(false);
        }

        var applied = new Applied();
        try
        {
            foreach (var row in parcel.Rows)
            {
                Outcome outcome;
                try
                {
                    outcome = ApplyRow(row);
                    Context.SaveChanges();
                }
                catch (Exception error) when (error is SqliteException or DbUpdateException or InvalidOperationException or FormatException or OverflowException)
                {
                    // A single bad row must not lose the batch it arrived in.
                    outcome = Outcome.Unreadable;
                }
                finally
                {
                    Context.ChangeTracker.Clear();
                }
                applied = outcome switch
                {
                    Outcome.Written => applied with { Written = applied.Written + 1 },
                    Outcome.Removed => applied with { Removed = applied.Removed + 1 },
                    Outcome.Kept => applied with { Kept = applied.Kept + 1 },
                    _ => applied with { Unreadable = applied.Unreadable + 1 },
                };
            }
        }
        finally
        {
            Record(true);
            SetSetting("machine", mine);
        }
        return applied;
    });

    private Outcome ApplyRow(Row row)
    {
        if (Tables.Named(row.Scope) is not { } scope)
        {
            return Outcome.Unreadable;
        }
        var shape = ShapeOf(scope);
        if (InwardKey(scope, row.Key) is not { } key || key.Count != shape.Key.Count)
        {
            return Outcome.Unreadable;
        }

        var held = Find(shape, key);
        if (row.Fields is not { } fields)
        {
            if (held is null)
            {
                return Outcome.Kept;
            }
            Context.Remove(held);
            return Outcome.Removed;
        }
        if (fields.Count != shape.Columns.Count)
        {
            return Outcome.Unreadable;
        }

        var table = shape.Table;
        if (held is not null)
        {
            if (table.Guard.Kind == GuardKind.Keep)
            {
                // An evening and a price at an instant are statements about a
                // moment that has passed. What is held is the same statement.
                return Outcome.Kept;
            }
            if (table.Guard.Kind == GuardKind.Newer && !IsNewer(shape, Context.Entry(held), fields))
            {
                return Outcome.Kept;
            }
        }

        var arriving = held is null ? Context.Entry(Activator.CreateInstance(shape.Entity.ClrType)!) : Context.Entry(held);
        if (held is null)
        {
            for (var index = 0; index < shape.Key.Count; index++)
            {
                arriving.Property(shape.Key[index]).CurrentValue = ToClr(key[index], shape.Key[index].ClrType);
            }
        }

        // Settle each column against what is held: the larger of two counts,
        // two halves of a collectible merged, a body decoded. A stamp is set
        // but never argued from, the same rule the triggers follow, so "did
        // anything change" has one answer on both sides of the wire.
        var changed = held is null;
        for (var index = 0; index < shape.Columns.Count; index++)
        {
            var column = shape.Columns[index];
            var rule = RuleOf(shape, column);
            var property = arriving.Property(column);
            var mine = held is null ? null : property.CurrentValue;
            object? settled = rule switch
            {
                Rule.Max => Math.Max(AsLong(fields[index]) ?? 0, mine as long? ?? long.MinValue),
                Rule.MergeJson => Collections.Collectible.MergeJson(AsString(fields[index]) ?? "{}", mine as string ?? "{}"),
                Rule.Blob => AsString(fields[index]) is { } encoded && Wire.DecodeBase64(encoded) is { } bytes
                    ? bytes
                    : throw new FormatException("a body that is not base64"),
                _ => ToClr(fields[index], column.ClrType),
            };
            if (rule != Rule.Stamp && !Same(mine, settled))
            {
                changed = true;
            }
            property.CurrentValue = settled;
        }

        if (!changed)
        {
            return Outcome.Kept;
        }
        if (held is null)
        {
            Context.Add(arriving.Entity);
        }
        Context.SaveChanges();
        OnlyOneCurrentRun(scope, key);
        return Outcome.Written;
    }

    /// <summary>
    /// A run arriving as the current one makes every other run not current.
    /// The one place a row landing touches a row beside it: "the current run"
    /// is a singleton.
    /// </summary>
    private void OnlyOneCurrentRun(Scope scope, List<JsonNode?> key)
    {
        if (scope != Scope.Run || key.FirstOrDefault() is not JsonValue value || !value.TryGetValue<string>(out var runKey))
        {
            return;
        }
        var current = Context.Runs.AsNoTracking().Where(run => run.Key == runKey).Select(run => (long?)run.IsCurrent).FirstOrDefault();
        if (current != 1)
        {
            return;
        }
        Context.Runs.Where(run => run.Key != runKey && run.IsCurrent != 0).ExecuteUpdate(set => set.SetProperty(run => run.IsCurrent, 0L));
    }

    /// <summary>
    /// Whether an arriving row is at least as recent as the one held. Compared
    /// as text, which is right for RFC 3339 stamps written in UTC. A stamp
    /// that will not compare is treated as older than nothing, so the row
    /// lands: the alternative is a row that can never be corrected.
    /// </summary>
    private static bool IsNewer(Shape shape, EntityEntry held, List<JsonNode?> fields)
    {
        if (shape.Table.Guard.Column is not { } stamp)
        {
            return true;
        }
        var position = shape.Columns.ToList().FindIndex(column => column.GetColumnName() == stamp);
        if (position < 0)
        {
            return true;
        }
        var arriving = AsString(fields[position]) ?? "";
        return held.Property(shape.Columns[position]).CurrentValue is not string mine
            || string.CompareOrdinal(arriving, mine) >= 0;
    }

    // -- the pass -------------------------------------------------------------

    /// <summary>
    /// What to do next. Pushes before pulls, so a pass that dies half way has
    /// told the server about work that exists rather than only about work
    /// that is gone.
    /// </summary>
    public Result<Step, StoreError> NextStep(int batch) => Outbox(batch).Map(outbox =>
    {
        var (parcel, through) = outbox;
        if (parcel.Rows.Count > 0)
        {
            return (Step)new Step.Push(parcel, through);
        }
        return through > 0 ? new Step.Drain(through) : new Step.Pull(Cursor(Pulled));
    });

    /// <summary>The server has it. Forget it.</summary>
    public Result<Unit, StoreError> AbsorbPush(long through, int sent, Applied applied, Report report) =>
        Drain(through).Map(_ =>
        {
            report.Sent += sent;
            report.Unreadable += applied.Unreadable;
            return Unit.Value;
        });

    /// <summary>
    /// Write what arrived and move the cursor. Answers whether there is more.
    /// The cursor moves only once what it names is written, so a pass that
    /// dies between the two asks for the same batch again, which costs
    /// nothing because every rule here is idempotent.
    /// </summary>
    public Result<bool, StoreError> AbsorbPull(Pulled pulled, Report report) =>
        Apply(pulled.Parcel, Recording.Off).Map(applied =>
        {
            SetCursor(Pulled, pulled.Cursor);
            report.Landed += applied.Written;
            report.Removed += applied.Removed;
            report.Unreadable += applied.Unreadable;
            return pulled.More;
        });

    /// <summary>
    /// A whole pass, start to finish, blocking. What the check tool drives.
    /// The shell runs the same steps with an await between them instead.
    /// </summary>
    public Result<Report, SyncError> Pass(IRemote remote, int batch)
    {
        var report = new Report();
        static SyncError Fail(StoreError error) => new(error.Message);
        while (true)
        {
            var next = NextStep(batch);
            if (!next.IsOk)
            {
                return Result<Report, SyncError>.Err(Fail(next.Error));
            }
            switch (next.Value)
            {
                case Step.Push push:
                    {
                        var applied = remote.Push(push.Parcel);
                        if (!applied.IsOk)
                        {
                            return Result<Report, SyncError>.Err(applied.Error);
                        }
                        var absorbed = AbsorbPush(push.Through, push.Parcel.Rows.Count, applied.Value, report);
                        if (!absorbed.IsOk)
                        {
                            return Result<Report, SyncError>.Err(Fail(absorbed.Error));
                        }
                        break;
                    }
                case Step.Drain drain:
                    {
                        var drained = Drain(drain.Through);
                        if (!drained.IsOk)
                        {
                            return Result<Report, SyncError>.Err(Fail(drained.Error));
                        }
                        break;
                    }
                case Step.Pull pull:
                    {
                        var pulled = remote.Pull(pull.Since, batch);
                        if (!pulled.IsOk)
                        {
                            return Result<Report, SyncError>.Err(pulled.Error);
                        }
                        var more = AbsorbPull(pulled.Value, report);
                        if (!more.IsOk)
                        {
                            return Result<Report, SyncError>.Err(Fail(more.Error));
                        }
                        if (!more.Value)
                        {
                            return Result<Report, SyncError>.Ok(report);
                        }
                        break;
                    }
                default:
                    return Result<Report, SyncError>.Ok(report);
            }
        }
    }

    // -- the model, by table and column name ------------------------------------

    /// <summary>The entity behind one wire table, its key and columns matched by column name.</summary>
    private Shape ShapeOf(Scope scope)
    {
        if (shapes.TryGetValue(scope, out var shape))
        {
            return shape;
        }
        var table = Tables.Of(scope);
        var entity = Context.Model.GetEntityTypes().Single(type => type.GetTableName() == table.Name);
        var properties = entity.GetProperties().ToDictionary(property => property.GetColumnName(), StringComparer.Ordinal);
        shape = new Shape(
            table,
            entity,
            table.Key.Select(name => properties[name]).ToList(),
            table.Columns.Select(column => properties[column.Name]).ToList());
        shapes[scope] = shape;
        return shape;
    }

    private static Rule RuleOf(Shape shape, IProperty column) =>
        shape.Table.Columns.First(c => c.Name == column.GetColumnName()).Rule;

    /// <summary>
    /// One row by its wire key, tracked, or null. A query on the key columns
    /// rather than <c>Find</c> on the primary key: for every table but one
    /// they are the same columns, and for <c>run</c> they are not, because
    /// the wire key is the stable <c>key</c> and the primary key is the local
    /// autoincrement.
    /// </summary>
    private object? Find(Shape shape, List<JsonNode?> key)
    {
        var parameter = Expression.Parameter(shape.Entity.ClrType, "row");
        Expression? body = null;
        for (var index = 0; index < shape.Key.Count; index++)
        {
            var property = shape.Key[index];
            var value = ToClr(key[index], property.ClrType);
            if (value is null || property.PropertyInfo is null)
            {
                return null;
            }
            var equal = Expression.Equal(Expression.Property(parameter, property.PropertyInfo), Expression.Constant(value, property.ClrType));
            body = body is null ? equal : Expression.AndAlso(body, equal);
        }
        if (body is null)
        {
            return null;
        }
        var set = (IQueryable)typeof(DbContext).GetMethod(nameof(DbContext.Set), Type.EmptyTypes)!
            .MakeGenericMethod(shape.Entity.ClrType)
            .Invoke(Context, null)!;
        var filtered = Expression.Call(
            typeof(Queryable),
            nameof(Queryable.Where),
            [shape.Entity.ClrType],
            set.Expression,
            Expression.Quote(Expression.Lambda(body, parameter)));
        foreach (var row in set.Provider.CreateQuery(filtered))
        {
            return row;
        }
        return null;
    }

    /// <summary>Every row of one table, untracked.</summary>
    private IEnumerable<object> Rows(Shape shape)
    {
        var set = typeof(DbContext).GetMethod(nameof(DbContext.Set), Type.EmptyTypes)!
            .MakeGenericMethod(shape.Entity.ClrType)
            .Invoke(Context, null)!;
        return ((IQueryable<object>)set).AsNoTracking().AsEnumerable();
    }

    /// <summary>The key of one entity in exactly the encoding the triggers' <c>json_array</c> produces.</summary>
    private static string KeyJson(Shape shape, EntityEntry entry)
    {
        var array = new JsonArray();
        foreach (var property in shape.Key)
        {
            array.Add(Cell(entry.Property(property).CurrentValue, Rule.Take));
        }
        return array.ToJsonString();
    }

    // -- values across the seam -------------------------------------------------

    /// <summary>One stored value as JSON.</summary>
    private static JsonNode? Cell(object? value, Rule rule) => value switch
    {
        null => null,
        long number => JsonValue.Create(number),
        int number => JsonValue.Create((long)number),
        double number => double.IsFinite(number) ? JsonValue.Create(number) : null,
        string text => JsonValue.Create(text),
        // Nothing else in the schema is a blob; if something becomes one, this
        // says so rather than sending an empty string that reads as an answer.
        byte[] bytes => rule == Rule.Blob ? JsonValue.Create(Wire.EncodeBase64(bytes)) : null,
        _ => JsonValue.Create(value.ToString()),
    };

    /// <summary>One JSON value as the property wants it.</summary>
    private static object? ToClr(JsonNode? node, Type clr)
    {
        var target = Nullable.GetUnderlyingType(clr) ?? clr;
        if (node is null)
        {
            return null;
        }
        if (target == typeof(byte[]))
        {
            return AsString(node) is { } encoded ? Wire.DecodeBase64(encoded) : null;
        }
        if (node is not JsonValue value)
        {
            return target == typeof(string) ? node.ToJsonString() : null;
        }
        if (target == typeof(long))
        {
            if (value.TryGetValue<bool>(out var flag))
            {
                return flag ? 1L : 0L;
            }
            if (value.TryGetValue<long>(out var integer))
            {
                return integer;
            }
            if (value.TryGetValue<string>(out var digits) && long.TryParse(digits, NumberStyles.Integer, CultureInfo.InvariantCulture, out var parsed))
            {
                return parsed;
            }
            return null;
        }
        if (target == typeof(string))
        {
            return value.TryGetValue<string>(out var text) ? text : value.ToJsonString();
        }
        return value.ToJsonString();
    }

    private static bool Same(object? held, object? arriving) => (held, arriving) switch
    {
        (null, null) => true,
        (byte[] a, byte[] b) => a.AsSpan().SequenceEqual(b),
        _ => Equals(held, arriving),
    };

    private static long? AsLong(JsonNode? node) =>
        node is JsonValue value && value.TryGetValue<long>(out var number) ? number : null;

    private static string? AsString(JsonNode? node) =>
        node is JsonValue value && value.TryGetValue<string>(out var text) ? text : null;
}
