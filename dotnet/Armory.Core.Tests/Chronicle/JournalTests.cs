using System.Text;
using System.Text.Json.Nodes;
using Armory.Blizzard;
using Armory.Chronicle;
using Armory.Roster;
using Xunit;

namespace Armory.Tests.Chronicle;

/// <summary>Ported from <c>core/src/source/journal.rs</c>.</summary>
public sealed class JournalTests
{
    private static Digest ADigest() => new Session
    {
        Character = new CharacterKey("emerald-dream", "Somechar"),
        DisplayName = "Somechar",
        RealmName = "Emerald Dream",
        Class = "Druid",
        Race = "Tauren",
        Faction = Faction.Horde,
        StartedAt = new DateTimeOffset(2026, 8, 3, 19, 0, 0, TimeSpan.Zero),
        EndedAt = new DateTimeOffset(2026, 8, 3, 21, 0, 0, TimeSpan.Zero),
        StartLevel = 70,
        EndLevel = 71,
        StartMoney = 1_000_000,
        EndMoney = 1_250_000,
        StartItemLevel = 600,
        EndItemLevel = 604,
        Moments =
        [
            new Moment { At = 0, What = new Happening.Arrived("Nagrand", "Halaa", null) },
            new Moment { At = 10, What = new Happening.Accepted("Hero of the Mag'har", "Garrosh needs a champion.") },
            new Moment { At = 600, What = new Happening.Completed(9999, "Hero of the Mag'har", "The Mag'har will sing of this.") },
            new Moment { At = 600, What = new Happening.Paid(9999, 45_000, 1200) },
            new Moment { At = 900, What = new Happening.Fought("Durn the Hungerer", false) },
        ],
        Risen = [new Risen("The Severed Threads", 7)],
        Travelled = 41_288,
        LongestFight = 664,
    }.Digest();

    private static byte[] Bytes(string text) => Encoding.UTF8.GetBytes(text);

    [Fact(DisplayName = "the_request_goes_to_the_servers_completions_endpoint")]
    public void The_request_goes_to_the_servers_completions_endpoint()
    {
        var request = Journal.Write("http://127.0.0.1:8080", ADigest());
        Assert.Equal("http://127.0.0.1:8080/v1/chat/completions", request.Url);
        Assert.Equal("http://127.0.0.1:8080/v1/chat/completions", Journal.Write("http://127.0.0.1:8080/", ADigest()).Url);
        Assert.Contains(request.Headers, header => header.Name == "content-type" && header.Value == "application/json");
        Assert.DoesNotContain(request.Headers, header => header.Name.Equals("authorization", StringComparison.OrdinalIgnoreCase) || header.Name.Equals("x-api-key", StringComparison.OrdinalIgnoreCase));
    }

    [Fact(DisplayName = "the_model_is_asked_for_by_props_because_the_request_cannot_say")]
    public void The_model_is_asked_for_by_props_because_the_request_cannot_say()
    {
        Assert.Equal("http://127.0.0.1:8080/props", Journal.Identify("http://127.0.0.1:8080/").Url);
        Assert.Equal("qwen3-30b", Journal.ParseIdentity(Bytes("""{"model_alias":"qwen3-30b"}""")));
        Assert.Equal("Qwen3-30B-Q6_K", Journal.ParseIdentity(Bytes("""{"model_path":"/srv/models/Qwen3-30B-Q6_K.gguf"}""")));
        Assert.Null(Journal.ParseIdentity(Bytes("""{"something":"else"}""")));
        Assert.Null(Journal.ParseIdentity(Bytes("not json")));
    }

    [Fact(DisplayName = "the_request_body_is_json_and_asks_for_a_titled_entry")]
    public void The_request_body_is_json_and_asks_for_a_titled_entry()
    {
        var request = Journal.Write("k", ADigest());
        var body = JsonNode.Parse(request.Body!)!;
        Assert.Equal("json_schema", body["response_format"]!["type"]!.GetValue<string>());
        var required = body["response_format"]!["json_schema"]!["schema"]!["required"]!.AsArray().Select(node => node!.GetValue<string>()).ToList();
        Assert.Contains("title", required);
        Assert.Contains("entry", required);
        Assert.False(body["stream"]!.GetValue<bool>());
        Assert.Equal("system", body["messages"]![0]!["role"]!.GetValue<string>());
        Assert.Equal("user", body["messages"]![1]!["role"]!.GetValue<string>());
    }

