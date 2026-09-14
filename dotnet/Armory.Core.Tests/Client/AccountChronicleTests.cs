using System.Net;
using System.Text;
using System.Text.Json.Nodes;
using Armory.Addon;
using Armory.Blizzard;
using Armory.Chronicle;
using Armory.Client.Shell;
using Armory.Roster;
using Armory.Sharing;
using Armory.Tests.Support;
using Xunit;

namespace Armory.Tests.Client;

/// <summary>The chronicle half of the orchestrator: sessions off a dump into the store, and the journal written through a fake llama-server.</summary>
public sealed class AccountChronicleTests : IDisposable
{
    private static readonly CharacterKey Somechar = new("emerald-dream", "Somechar");
    private static readonly DateTimeOffset Evening = new(2026, 8, 3, 19, 0, 0, TimeSpan.Zero);

    private readonly string directory = Directory.CreateTempSubdirectory("armory-chronicle-").FullName;
    private readonly List<Account> accounts = [];
    private readonly Armory.Store.Store server = Armory.Store.Store.InMemory();

    public AccountChronicleTests()
    {
        server.SetMachine("server");
    }

    /// <summary>A llama-server in-process: /props names a model, completions answer as told.</summary>
    private sealed class Llama
    {
        public Func<HttpResponseMessage> Completion { get; set; } = () => Json(Reply("Halaa Again", "The wind came off the plains all evening."));

        public bool Answering { get; set; } = true;

        public int Asked { get; private set; }

        public HttpTests.Answering Handler => new(seen =>
        {
            if (seen.Url.EndsWith("/props", StringComparison.Ordinal))
            {
                return Answering ? Json("""{"model_alias":"qwen3-8b"}""") : new HttpResponseMessage(HttpStatusCode.ServiceUnavailable);
            }
            Asked++;
            return Completion();
        });

        public static HttpResponseMessage Json(string body) => new(HttpStatusCode.OK)
        {
            Content = new StringContent(body, Encoding.UTF8, "application/json"),
        };

        public static string Reply(string title, string entry)
        {
            var content = new JsonObject { ["title"] = title, ["entry"] = entry }.ToJsonString();
            return new JsonObject
            {
                ["model"] = "armory-chronicle",
                ["choices"] = new JsonArray(new JsonObject
                {
                    ["finish_reason"] = "stop",
                    ["message"] = new JsonObject { ["role"] = "assistant", ["content"] = content },
                }),
            }.ToJsonString();
        }
    }

    /// <summary>The claude command-line in-process: auth status says who is signed in, and a composition answers as told.</summary>
    private sealed class Claude
    {
        public bool SignedIn { get; set; } = true;

        public Func<Command, Result<byte[], Reason>> Composition { get; set; } = _ => Result<byte[], Reason>.Ok(Composed("Halaa Again", "The wind came off the plains all evening."));

        public List<Command> Ran { get; } = [];

        public int Compositions => Ran.Count(command => command.Arguments.Contains("-p"));

        public Task<Result<byte[], Reason>> Run(Command command)
        {
            Ran.Add(command);
            if (command.Arguments.SequenceEqual(["auth", "status"]))
            {
                return Task.FromResult(Result<byte[], Reason>.Ok(Encoding.UTF8.GetBytes(SignedIn
                    ? """{"loggedIn":true,"email":"somebody@example.org","subscriptionType":"max"}"""
                    : """{"loggedIn":false}""")));
            }
            return Task.FromResult(Composition(command));
        }

        public static byte[] Composed(string title, string entry) => Encoding.UTF8.GetBytes(new JsonObject
        {
            ["type"] = "result",
            ["is_error"] = false,
            ["result"] = new JsonObject { ["title"] = title, ["entry"] = entry }.ToJsonString(),
            ["modelUsage"] = new JsonObject
            {
                ["claude-haiku-4-5-20251001"] = new JsonObject { ["outputTokens"] = 21, ["canonicalModel"] = "claude-haiku-4-5" },
                ["claude-sonnet-5"] = new JsonObject { ["outputTokens"] = 430, ["canonicalModel"] = "claude-sonnet-5" },
            },
            ["structured_output"] = new JsonObject { ["title"] = title, ["entry"] = entry },
        }.ToJsonString());
    }

