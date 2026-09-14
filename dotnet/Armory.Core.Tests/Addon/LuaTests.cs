using Armory.Addon;
using Xunit;

namespace Armory.Tests.Addon;

/// <summary>Ported from <c>core/src/addon/lua.rs</c>.</summary>
public sealed class LuaTests
{
    private static SortedDictionary<string, LuaValue> Parsed(string source) => Lua.Parse(source).Value;

    [Fact(DisplayName = "a_saved_variables_file_reads_into_its_globals")]
    public void A_saved_variables_file_reads_into_its_globals()
    {
        const string source = """

            ArmoryCollectorDB = {
            	["version"] = 1,
            	["characters"] = {
            		["Somechar-Emerald Dream"] = {
            			["level"] = 80,
            			["gold"] = 123456,
            		},
            	},
            }

            """;
        var db = Parsed(source)["ArmoryCollectorDB"];
        Assert.Equal(1, db.Get("version")?.AsInteger());

        var character = db.Get("characters")?.Get("Somechar-Emerald Dream");
        Assert.NotNull(character);
        Assert.Equal(80, character.Get("level")?.AsInteger());
    }

    [Fact(DisplayName = "numeric_keys_survive_as_numbers")]
    public void Numeric_keys_survive_as_numbers()
    {
        // Achievement ids are numeric keys, and reading them as strings would
        // mean re-parsing every one at every lookup.
        var table = Parsed("""X = { [4956] = "Aeltor", [1234] = "Somechar" }""")["X"];
        var ids = table.Entries().Select(entry => entry.Key.AsInteger()).OfType<long>().ToList();
        Assert.Equal([1234L, 4956L], ids);
        Assert.Equal("Aeltor", table.Get("4956")?.AsStr());
    }

    [Fact(DisplayName = "both_halves_of_a_table_are_kept_apart")]
    public void Both_halves_of_a_table_are_kept_apart()
    {
        var table = Parsed("""X = { "first", "second", ["name"] = "third" }""")["X"];
        Assert.Equal(2, table.Items().Count);
        Assert.Equal("first", table.Items()[0].AsStr());
        Assert.Equal("third", table.Get("name")?.AsStr());
    }

    [Fact(DisplayName = "multi_byte_escapes_reassemble_into_utf8")]
    public void Multi_byte_escapes_reassemble_into_utf8()
    {
        // The writer escapes anything above ASCII as `\ddd` bytes, and every
        // realm name with an accent arrives this way.
        var globals = Parsed("""X = { ["realm"] = "Kh\195\182l" }""");
        Assert.Equal("Khöl", globals["X"].Get("realm")?.AsStr());
    }

    [Fact(DisplayName = "comments_are_skipped_in_both_forms")]
    public void Comments_are_skipped_in_both_forms()
    {
        const string source = """

            -- a line comment
            X = { --[[ a long one ]] ["a"] = 1 }

            """;
        Assert.Equal(1, Parsed(source)["X"].Get("a")?.AsInteger());
    }

    [Fact(DisplayName = "unquoted_keys_and_the_usual_scalars_all_read")]
    public void Unquoted_keys_and_the_usual_scalars_all_read()
    {
        var table = Parsed("X = { enabled = true, off = false, missing = nil, ratio = -1.5e2 }")["X"];
        Assert.Equal(true, table.Get("enabled")?.AsBool());
        Assert.Equal(false, table.Get("off")?.AsBool());
        Assert.Equal(LuaValue.Nil.Value, table.Get("missing"));
        Assert.Equal(-150.0, table.Get("ratio")?.AsDouble());
    }

    [Fact(DisplayName = "a_sparse_array_padded_with_nil_reads")]
    public void A_sparse_array_padded_with_nil_reads()
    {
        // What WoW actually writes for a table keyed by mount id: positional
        // holes up to the first entry, then the entries.
        var table = Parsed("""X = { nil, nil, { "Brown Horse", 1 }, }""")["X"];
        Assert.Equal(3, table.Items().Count);
        Assert.Equal(LuaValue.Nil.Value, table.Items()[0]);
        Assert.Equal("Brown Horse", table.Items()[2].Items()[0].AsStr());
    }

