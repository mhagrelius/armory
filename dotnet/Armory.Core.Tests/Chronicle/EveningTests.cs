using System.Text;
using Armory.Addon;
using Armory.Blizzard;
using Armory.Chronicle;
using Xunit;

namespace Armory.Tests.Chronicle;

/// <summary>
/// An evening, from the game's file to the digest, with nothing mocked. Ported
/// from <c>tests/chronicle.rs</c> at the repo root, all but the page test —
/// that one drives a GTK widget and has no counterpart here.
/// </summary>
/// <remarks>
/// Every step of the chronicle is unit tested where it lives. What no unit
/// test covers is the <em>join</em>: a file the addon wrote, parsed, filed,
/// read back and digested — and every one of those steps hands a slightly
/// different shape to the next. This drives the chain against a file in the
/// shape WoW actually writes, on disk in a directory of its own.
/// </remarks>
public sealed class EveningTests : IDisposable
{
    /// <summary>
    /// A per-character SavedVariables file with both of the addon's tables in
    /// it. Both, on purpose: the chronicle is a second saved variable of the
    /// same addon, so this is one file in practice and reading either half
    /// must not disturb the other.
    /// </summary>
    private const string Saved = """

        ArmoryCollectorCharDB = {
        	["format"] = 4,
        	["name"] = "Somechar",
        	["realm"] = "Emerald Dream",
        	["level"] = 71,
        	["class"] = "DRUID",
        	["race"] = "Tauren",
        	["faction"] = "Horde",
        	["quests"] = { 100, 200 },
        }
        ArmoryChronicleDB = {
        	["format"] = 1,
        	["sessions"] = {
        		{
        			["startedAt"] = 1785000000,
        			["endedAt"] = 1785009240,
        			["name"] = "Somechar",
        			["realm"] = "Emerald Dream",
        			["class"] = "DRUID",
        			["race"] = "Tauren",
        			["faction"] = "Horde",
        			["startLevel"] = 70,
        			["endLevel"] = 71,
        			["startMoney"] = 118204500,
        			["endMoney"] = 121950300,
        			["startItemLevel"] = 602.4,
        			["endItemLevel"] = 606.1,
        			["events"] = {
        				{ 0, "zone", "Orgrimmar", "The Drag", "" },
        				{ 420, "zone", "Nagrand", "Halaa", "" },
        				{ 460, "accepted", "Hero of the Mag'har", "Garrosh has not left his tent.", "" },
        				{ 1980, "quest", 9923, "Hero of the Mag'har", "You have given him back his father." },
        				{ 1980, "questpay", 9923, 84500, 12400 },
        				{ 2100, "level", 71, "Nagrand", "" },
        				{ 3600, "encounter", "Durn the Hungerer", 0, 14 },
        				{ 3900, "death", "Nagrand", "Halaa", "" },
        				{ 4500, "encounter", "Durn the Hungerer", 1, 14 },
        				{ 5400, "gained", "mount", "Talbuk Doe", "" },
        				{ 6000, "sale", "Auction successful: Mycobloom", 3745800, "" },
        				{ 7200, "with", "Velkurai", "", "" },
        			},
        		},
        	},
        }

        """;

    private readonly string directory = Directory.CreateTempSubdirectory("armory-evening-").FullName;

    public void Dispose() => Directory.Delete(directory, recursive: true);

    /// <summary>The evening, off a file on disk rather than a string in memory: the seam the addon writes through.</summary>
    private Session ReadOne()
    {
        var path = Path.Combine(directory, "Armory_Collector.lua");
        // No byte-order mark: the game writes none, and the reader takes the file as it comes.
        File.WriteAllText(path, Saved, new UTF8Encoding(encoderShouldEmitUTF8Identifier: false));
        var sessions = ChronicleReader.Read(File.ReadAllBytes(path));
        Assert.True(sessions.IsOk, "the file reads");
        return Assert.Single(sessions.Value);
    }