    [Fact(DisplayName = "the_brief_carries_the_quest_text_the_game_put_on_screen")]
    public void The_brief_carries_the_quest_text_the_game_put_on_screen()
    {
        var brief = Journal.Brief(ADigest());
        Assert.Contains("Garrosh needs a champion.", brief, StringComparison.Ordinal);
        Assert.Contains("The Mag'har will sing of this.", brief, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "the_brief_says_a_wipe_was_a_wipe")]
    public void The_brief_says_a_wipe_was_a_wipe()
    {
        var brief = Journal.Brief(ADigest());
        Assert.Contains("Fought and lost to", brief, StringComparison.Ordinal);
        Assert.Contains("Durn the Hungerer", brief, StringComparison.Ordinal);
        Assert.DoesNotContain("Bosses defeated", brief, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "an_empty_section_is_left_out_rather_than_left_blank")]
    public void An_empty_section_is_left_out_rather_than_left_blank()
    {
        var brief = Journal.Brief(ADigest());
        Assert.DoesNotContain("Sold at auction", brief, StringComparison.Ordinal);
        Assert.DoesNotContain("Achievements earned", brief, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "the_brief_names_the_character_the_way_a_voice_needs")]
    public void The_brief_names_the_character_the_way_a_voice_needs()
    {
        var brief = Journal.Brief(ADigest());
        Assert.Contains("Somechar of Emerald Dream", brief, StringComparison.Ordinal);
        Assert.Contains("tauren druid of the Horde", brief, StringComparison.Ordinal);
        Assert.Contains("Monday 3 August 2026 — 19:00 UTC, lasting 2 hr", brief, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "a_written_entry_reads_out_of_the_first_choice")]
    public void A_written_entry_reads_out_of_the_first_choice()
    {
        var body = Bytes("""{"model":"qwen3-30b","choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"{\"title\":\"Halaa Again\",\"entry\":\"The wind off the plains.\"}"}}]}""");
        var written = Assert.IsType<Outcome<Written>.Found>(Journal.ParseWritten(body)).Value;
        Assert.Equal("Halaa Again", written.Title);
        Assert.Equal("The wind off the plains.", written.Body);
        Assert.Equal("qwen3-30b", written.Model);
    }

    [Fact(DisplayName = "a_thinking_block_in_the_content_is_cut_off_rather_than_choked_on")]
    public void A_thinking_block_in_the_content_is_cut_off_rather_than_choked_on()
    {
        var body = Bytes("""{"choices":[{"message":{"content":"<think>The player went to Halaa. I should write about that.</think>\n{\"title\":\"Halaa\",\"entry\":\"I went.\"}"}}]}""");
        var written = Assert.IsType<Outcome<Written>.Found>(Journal.ParseWritten(body)).Value;
        Assert.Equal("Halaa", written.Title);
        Assert.Equal("I went.", written.Body);
        Assert.Equal("a local model", written.Model);
    }

    [Fact(DisplayName = "an_entry_cut_off_by_the_token_budget_is_reported_rather_than_half_saved")]
    public void An_entry_cut_off_by_the_token_budget_is_reported_rather_than_half_saved()
    {
        Assert.IsType<Outcome<Written>.Stale>(Journal.ParseWritten(Bytes("""{"choices":[{"finish_reason":"length","message":{"content":"{\"title\":\"Hal"}}]}""")));
    }

    [Fact(DisplayName = "a_reply_in_a_shape_we_do_not_know_is_stale_and_never_empty")]
    public void A_reply_in_a_shape_we_do_not_know_is_stale_and_never_empty()
    {
        Assert.IsType<Outcome<Written>.Stale>(Journal.ParseWritten(Bytes("""{"choices":[]}""")));
        Assert.IsType<Outcome<Written>.Stale>(Journal.ParseWritten(Bytes("""{"choices":[{"message":{"content":"just prose"}}]}""")));
        Assert.IsType<Outcome<Written>.Stale>(Journal.ParseWritten(Bytes("<html>nope</html>")));
        Assert.IsType<Outcome<Written>.Empty>(Journal.ParseWritten(Bytes("""{"choices":[{"message":{"content":"{\"title\":\"x\",\"entry\":\"  \"}"}}]}""")));
    }

    [Fact(DisplayName = "an_error_body_is_read_for_what_it_actually_says")]
    public void An_error_body_is_read_for_what_it_actually_says()
    {
        Assert.Equal("the slot is not available", Journal.ParseError(Bytes("""{"error":{"code":500,"message":"the slot is not available","type":"server_error"}}""")));
        Assert.Null(Journal.ParseError(Bytes("not json")));
    }

    // -- Claude Code -------------------------------------------------------------
    //
    // No Rust counterpart: the GTK build writes through a llama-server only.
    // The result objects below were captured from `claude -p` 2.1.270 on
    // 2026-09-14, trimmed to the fields that are read.

    private static string After(Command command, string flag)
    {
        var at = command.Arguments.ToList().IndexOf(flag);
        Assert.True(at >= 0 && at + 1 < command.Arguments.Count, $"{flag} with a value");
        return command.Arguments[at + 1];
    }

    private const string Composed = """
        {"type":"result","subtype":"success","is_error":false,"duration_ms":9233,"num_turns":2,
         "result":"{\"title\":\"Iron in the Deeps\",\"entry\":\"Sun's gone from Dornogal.\"}",
         "session_id":"66faea4d","total_cost_usd":0.103127,
         "usage":{"input_tokens":2,"cache_creation_input_tokens":24418,"output_tokens":430},
         "modelUsage":{
           "claude-haiku-4-5-20251001":{"inputTokens":1046,"outputTokens":21,"canonicalModel":"claude-haiku-4-5"},
           "claude-sonnet-5":{"inputTokens":2,"outputTokens":430,"canonicalModel":"claude-sonnet-5"}},
         "structured_output":{"title":"Iron in the Deeps","entry":"Sun's gone from Dornogal."}}
        """;