    private Account Make(string machine, Llama llama, bool automatic = true, Claude? claude = null)
    {
        var store = Armory.Store.Store.InMemory();
        store.SetMachine(machine);
        var settings = Path.Combine(directory, machine, "settings.json");
        new Armory.Settings.Settings
        {
            JournalAutomatic = automatic,
            JournalServer = "http://journal.test",
            JournalBackend = claude is null ? Armory.Settings.JournalBackend.LlamaServer : Armory.Settings.JournalBackend.ClaudeCode,
        }.Save(settings);
        var account = new Account(
            new StoreWorker(store),
            new MemorySecrets(),
            settings,
            work => work().GetAwaiter().GetResult(),
            (_, _, who, _) => Result<IRemote, SyncError>.Ok(new InProcessRemote(server, who)),
            handler: llama.Handler,
            run: (claude ?? new Claude { SignedIn = false }).Run);
        accounts.Add(account);
        return account;
    }

    private static Session AnEvening(int hoursAgo = 0, params string[] quests) => new()
    {
        Character = Somechar,
        DisplayName = "Somechar",
        RealmName = "Emerald Dream",
        Class = "Druid",
        Race = "Tauren",
        Faction = Faction.Horde,
        StartedAt = Evening.AddHours(-hoursAgo),
        EndedAt = Evening.AddHours(-hoursAgo + 2),
        StartLevel = 70,
        EndLevel = 70,
        Moments =
        [
            new Moment { At = 0, What = new Happening.Arrived("Nagrand", "Halaa", null) },
            .. quests.Select((title, index) => new Moment { At = 60 * (index + 1), What = new Happening.Completed(index + 1, title, null) }),
        ],
    };

    private static Dump ADump(params Session[] sessions) => new() { Collected = new Collected(), Sessions = [.. sessions] };

    [Fact(DisplayName = "sessions off a dump land in the store, once, and the toast says how many")]
    public async Task Sessions_off_a_dump_land_in_the_store_once()
    {
        var account = Make("one", new Llama { Answering = false }, automatic: false);
        var toasts = new List<string>();
        account.Toasted += toasts.Add;

        await account.Collected(Result<Dump, ReadError>.Ok(ADump(AnEvening(0, "Hero of the Mag'har"), AnEvening(24))));
        Assert.Contains("2 new evenings in the Chronicle", toasts);
        Assert.Equal(2, (await account.Sessions()).Count);

        // The addon rewrites its whole file at every logout; the same
        // evenings again are not new.
        toasts.Clear();
        await account.Collected(Result<Dump, ReadError>.Ok(ADump(AnEvening(0, "Hero of the Mag'har"), AnEvening(24), AnEvening(48))));
        Assert.Contains("One new evening in the Chronicle", toasts);
        Assert.Equal(3, (await account.Sessions()).Count);
        // Newest first.
        Assert.Equal(Evening, (await account.Sessions())[0].StartedAt);
    }

    [Fact(DisplayName = "the journal is identified from /props and an entry is written and kept")]
    public async Task The_journal_is_identified_and_an_entry_written()
    {
        var llama = new Llama();
        var account = Make("one", llama, automatic: false);
        Assert.False(account.JournalReady);

        Assert.Equal("qwen3-8b", await account.IdentifyJournal());
        Assert.True(account.JournalReady);
        Assert.Equal("qwen3-8b", account.JournalModel);

        var evening = AnEvening(0, "Hero of the Mag'har");
        await account.Collected(Result<Dump, ReadError>.Ok(ADump(evening)));
        Assert.Empty(await account.Entries());

        await account.WriteEntry(evening.Id);
        var entry = Assert.Single(await account.Entries()).Value;
        Assert.Equal("Halaa Again", entry.Title);
        Assert.Equal("The wind came off the plains all evening.", entry.Body);
        // What /props said, not what the completion echoed back.
        Assert.Equal("qwen3-8b", entry.Model);
        Assert.False(account.IsWriting(evening.Id));
        Assert.Equal(1, llama.Asked);
    }