    [Fact(DisplayName = "bare_booleans_in_the_array_part_read_too")]
    public void Bare_booleans_in_the_array_part_read_too()
    {
        var table = Parsed("X = { true, false, nil }")["X"];
        Assert.Equal(3, table.Items().Count);
        Assert.Equal(new LuaValue.Bool(true), table.Items()[0]);
    }

    [Fact(DisplayName = "an_unquoted_key_still_works_beside_them")]
    public void An_unquoted_key_still_works_beside_them()
    {
        var table = Parsed("X = { nil, enabled = true, nil }")["X"];
        Assert.Equal(2, table.Items().Count);
        Assert.Equal(new LuaValue.Bool(true), table.Get("enabled"));
    }

    [Fact(DisplayName = "the_hybrid_shape_the_serializer_actually_emits_reads")]
    public void The_hybrid_shape_the_serializer_actually_emits_reads()
    {
        // Real dumps switch part-way: positional while the ids are dense, then
        // keyed once they are not.
        var table = Parsed("""X = { nil, { "a" }, [382] = { "b" }, }""")["X"];
        Assert.Equal(2, table.Items().Count);
        Assert.Equal("b", table.Get("382")?.Items()[0].AsStr());
    }

    [Fact(DisplayName = "anything_that_is_not_data_is_refused_rather_than_evaluated")]
    public void Anything_that_is_not_data_is_refused_rather_than_evaluated()
    {
        // This reads a file out of a directory addon managers also write to.
        Assert.False(Lua.Parse("""X = os.execute("rm -rf /")""").IsOk);
        Assert.False(Lua.Parse("""X = { [1] = loadstring("...") }""").IsOk);
    }

    [Fact(DisplayName = "a_truncated_file_is_an_error_and_not_a_panic")]
    public void A_truncated_file_is_an_error_and_not_a_panic()
    {
        // WoW has historically truncated SavedVariables on a hard exit.
        var refused = Lua.Parse("""X = { ["a"] = { ["b"] = 1,""");
        Assert.False(refused.IsOk);
        Assert.Contains("never closes", refused.Error.Message, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "a_pathologically_nested_file_is_refused_before_it_overflows_the_stack")]
    public void A_pathologically_nested_file_is_refused_before_it_overflows_the_stack()
    {
        var source = "X = " + new string('{', 500) + new string('}', 500);
        Assert.False(Lua.Parse(source).IsOk);
    }

    [Fact(DisplayName = "an_empty_file_has_no_globals_and_is_not_an_error")]
    public void An_empty_file_has_no_globals_and_is_not_an_error()
    {
        Assert.Empty(Parsed(""));
        Assert.Empty(Parsed("\n-- nothing here\n"));
    }

    [Fact(DisplayName = "several_globals_in_one_file_all_arrive")]
    public void Several_globals_in_one_file_all_arrive()
    {
        var globals = Parsed("A = { 1 }\nB = { 2 }\n");
        Assert.Equal(2, globals.Count);
        Assert.Contains("A", globals.Keys);
        Assert.Contains("B", globals.Keys);
    }

    [Fact(DisplayName = "a hex number and a long string both read")]
    public void A_hex_number_and_a_long_string_both_read()
    {
        var table = Parsed("X = { flags = 0x1F, text = [[two\nlines]] }")["X"];
        Assert.Equal(31, table.Get("flags")?.AsInteger());
        Assert.Equal("two\nlines", table.Get("text")?.AsStr());
    }

    [Fact(DisplayName = "an error says where in the file it gave up")]
    public void An_error_says_where_in_the_file_it_gave_up()
    {
        var refused = Lua.Parse("X = { 1, 2, os.x }");
        Assert.False(refused.IsOk);
        Assert.True(refused.Error.At > 8, refused.Error.ToString());
    }
}
