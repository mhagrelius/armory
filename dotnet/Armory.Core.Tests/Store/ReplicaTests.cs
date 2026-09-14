using System.Text.Json.Nodes;
using Armory.Sharing;
using Armory.Store;
using Microsoft.EntityFrameworkCore;
using Xunit;

namespace Armory.Tests.Store;

/// <summary>
/// Ported from <c>core/src/replica.rs</c>. Four of its tests need slices not
/// yet ported (<c>save_collected</c>, <c>save_run</c>, <c>record_snapshot</c>,
/// <c>save_owned</c>) and live with those slices' tests when they land:
/// <c>a_counter_never_goes_backwards_whichever_side_is_behind</c>,
/// <c>a_second_identical_write_enqueues_nothing</c>,
/// <c>a_run_and_its_goals_travel_and_stay_one_run</c>,
/// <c>a_goal_whose_run_never_arrived_is_dropped_rather_than_hung_off_another</c>.
/// </summary>
public sealed class ReplicaTests
{
    private static Armory.Store.Store StoreNamed(string machine)
    {
        var store = Armory.Store.Store.InMemory();
        store.SetMachine(machine);
        return store;
    }

    /// <summary>Push everything <paramref name="from"/> has waiting straight into <paramref name="to"/>, the way a pass would.</summary>
    private static Applied Carry(Armory.Store.Store from, Armory.Store.Store to)
    {
        var (parcel, through) = from.Outbox(10_000).Value;
        var applied = to.Apply(parcel, Recording.Off).Value;
        from.Drain(through);
        return applied;
    }

    private static JsonNode Json(object value) => JsonValue.Create(value)!;

    [Fact(DisplayName = "a_write_is_logged_without_the_writer_knowing_it_exists")]
    public void A_write_is_logged_without_the_writer_knowing_it_exists()
    {
        // `WatchItem` says nothing about syncing. The trigger does.
        using var store = StoreNamed("one");
        store.WatchItem(4306, "Silk Cloth");

        var (parcel, _) = store.Outbox(10).Value;
        var row = Assert.Single(parcel.Rows);
        Assert.Equal("watched", row.Scope);
        Assert.Equal(4306, row.Key[0]!.GetValue<long>());
        Assert.Equal("Silk Cloth", row.Fields![0]!.GetValue<string>());
    }

    [Fact(DisplayName = "writing_the_same_thing_again_is_not_news")]
    public void Writing_the_same_thing_again_is_not_news()
    {
        using var store = StoreNamed("one");
        store.WatchItem(4306, "Silk Cloth");
        store.Drain(store.HighWater());

        store.WatchItem(4306, "Silk Cloth");
        var (parcel, _) = store.Outbox(10).Value;
        Assert.Empty(parcel.Rows);
    }

    [Fact(DisplayName = "a_row_written_twice_is_one_entry_at_the_later_place")]
    public void A_row_written_twice_is_one_entry_at_the_later_place()
    {
        using var store = StoreNamed("one");
        store.WatchItem(4306, "Silk");
        store.WatchItem(4306, "Silk Cloth");

        var (parcel, _) = store.Outbox(10).Value;
        var row = Assert.Single(parcel.Rows);
        Assert.Equal("Silk Cloth", row.Fields![0]!.GetValue<string>());
    }

    [Fact(DisplayName = "what_one_machine_wrote_lands_on_the_other")]
    public void What_one_machine_wrote_lands_on_the_other()
    {
        using var one = StoreNamed("one");
        using var two = StoreNamed("two");
        one.WatchItem(4306, "Silk Cloth");

        var applied = Carry(one, two);
        Assert.Equal(1, applied.Written);
        Assert.Equal([(4306L, "Silk Cloth")], two.WatchedItems().Value);
    }

    [Fact(DisplayName = "what_arrived_is_not_sent_straight_back")]
    public void What_arrived_is_not_sent_straight_back()
    {
        // The failure this is here for: two machines handing each other the
        // same row forever, each pass looking like work.
        using var one = StoreNamed("one");
        using var two = StoreNamed("two");
        one.WatchItem(4306, "Silk Cloth");
        Carry(one, two);

        var (parcel, _) = two.Outbox(10).Value;
        Assert.Empty(parcel.Rows);
    }

    [Fact(DisplayName = "a_row_already_agreed_on_is_kept_rather_than_written")]
    public void A_row_already_agreed_on_is_kept_rather_than_written()
    {
        using var one = StoreNamed("one");
        using var two = StoreNamed("two");
        one.WatchItem(4306, "Silk Cloth");
        two.WatchItem(4306, "Silk Cloth");

        var (parcel, _) = one.Outbox(10).Value;
        var applied = two.Apply(parcel, Recording.Off).Value;
        Assert.Equal(0, applied.Written);
        Assert.Equal(1, applied.Kept);
    }