    [Fact(DisplayName = "automatic writing obeys the setting")]
    public async Task Automatic_writing_obeys_the_setting()
    {
        var llama = new Llama();
        var automatic = Make("auto", llama);
        await automatic.IdentifyJournal();
        await automatic.Collected(Result<Dump, ReadError>.Ok(ADump(AnEvening(0, "Hero of the Mag'har"))));
        Assert.Single(await automatic.Entries());

        var byHand = Make("hand", llama, automatic: false);
        await byHand.IdentifyJournal();
        await byHand.Collected(Result<Dump, ReadError>.Ok(ADump(AnEvening(0, "Hero of the Mag'har"))));
        Assert.Empty(await byHand.Entries());
    }

    [Fact(DisplayName = "an evening not worth writing is not queued, and a written one is not written twice")]
    public async Task Only_evenings_worth_writing_are_queued_once()
    {
        var llama = new Llama();
        var account = Make("one", llama, automatic: false);
        var toasts = new List<string>();
        account.Toasted += toasts.Add;
        await account.IdentifyJournal();
        // A quiet ten minutes in one zone, and a real evening.
        var quiet = AnEvening(48) with { EndedAt = Evening.AddHours(-48).AddMinutes(10) };
        var real = AnEvening(0, "Hero of the Mag'har");
        await account.Collected(Result<Dump, ReadError>.Ok(ADump(quiet, real)));

        Assert.True(await account.WriteAll());
        Assert.Contains("Writing 1 entries, one at a time", toasts);
        Assert.Equal(1, llama.Asked);
        Assert.Single(await account.Entries());

        toasts.Clear();
        Assert.True(await account.WriteAll());
        Assert.Contains("Every evening is already written up", toasts);
        Assert.Equal(1, llama.Asked);
    }

    [Fact(DisplayName = "a failing server abandons the queue after one attempt")]
    public async Task A_failing_server_abandons_the_queue()
    {
        var llama = new Llama
        {
            Completion = () => new HttpResponseMessage(HttpStatusCode.InternalServerError)
            {
                Content = new StringContent("""{"error":{"message":"model not loaded"}}""", Encoding.UTF8, "application/json"),
            },
        };
        var account = Make("one", llama, automatic: false);
        var toasts = new List<string>();
        account.Toasted += toasts.Add;
        await account.IdentifyJournal();
        await account.Collected(Result<Dump, ReadError>.Ok(ADump(AnEvening(0, "A"), AnEvening(24, "B"), AnEvening(48, "C"))));

        await account.WriteAll();
        // One request, one toast naming the reason, and the rest left alone.
        Assert.Equal(1, llama.Asked);
        Assert.Contains(toasts, toast => toast.StartsWith("No entry written:", StringComparison.Ordinal) && toast.Contains("model not loaded", StringComparison.Ordinal));
        Assert.Contains("Stopped — 2 evenings left unwritten", toasts);
        Assert.Equal(0, account.QueuedEntries);
        Assert.Empty(await account.Entries());
    }

    [Fact(DisplayName = "with no model answering, writing everything says so and writes nothing")]
    public async Task With_no_model_answering_nothing_is_written()
    {
        var llama = new Llama { Answering = false };
        var account = Make("one", llama, automatic: true);
        var toasts = new List<string>();
        account.Toasted += toasts.Add;
        Assert.Null(await account.IdentifyJournal());
        await account.Collected(Result<Dump, ReadError>.Ok(ADump(AnEvening(0, "A"))));
        Assert.False(await account.WriteAll());
        Assert.Contains(toasts, toast => toast.StartsWith("No model is answering", StringComparison.Ordinal));
        Assert.Equal(0, llama.Asked);
    }