    [Fact(DisplayName = "the CLI is asked in print mode, every tool off, the schema and the voice the server gets, and the brief on stdin")]
    public void The_cli_is_asked_for_the_same_entry_the_server_is()
    {
        var digest = ADigest();
        var command = Journal.Compose("opus", digest);
        Assert.Equal("claude", command.Program);
        Assert.Contains("-p", command.Arguments);
        Assert.Contains("--no-session-persistence", command.Arguments);
        Assert.Equal("json", After(command, "--output-format"));
        Assert.Equal("", After(command, "--tools"));
        Assert.Equal("", After(command, "--setting-sources"));
        Assert.Equal("opus", After(command, "--model"));
        // Bare mode skips the keychain, and with it the login this exists to use.
        Assert.DoesNotContain("--bare", command.Arguments);

        var schema = JsonNode.Parse(After(command, "--json-schema"))!;
        Assert.Equal(["title", "entry"], schema["required"]!.AsArray().Select(node => node!.GetValue<string>()));
        Assert.False(schema["additionalProperties"]!.GetValue<bool>());

        // Both backends are told the same thing about the evening.
        var server = JsonNode.Parse(Journal.Write("http://127.0.0.1:8080", digest).Body!)!;
        Assert.Equal(server["messages"]![0]!["content"]!.GetValue<string>(), After(command, "--system-prompt"));
        Assert.Equal(Journal.Brief(digest), command.Stdin);
        Assert.Equal(server["response_format"]!["json_schema"]!["schema"]!.ToJsonString(), schema.ToJsonString());
    }

    [Fact(DisplayName = "a blank model asks for the default rather than for nothing")]
    public void A_blank_model_asks_for_the_default()
    {
        Assert.Equal("sonnet", After(Journal.Compose("  ", ADigest()), "--model"));
    }

    [Fact(DisplayName = "a composed entry reads out of the structured output, named for the model that wrote most of it")]
    public void A_composed_entry_reads_out_of_the_structured_output()
    {
        var written = Assert.IsType<Outcome<Written>.Found>(Journal.ParseComposed(Bytes(Composed))).Value;
        Assert.Equal("Iron in the Deeps", written.Title);
        Assert.Equal("Sun's gone from Dornogal.", written.Body);
        // Not the helper model the CLI spends a few tokens on beside it.
        Assert.Equal("claude-sonnet-5", written.Model);
    }

    [Fact(DisplayName = "a run that failed before a model was asked is unusable and says why in the CLI's words")]
    public void A_failed_run_is_unusable_and_says_why()
    {
        var body = Bytes("""{"type":"result","subtype":"success","is_error":true,"num_turns":1,"result":"Not logged in · Please run /login","total_cost_usd":0,"modelUsage":{}}""");
        var unusable = Assert.IsType<Outcome<Written>.Unusable>(Journal.ParseComposed(body));
        Assert.Equal("Not logged in · Please run /login", unusable.Why.ToString());
    }

    [Fact(DisplayName = "a result with the JSON only in its text still reads, and one in no known shape is stale and never empty")]
    public void A_result_without_structured_output_falls_back_to_the_text()
    {
        var textOnly = Bytes("""{"type":"result","is_error":false,"result":"{\"title\":\"Halaa\",\"entry\":\"I went.\"}","modelUsage":{}}""");
        var written = Assert.IsType<Outcome<Written>.Found>(Journal.ParseComposed(textOnly)).Value;
        Assert.Equal("Halaa", written.Title);
        Assert.Equal("Claude Code", written.Model);

        Assert.IsType<Outcome<Written>.Stale>(Journal.ParseComposed(Bytes("""{"type":"result","is_error":false,"result":"just prose"}""")));
        Assert.IsType<Outcome<Written>.Stale>(Journal.ParseComposed(Bytes("""{"type":"result"}""")));
        Assert.IsType<Outcome<Written>.Stale>(Journal.ParseComposed(Bytes("Not logged in")));
        Assert.IsType<Outcome<Written>.Empty>(Journal.ParseComposed(Bytes("""{"is_error":false,"structured_output":{"title":"x","entry":"  "}}""")));
    }

    [Fact(DisplayName = "the sign-in status names the plan and the account, and nobody when nobody is signed in")]
    public void The_sign_in_status_names_the_plan_and_the_account()
    {
        Assert.Equal(["auth", "status"], Journal.AuthStatus().Arguments);
        Assert.Equal(
            "Claude Code · Max plan · somebody@example.org",
            Journal.ParseAuthStatus(Bytes("""{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty","email":"somebody@example.org","subscriptionType":"max"}""")));
        Assert.Equal("Claude Code", Journal.ParseAuthStatus(Bytes("""{"loggedIn":true}""")));
        Assert.Null(Journal.ParseAuthStatus(Bytes("""{"loggedIn":false}""")));
        Assert.Null(Journal.ParseAuthStatus(Bytes("Not logged in")));
    }
}