    [Fact(DisplayName = "a_deletion_travels")]
    public void A_deletion_travels()
    {
        using var one = StoreNamed("one");
        using var two = StoreNamed("two");
        one.WatchItem(4306, "Silk Cloth");
        Carry(one, two);

        one.UnwatchItem(4306);
        var applied = Carry(one, two);
        Assert.Equal(1, applied.Removed);
        Assert.Empty(two.WatchedItems().Value);
    }

    [Fact(DisplayName = "a_sweep_is_this_machines_expiry_and_not_a_deletion_anybody_else_hears_about")]
    public void A_sweep_is_this_machines_expiry_and_not_a_deletion_anybody_else_hears_about()
    {
        using var store = StoreNamed("one");
        store.StoreResponse("https://example.test/a", "body"u8.ToArray(), null);
        store.Drain(store.HighWater());

        store.Purge();
        var (parcel, _) = store.Outbox(10).Value;
        Assert.Empty(parcel.Rows);
        Assert.True(store.IsRecording(), "and the flag is put back");
    }

    [Fact(DisplayName = "a_scope_this_build_does_not_know_is_counted_rather_than_guessed_at")]
    public void A_scope_this_build_does_not_know_is_counted_rather_than_guessed_at()
    {
        using var store = StoreNamed("one");
        var parcel = new Parcel { Rows = [new Row { Scope = "transmog", Key = [Json(1)], Fields = [Json("x")] }] };
        var applied = store.Apply(parcel, Recording.Off).Value;
        Assert.Equal(1, applied.Unreadable);
        Assert.Equal(0, applied.Written);
    }

    [Fact(DisplayName = "a_row_of_the_wrong_shape_is_unreadable_rather_than_half_written")]
    public void A_row_of_the_wrong_shape_is_unreadable_rather_than_half_written()
    {
        using var store = StoreNamed("one");
        var parcel = new Parcel { Rows = [new Row { Scope = "watched", Key = [Json(1)], Fields = [Json("a"), Json("b")] }] };
        Assert.Equal(1, store.Apply(parcel, Recording.Off).Value.Unreadable);
    }

    [Fact(DisplayName = "a_server_logs_what_a_client_pushed_under_that_clients_name")]
    public void A_server_logs_what_a_client_pushed_under_that_clients_name()
    {
        using var one = StoreNamed("one");
        using var server = StoreNamed("server");
        one.WatchItem(4306, "Silk Cloth");

        var (parcel, _) = one.Outbox(10).Value;
        server.Apply(parcel, Recording.As("one"));

        // The machine that wrote it hears nothing back...
        var mine = server.LogSince(0, "one", 100).Value;
        Assert.Empty(mine.Parcel.Rows);

        // ...and every other machine does.
        var theirs = server.LogSince(0, "two", 100).Value;
        Assert.Single(theirs.Parcel.Rows);
        Assert.True(theirs.Cursor > 0);

        // And the server's own name is back where it was.
        Assert.Equal("server", server.Machine());
    }

    [Fact(DisplayName = "a_cursor_only_moves_forward_and_says_when_there_is_more")]
    public void A_cursor_only_moves_forward_and_says_when_there_is_more()
    {
        using var one = StoreNamed("one");
        using var server = StoreNamed("server");
        for (var item = 0; item < 5; item++)
        {
            one.WatchItem(item, "thing");
        }
        var (parcel, _) = one.Outbox(100).Value;
        server.Apply(parcel, Recording.As("one"));

        var first = server.LogSince(0, "two", 2).Value;
        Assert.Equal(2, first.Parcel.Rows.Count);
        Assert.True(first.More);

        var second = server.LogSince(first.Cursor, "two", 2).Value;
        Assert.Equal(2, second.Parcel.Rows.Count);
        Assert.True(second.Cursor > first.Cursor);

        var third = server.LogSince(second.Cursor, "two", 2).Value;
        Assert.Single(third.Parcel.Rows);
        Assert.False(third.More, "one short of a full batch is the end");
    }

    [Fact(DisplayName = "nothing_waiting_means_nothing_to_send")]
    public void Nothing_waiting_means_nothing_to_send()
    {
        using var store = StoreNamed("one");
        var (parcel, through) = store.Outbox(100).Value;
        Assert.Empty(parcel.Rows);
        Assert.Equal(0, through);
        Assert.Empty(store.Queued().Value);
    }