    [Fact(DisplayName = "a think block in the reply is cut off before the entry is read")]
    public async Task A_think_block_is_cut_off()
    {
        var llama = new Llama
        {
            Completion = () => Llama.Json(Llama.Reply("x", "y").Replace("{\\\"title\\\"", "<think>hmm</think>{\\\"title\\\"", StringComparison.Ordinal)),
        };
        var account = Make("one", llama, automatic: false);
        await account.IdentifyJournal();
        var evening = AnEvening(0, "A");
        await account.Collected(Result<Dump, ReadError>.Ok(ADump(evening)));
        await account.WriteEntry(evening.Id);
        Assert.Equal("x", Assert.Single(await account.Entries()).Value.Title);
    }

    [Fact(DisplayName = "a forgotten evening stays forgotten through the next read")]
    public async Task A_forgotten_evening_stays_forgotten()
    {
        var llama = new Llama();
        var account = Make("one", llama, automatic: false);
        await account.IdentifyJournal();
        var evening = AnEvening(0, "A");
        await account.Collected(Result<Dump, ReadError>.Ok(ADump(evening)));
        await account.WriteEntry(evening.Id);
        Assert.Single(await account.Entries());

        await account.ForgetSession(evening.Id);
        Assert.Empty(await account.Sessions());
        Assert.Empty(await account.Entries());
        await account.Collected(Result<Dump, ReadError>.Ok(ADump(evening)));
        Assert.Empty(await account.Sessions());
    }

    [Fact(DisplayName = "saving the journal settings re-identifies the server")]
    public async Task Saving_the_journal_settings_reidentifies()
    {
        var llama = new Llama();
        var account = Make("one", llama, automatic: true);
        await account.SaveJournalSettings("  ", automatic: false);
        Assert.Equal(Journal.DefaultServer, account.Settings.JournalServer);
        Assert.False(account.Settings.JournalAutomatic);
        Assert.True(account.JournalReady);
    }

    // -- Claude Code -------------------------------------------------------------

    [Fact(DisplayName = "with Claude Code chosen, the journal is identified from its sign-in and an entry is written, kept, and named for the model that wrote it")]
    public async Task With_claude_code_an_entry_is_written_through_the_cli()
    {
        var llama = new Llama();
        var claude = new Claude();
        var account = Make("one", llama, automatic: false, claude);
        Assert.False(account.JournalReady);

        Assert.Equal("Claude Code · Max plan · somebody@example.org", await account.IdentifyJournal());
        Assert.True(account.JournalReady);

        var evening = AnEvening(0, "Hero of the Mag'har");
        await account.Collected(Result<Dump, ReadError>.Ok(ADump(evening)));
        await account.WriteEntry(evening.Id);

        var entry = Assert.Single(await account.Entries()).Value;
        Assert.Equal("Halaa Again", entry.Title);
        Assert.Equal("The wind came off the plains all evening.", entry.Body);
        // The model that wrote, not the sign-in that answered the check.
        Assert.Equal("claude-sonnet-5", entry.Model);
        Assert.False(account.IsWriting(evening.Id));

        // One composition, carrying this evening's brief, and the server never asked.
        var composed = Assert.Single(claude.Ran, command => command.Arguments.Contains("-p"));
        Assert.Contains("Hero of the Mag'har", composed.Stdin, StringComparison.Ordinal);
        Assert.Equal(0, llama.Asked);
    }

    [Fact(DisplayName = "with nobody signed in to Claude Code, writing everything says so and runs nothing")]
    public async Task With_nobody_signed_in_nothing_is_written()
    {
        var claude = new Claude { SignedIn = false };
        var account = Make("one", new Llama(), automatic: true, claude);
        var toasts = new List<string>();
        account.Toasted += toasts.Add;
        Assert.Null(await account.IdentifyJournal());
        await account.Collected(Result<Dump, ReadError>.Ok(ADump(AnEvening(0, "A"))));
        Assert.False(await account.WriteAll());
        Assert.Contains(toasts, toast => toast.StartsWith("Claude Code is not signed in", StringComparison.Ordinal));
        Assert.Equal(0, claude.Compositions);
    }

