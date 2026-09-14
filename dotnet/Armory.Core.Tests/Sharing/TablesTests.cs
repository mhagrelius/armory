using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Armory.Sharing;
using Xunit;

namespace Armory.Tests.Sharing;

/// <summary>Ported from <c>core/src/sync.rs</c>; the display names are the Rust test names.</summary>
public sealed class TablesTests
{
    [Fact(DisplayName = "every_scope_has_exactly_one_table_and_every_table_its_own_name")]
    public void Every_scope_has_exactly_one_table_and_every_table_its_own_name()
    {
        var scopes = new HashSet<Scope>();
        var names = new HashSet<string>(StringComparer.Ordinal);
        foreach (var table in Tables.All)
        {
            Assert.True(scopes.Add(table.Scope), $"{table.Scope} is described twice");
            Assert.True(names.Add(table.Name), $"two tables called {table.Name}");
        }
        Assert.Equal(Enum.GetValues<Scope>().Length, scopes.Count);
    }

    [Fact(DisplayName = "a_table_name_is_the_wire_name")]
    public void A_table_name_is_the_wire_name()
    {
        foreach (var table in Tables.All)
        {
            Assert.Equal(table.Scope, Tables.Named(table.Name));
            Assert.Equal(table.Name, Tables.NameOf(table.Scope));
        }
    }

    [Fact(DisplayName = "a_scope_this_build_has_never_heard_of_is_none_rather_than_a_guess")]
    public void A_scope_this_build_has_never_heard_of_is_none_rather_than_a_guess()
    {
        Assert.Null(Tables.Named("transmog"));
    }

    [Fact(DisplayName = "no_table_names_a_column_twice")]
    public void No_table_names_a_column_twice()
    {
        foreach (var table in Tables.All)
        {
            var seen = new HashSet<string>(StringComparer.Ordinal);
            foreach (var column in table.AllColumns)
            {
                Assert.True(seen.Add(column), $"{table.Name} names {column} twice");
            }
        }
    }

    [Fact(DisplayName = "a_guard_names_a_column_the_table_actually_has")]
    public void A_guard_names_a_column_the_table_actually_has()
    {
        foreach (var table in Tables.All.Where(table => table.Guard.Kind == GuardKind.Newer))
        {
            Assert.Contains(table.Guard.Column, table.AllColumns);
        }
    }

    [Fact(DisplayName = "the_column_a_table_is_guarded_on_is_never_itself_news")]
    public void The_column_a_table_is_guarded_on_is_never_itself_news()
    {
        // The rule `Stamp` exists for. A guard column that counted as a change
        // would make every one of these tables re-send itself every time it
        // was merely refreshed.
        foreach (var table in Tables.All.Where(table => table.Guard.Kind == GuardKind.Newer))
        {
            var column = table.Columns.SingleOrDefault(column => column.Name == table.Guard.Column);
            Assert.NotNull(column);
            Assert.Equal(Rule.Stamp, column.Rule);
        }
    }

    [Fact(DisplayName = "only_goal_carries_a_local_id_and_it_points_at_a_key_column")]
    public void Only_goal_carries_a_local_id_and_it_points_at_a_key_column()
    {
        foreach (var table in Tables.All.Where(table => table.LocalId is not null))
        {
            Assert.Equal(Scope.Goal, table.Scope);
            Assert.Equal(Scope.Run, table.LocalId!.Value.Scope);
            Assert.True(table.LocalId.Value.Position < table.Key.Count);
        }
    }

    [Fact(DisplayName = "a_run_is_described_before_its_goals")]
    public void A_run_is_described_before_its_goals()
    {
        // Not decoration: a goal whose run has not arrived is dropped, and the
        // order rows are applied in is the order they were written in.
        var run = Tables.All.ToList().FindIndex(table => table.Scope == Scope.Run);
        var goal = Tables.All.ToList().FindIndex(table => table.Scope == Scope.Goal);
        Assert.True(run < goal);
    }

    [Fact(DisplayName = "base64_round_trips_including_the_awkward_lengths")]
    public void Base64_round_trips_including_the_awkward_lengths()
    {
        for (var length = 0; length < 32; length++)
        {
            var bytes = Enumerable.Range(0, length).Select(n => (byte)(n * 7 + 3)).ToArray();
            var encoded = Wire.EncodeBase64(bytes);
            Assert.Equal(0, encoded.Length % 4);
            Assert.Equal(bytes, Wire.DecodeBase64(encoded));
        }
    }

    [Fact(DisplayName = "base64_matches_the_worked_example")]
    public void Base64_matches_the_worked_example()
    {
        Assert.Equal("QXJtb3J5", Wire.EncodeBase64("Armory"u8));
        Assert.Equal("TQ==", Wire.EncodeBase64("M"u8));
        Assert.Equal("TWE=", Wire.EncodeBase64("Ma"u8));
        Assert.Equal("Armory", Encoding.UTF8.GetString(Wire.DecodeBase64("QXJtb3J5")!));
    }

    [Fact(DisplayName = "something_that_is_not_base64_decodes_to_nothing_rather_than_to_half")]
    public void Something_that_is_not_base64_decodes_to_nothing_rather_than_to_half()
    {
        Assert.Null(Wire.DecodeBase64("not base64!"));
    }

    [Fact(DisplayName = "a_row_without_fields_is_a_deletion")]
    public void A_row_without_fields_is_a_deletion()
    {
        var gone = new Row { Scope = "watched", Key = [JsonValue.Create(1234)], Fields = null };
        Assert.True(gone.IsGone);
        // And it says so on the wire by leaving the field out entirely, rather
        // than by sending a null somebody could read as an empty row.
        var encoded = JsonSerializer.Serialize(gone, Wire.Json);
        Assert.DoesNotContain("fields", encoded, StringComparison.Ordinal);
        Assert.Equal("""{"scope":"watched","key":[1234]}""", encoded);
    }

    [Fact(DisplayName = "a parcel reads back the shape the Rust server writes")]
    public void A_parcel_reads_back_the_shape_the_rust_server_writes()
    {
        const string wire = """{"parcel":{"rows":[{"scope":"tally","key":["draenor","aeltor","recipe","1234"],"fields":[3,"Flask"]}]},"cursor":42,"more":true}""";
        var pulled = JsonSerializer.Deserialize<Pulled>(wire, Wire.Json)!;
        Assert.Equal(42, pulled.Cursor);
        Assert.True(pulled.More);
        var row = Assert.Single(pulled.Parcel.Rows);
        Assert.Equal("tally", row.Scope);
        Assert.Equal(4, row.Key.Count);
        Assert.Equal(3, row.Fields![0]!.GetValue<int>());
        Assert.Equal("Flask", row.Fields[1]!.GetValue<string>());
    }
}