    [Fact(DisplayName = "a_queue_full_of_cached_bodies_is_sent_in_pieces")]
    public void A_queue_full_of_cached_bodies_is_sent_in_pieces()
    {
        // A batch bounded only by a row count is enormous for `response`. A
        // client that built one past the server's ceiling would rebuild the
        // same one on every pass forever.
        using var store = StoreNamed("one");
        var body = new byte[1024 * 1024];
        Array.Fill(body, (byte)'x');
        for (var index = 0; index < 40; index++)
        {
            store.StoreResponse($"https://example.test/{index}", body, null);
        }

        var (parcel, through) = store.Outbox(10_000).Value;
        Assert.True(parcel.Rows.Count < 40, $"the whole queue went in one batch: {parcel.Rows.Count} rows");
        Assert.NotEmpty(parcel.Rows);
        Assert.True(through > 0);

        // And the rest is still queued, rather than having been skipped.
        store.Drain(through);
        var (rest, _) = store.Outbox(10_000).Value;
        Assert.NotEmpty(rest.Rows);
    }

    [Fact(DisplayName = "one_row_larger_than_a_batch_still_goes_on_its_own")]
    public void One_row_larger_than_a_batch_still_goes_on_its_own()
    {
        using var store = StoreNamed("one");
        var body = new byte[3 * 1024 * 1024];
        Array.Fill(body, (byte)'x');
        store.StoreResponse("https://example.test/big", body, null);
        var (parcel, _) = store.Outbox(10_000).Value;
        Assert.Single(parcel.Rows);
    }

    [Fact(DisplayName = "a_body_too_large_to_carry_is_left_where_it_is")]
    public void A_body_too_large_to_carry_is_left_where_it_is()
    {
        using var store = StoreNamed("one");
        var body = new byte[Wire.MaxBody + 1];
        Array.Fill(body, (byte)'x');
        store.StoreResponse("https://example.test/auctions", body, null);
        var (parcel, through) = store.Outbox(10).Value;
        Assert.Empty(parcel.Rows);
        // But its entry still leaves the queue, or the pass retries it forever.
        Assert.True(through > 0);
    }