    [Fact(DisplayName = "a CLI that cannot run abandons the queue after one attempt, in the seam's words")]
    public async Task A_cli_that_cannot_run_abandons_the_queue()
    {
        var claude = new Claude { Composition = _ => Result<byte[], Reason>.Err(new Reason.NotConfigured("claude is not installed, or is not on PATH for this application")) };
        var account = Make("one", new Llama(), automatic: false, claude);
        var toasts = new List<string>();
        account.Toasted += toasts.Add;
        await account.IdentifyJournal();
        await account.Collected(Result<Dump, ReadError>.Ok(ADump(AnEvening(0, "A"), AnEvening(24, "B"), AnEvening(48, "C"))));

        await account.WriteAll();
        Assert.Equal(1, claude.Compositions);
        Assert.Contains("No entry written: not set up: claude is not installed, or is not on PATH for this application", toasts);
        Assert.Contains("Stopped — 2 evenings left unwritten", toasts);
        Assert.Empty(await account.Entries());
    }

    [Fact(DisplayName = "a run the CLI reports as an error is not an entry, and the queue stops with its words")]
    public async Task A_run_reported_as_an_error_is_not_an_entry()
    {
        var claude = new Claude
        {
            Composition = _ => Result<byte[], Reason>.Ok(Encoding.UTF8.GetBytes("""{"type":"result","is_error":true,"result":"Not logged in · Please run /login"}""")),
        };
        var account = Make("one", new Llama(), automatic: false, claude);
        var toasts = new List<string>();
        account.Toasted += toasts.Add;
        await account.IdentifyJournal();
        var evening = AnEvening(0, "A");
        await account.Collected(Result<Dump, ReadError>.Ok(ADump(evening)));
        await account.WriteEntry(evening.Id);
        Assert.Contains("No entry written: Not logged in · Please run /login", toasts);
        Assert.Empty(await account.Entries());
    }

    [Fact(DisplayName = "saving the journal settings can change the writer, and re-identifies through the new one")]
    public async Task Saving_the_journal_settings_can_change_the_writer()
    {
        var llama = new Llama();
        var claude = new Claude();
        var account = Make("one", llama, automatic: true, claude);
        await account.SaveJournalSettings("  ", automatic: false, Armory.Settings.JournalBackend.LlamaServer, "  ");
        Assert.Equal(Armory.Settings.JournalBackend.LlamaServer, account.Settings.JournalBackend);
        Assert.Equal("sonnet", account.Settings.JournalModel);
        Assert.Equal("qwen3-8b", account.JournalModel);

        await account.SaveJournalSettings("http://journal.test", automatic: false, Armory.Settings.JournalBackend.ClaudeCode, "opus");
        Assert.Equal("opus", account.Settings.JournalModel);
        Assert.StartsWith("Claude Code", account.JournalModel, StringComparison.Ordinal);
        // And the saved file says so, in the Rust's code.
        Assert.Equal(Armory.Settings.JournalBackend.ClaudeCode, Armory.Settings.Settings.Load(Path.Combine(directory, "one", "settings.json")).JournalBackend);

        var evening = AnEvening(0, "A");
        await account.Collected(Result<Dump, ReadError>.Ok(ADump(evening)));
        await account.WriteEntry(evening.Id);
        var arguments = Assert.Single(claude.Ran, command => command.Arguments.Contains("-p")).Arguments.ToList();
        Assert.Equal("opus", arguments[arguments.IndexOf("--model") + 1]);
    }

    public void Dispose()
    {
        foreach (var account in accounts)
        {
            account.Dispose();
        }
        server.Dispose();
        try
        {
            Directory.Delete(directory, recursive: true);
        }
        catch (IOException)
        {
        }
    }
}