    [Fact(DisplayName = "an_evening_survives_the_file_the_store_and_the_digest")]
    public void An_evening_survives_the_file_the_store_and_the_digest()
    {
        var session = ReadOne();
        using var store = Armory.Store.Store.InMemory();
        Assert.Equal(1, store.SaveSessions([session]).Value);

        // Back out of SQLite, which is a JSON round trip through every variant.
        var stored = store.SessionsHeld(10).Value;
        var held = Assert.Single(stored);
        Assert.Equal(session, held);

        var digest = held.Digest();
        Assert.Equal("Somechar", digest.DisplayName);
        // Orgrimmar then Nagrand, in that order, with the subzone kept.
        Assert.Equal(["Orgrimmar", "Nagrand"], digest.Route.Select(stop => stop.Zone));
        Assert.Equal(["Halaa"], digest.Route[1].Within);
        // Wiped on and then killed, so it was killed.
        Assert.Equal(["Durn the Hungerer"], digest.Felled);
        Assert.Empty(digest.LostTo);
        Assert.Equal([(71, "Nagrand")], digest.Levels);
        Assert.Equal(3_745_800, digest.Purse);
        Assert.True(digest.IsWorthWriting());
    }

    [Fact(DisplayName = "the_brief_is_built_from_the_file_and_carries_the_games_own_words")]
    public void The_brief_is_built_from_the_file_and_carries_the_games_own_words()
    {
        // The quest text is the reason the addon records anything at all here.
        // If it stops reaching the brief, entries go back to being lists of
        // titles — and nothing else in the chain would fail.
        var brief = Journal.Brief(ReadOne().Digest());

        Assert.Contains("Garrosh has not left his tent.", brief, StringComparison.Ordinal);
        Assert.Contains("You have given him back his father.", brief, StringComparison.Ordinal);
        Assert.Contains("Hero of the Mag'har", brief, StringComparison.Ordinal);
        Assert.Contains("Somechar of Emerald Dream", brief, StringComparison.Ordinal);
        Assert.Contains("Talbuk Doe", brief, StringComparison.Ordinal);
        Assert.Contains("Velkurai", brief, StringComparison.Ordinal);
        // A wipe that ended in a kill is reported as the kill and nothing else,
        // so the model is not told to write about a defeat that was reversed.
        Assert.Contains("Bosses defeated", brief, StringComparison.Ordinal);
        Assert.DoesNotContain("Fought and lost to", brief, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "the_request_goes_to_the_local_server_with_no_credential")]
    public void The_request_goes_to_the_local_server_with_no_credential()
    {
        // The journal talks to a llama-server on this machine. Nothing leaves
        // it, and there is nothing that could.
        var request = Journal.Write("http://127.0.0.1:8080", ReadOne().Digest());

        Assert.Equal("http://127.0.0.1:8080/v1/chat/completions", request.Url);
        // Nothing to authenticate with, which is most of the point of pointing
        // it at a server on this machine.
        Assert.DoesNotContain(request.Headers, header =>
            string.Equals(header.Name, "authorization", StringComparison.OrdinalIgnoreCase)
            || string.Equals(header.Name, "x-api-key", StringComparison.OrdinalIgnoreCase));
    }

    [Fact(DisplayName = "an_entry_is_filed_against_the_evening_it_is_about")]
    public void An_entry_is_filed_against_the_evening_it_is_about()
    {
        var session = ReadOne();
        using var store = Armory.Store.Store.InMemory();
        Assert.True(store.SaveSessions([session]).IsOk);

        const string body = """
            {"model":"qwen3-30b","choices":[{"finish_reason":"stop","message":
            {"content":"{\"title\":\"What the Mag'har Sing\",\"entry\":\"I went to Halaa.\"}"}}]}
            """;
        var written = Assert.IsType<Outcome<Written>.Found>(Journal.ParseWritten(Encoding.UTF8.GetBytes(body))).Value;

        Assert.True(store.SaveEntry(new Entry
        {
            Session = session.Id,
            Title = written.Title,
            Body = written.Body,
            Model = written.Model,
            WrittenAt = DateTimeOffset.UtcNow,
        }).IsOk);

        var entries = store.EntriesHeld().Value;
        var entry = Assert.Single(entries).Value;
        Assert.Equal(entries[session.Id], entry);
        Assert.Equal("What the Mag'har Sing", entry.Title);
        Assert.Equal("qwen3-30b", entry.Model);
    }
}