    [Fact(DisplayName = "no_table_is_left_out_of_the_wire_by_accident")]
    public void No_table_is_left_out_of_the_wire_by_accident()
    {
        // The schema is asked, rather than trusted. `change` is the log,
        // `sync_state` is the cursor and this installation's name, and
        // `__EFMigrationsHistory` is Entity Framework's own bookkeeping.
        using var store = StoreNamed("one");
        var held =
store.Context.Database
            .SqlQueryRaw<string>("SELECT name AS \"Value\" FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
            .ToHashSet(StringComparer.Ordinal);
        var travelling = Tables.All.Select(table => table.Name).ToHashSet(StringComparer.Ordinal);
        var keptBack = new HashSet<string>(StringComparer.Ordinal) { "change", "sync_state" };

        var unaccounted = held
            .Where(name => !travelling.Contains(name) && !keptBack.Contains(name) && !name.StartsWith("__EF", StringComparison.Ordinal))
            .ToList();
        Assert.True(unaccounted.Count == 0, $"these tables are in the schema and travel nowhere: {string.Join(", ", unaccounted)}");

        var phantom = travelling.Where(name => !held.Contains(name)).ToList();
        Assert.True(phantom.Count == 0, $"these are described on the wire and are not in the schema: {string.Join(", ", phantom)}");
    }

    [Fact(DisplayName = "an_account_that_predates_sharing_is_offered_up_rather_than_stranded")]
    public void An_account_that_predates_sharing_is_offered_up_rather_than_stranded()
    {
        using var store = Armory.Store.Store.InMemory();

        // Written with recording off, which is exactly what "these rows
        // predate the triggers" looks like from the log's point of view.
        store.Record(false);
        store.WatchItem(4306, "Silk Cloth");
        store.WatchItem(2589, "Linen Cloth");
        store.Record(true);
        store.SetMachine("one");

        Assert.Empty(store.Queued().Value);
        Assert.False(store.Seeded());

        var seeded = store.SeedLog().Value;
        Assert.True(seeded >= 2, $"seeded {seeded}");
        Assert.True(store.Seeded());

        var (parcel, _) = store.Outbox(100).Value;
        var watched = parcel.Rows.Where(row => row.Scope == "watched").ToList();
        Assert.Equal(2, watched.Count);
        // And the rows carry their contents, not just their keys.
        Assert.All(watched, row => Assert.NotNull(row.Fields));
    }

    [Fact(DisplayName = "seeding_twice_does_not_re_send_the_account")]
    public void Seeding_twice_does_not_re_send_the_account()
    {
        using var store = Armory.Store.Store.InMemory();
        store.SetMachine("one");
        store.Record(false);
        store.WatchItem(4306, "Silk Cloth");
        store.Record(true);

        store.SeedLog();
        store.Drain(store.HighWater());

        Assert.Equal(0, store.SeedLog().Value);
        Assert.Empty(store.Queued().Value);
    }

    [Fact(DisplayName = "seeding_does_not_displace_something_already_waiting")]
    public void Seeding_does_not_displace_something_already_waiting()
    {
        // A row the triggers already logged has moved since; its place at the
        // back of the queue is the newer fact and must survive the seed.
        using var store = Armory.Store.Store.InMemory();
        store.SetMachine("one");
        store.Record(false);
        store.WatchItem(1, "old");
        store.Record(true);
        store.WatchItem(2, "new");

        var before = store.Context.Changes.AsNoTracking().Single(change => change.Scope == "watched" && change.Key == "[2]").Seq;
        store.SeedLog();
        var after = store.Context.Changes.AsNoTracking().Single(change => change.Scope == "watched" && change.Key == "[2]").Seq;
        Assert.Equal(before, after);
    }

    [Fact(DisplayName = "forgetting_the_server_offers_the_account_up_again")]
    public void Forgetting_the_server_offers_the_account_up_again()
    {
        using var store = StoreNamed("one");
        store.Record(false);
        store.WatchItem(4306, "Silk Cloth");
        store.Record(true);

        store.SeedLog();
        store.Drain(store.HighWater());
        store.SetCursor(Armory.Store.Store.Pulled, 9_999);
        Assert.True(store.Seeded());
        Assert.Empty(store.Queued().Value);

        store.ForgetServer();

        Assert.False(store.Seeded());
        Assert.Equal(0, store.Cursor(Armory.Store.Store.Pulled));
        Assert.True(store.SeedLog().Value > 0);
        Assert.NotEmpty(store.Queued().Value);
    }

    [Fact(DisplayName = "every_table_that_travels_has_its_triggers")]
    public void Every_table_that_travels_has_its_triggers()
    {
        using var store = StoreNamed("one");
        var names = store.Context.Database
            .SqlQueryRaw<string>("SELECT name AS \"Value\" FROM sqlite_master WHERE type = 'trigger'")
            .ToList();
        foreach (var table in Tables.All)
        {
            Assert.Contains($"log_{table.Name}_ins", names);
            Assert.Contains($"log_{table.Name}_del", names);
            if (table.Columns.Count > 0)
            {
                Assert.Contains($"log_{table.Name}_upd", names);
            }
        }
    }

    [Fact(DisplayName = "a whole pass converges two machines through a server")]
    public void A_whole_pass_converges_two_machines_through_a_server()
    {
        // What sync-check does with a real socket, done in-process: the
        // server is a third store applying under each client's name.
        using var server = StoreNamed("server");
        using var one = StoreNamed("one");
        using var two = StoreNamed("two");
        one.WatchItem(4306, "Silk Cloth");
        two.WatchRealm(61, "Emerald Dream");

        var first = one.Pass(new InProcessRemote(server, "one"), 100).Value;
        Assert.Equal(1, first.Sent);
        var second = two.Pass(new InProcessRemote(server, "two"), 100).Value;
        Assert.Equal(1, second.Sent);
        Assert.Equal(1, second.Landed);
        var third = one.Pass(new InProcessRemote(server, "one"), 100).Value;
        Assert.Equal(1, third.Landed);

        Assert.Equal(two.WatchedItems().Value, one.WatchedItems().Value);
        Assert.Equal(two.WatchedRealmList().Value, one.WatchedRealmList().Value);
        // And a fourth pass moves nothing: the log is the size of the data.
        Assert.True(one.Pass(new InProcessRemote(server, "one"), 100).Value.IsEmpty);
    }

    /// <summary>The Rust server's three routes, answered by a store in the same process.</summary>
    private sealed class InProcessRemote : IRemote
    {
        private readonly Armory.Store.Store server;
        private readonly string machine;

        public InProcessRemote(Armory.Store.Store server, string machine)
        {
            this.server = server;
            this.machine = machine;
        }

        public Result<Applied, SyncError> Push(Parcel parcel) =>
            server.Apply(parcel, Recording.As(machine)).Match(Result<Applied, SyncError>.Ok, error => Result<Applied, SyncError>.Err(new SyncError(error.Message)));

        public Result<Pulled, SyncError> Pull(long since, int limit) =>
            server.LogSince(since, machine, limit).Match(Result<Pulled, SyncError>.Ok, error => Result<Pulled, SyncError>.Err(new SyncError(error.Message)));

        public Result<bool, SyncError> Wait(long since) => Result<bool, SyncError>.Ok(server.AnythingSince(since, machine));
    }
}
