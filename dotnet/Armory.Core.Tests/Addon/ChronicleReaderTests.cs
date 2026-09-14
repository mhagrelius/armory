using Armory.Addon;
using Armory.Chronicle;
using Armory.Roster;
using Xunit;

namespace Armory.Tests.Addon;

/// <summary>Ported from <c>core/src/addon/chronicle.rs</c>.</summary>
public sealed class ChronicleReaderTests
{
    /// <summary>What the addon actually writes, in the shape WoW's serializer emits it.</summary>
    private const string Sample = """

        ArmoryChronicleDB = {
        	["format"] = 1,
        	["sessions"] = {
        		{
        			["startedAt"] = 1785000000,
        			["endedAt"] = 1785009000,
        			["name"] = "Somechar",
        			["realm"] = "Emerald Dream",
        			["class"] = "DRUID",
        			["race"] = "Tauren",
        			["faction"] = "Horde",
        			["startLevel"] = 70,
        			["endLevel"] = 71,
        			["startMoney"] = 1000000,
        			["endMoney"] = 1250000,
        			["startItemLevel"] = 600.4,
        			["endItemLevel"] = 604.2,
        			["events"] = {
        				{ 0, "zone", "Nagrand", "", 107 },
        				{ 12, "accepted", "Hero of the Mag'har", "Garrosh needs a champion.", "" },
        				{ 640, "quest", 9999, "Hero of the Mag'har", "The Mag'har will sing of this." },
        				{ 640, "questpay", 9999, 45000, 1200 },
        				{ 900, "level", 71, "Nagrand", "" },
        				{ 1200, "death", "Nagrand", "Halaa", "" },
        				{ 950, "worldtier", "Heroic", "", "" },
        				{ 960, "weather", "Rain", "Nagrand", 0.4 },
        				{ 1500, "encounter", "Durn the Hungerer", 0, 14 },
        				{ 1500, "wipe", "Durn the Hungerer", 31, "" },
        				{ 1800, "encounter", "Durn the Hungerer", 1, 14 },
        				{ 1900, "encounter", "Tarlna the Ageless", 0, 14 },
        				{ 1900, "wipe", "Tarlna the Ageless", 12, "" },
        				{ 1950, "encounter", "Tarlna the Ageless", 0, 14 },
        				{ 1950, "wipe", "Tarlna the Ageless", 4, "" },
        				{ 2000, "loot", 32458, "Ashes of Al'ar", 5 },
        				{ 2100, "gained", "mount", "Ashes of Al'ar", "" },
        				{ 2200, "achievement", 4956, "Loremaster of Kalimdor", "" },
        				{ 2300, "sale", "Auction successful: Mycobloom", 250000, "" },
        				{ 2300, "coin", "sale", 250000, 1 },
        				{ 2350, "coin", "repair", 9000, 0 },
        				{ 2360, "craft", 371637, "Flask of Alchemical Chaos", "" },
        				{ 2400, "flight", "Nagrand", "", "" },
        				{ 2500, "scenario", "The Sinkhole", "Tier 11", "" },
        				{ 640, "giver", "Garrosh Hellscream", 9999, 18166 },
        				{ 2600, "recipe", "Flask of Alchemical Chaos", "", "" },
        				{ 2700, "said", "Garrosh Hellscream", "The Mag'har will sing of this day.", "" },
        				{ 2800, "expired", "Auction expired: Mycobloom", "", "" },
        				{ 2850, "gossip", "Nisha", "The elements are restless in Nagrand.", "" },
        				{ 2900, "cutscene", "Nagrand", 872, "" },
        				{ 2950, "cutscene", "Nagrand", "", "" },
        				{ 2400, "with", "Velkurai", "", "" },
        			},
        			-- Session totals, written at logout rather than as events.
        			["kills"] = 214,
        			["travelled"] = 41288,
        			["longestFight"] = 664,
        			["worstHit"] = 812004,
        			["worstHitBy"] = "Durn the Hungerer",
        			["lowestHealth"] = 7,
        		},
        	},
        }

        """;

    private static List<Session> Read(string source) => ChronicleReader.Read(source).Value;

    [Fact(DisplayName = "a_session_reads_whole")]
    public void A_session_reads_whole()
    {
        var session = Assert.Single(Read(Sample));
        Assert.Equal("emerald-dream", session.Character.RealmSlug);
        Assert.Equal("Somechar", session.DisplayName);
        Assert.Equal("Druid", session.Class);
        Assert.Equal(Faction.Horde, session.Faction);
        Assert.Equal(70, session.StartLevel);
        Assert.Equal(71, session.EndLevel);
        Assert.Equal(600, session.StartItemLevel);
        Assert.Equal(150, (int)session.Duration.TotalMinutes);
    }

    [Fact(DisplayName = "every_kind_of_moment_survives_the_round_trip")]
    public void Every_kind_of_moment_survives_the_round_trip()
    {
        var digest = Read(Sample)[0].Digest();
        Assert.Equal("Nagrand", digest.Route[0].Zone);
        var quest = Assert.Single(digest.Quests);
        Assert.Equal(9999, quest.Id);
        Assert.Equal("Hero of the Mag'har", quest.Title);
        Assert.Equal("Garrosh needs a champion.", quest.Premise);
        Assert.Equal("The Mag'har will sing of this.", quest.Story);
        Assert.Equal(45_000, quest.Money);
        Assert.Equal([(71, "Nagrand")], digest.Levels);
        Assert.Equal("Halaa", digest.Deaths[0].Subzone);
        Assert.Equal(["Durn the Hungerer"], digest.Felled);
        Assert.Equal(["Tarlna the Ageless (down to 4%)"], digest.LostTo);
        Assert.Equal(["Heroic"], digest.WorldTiers);
        Assert.Equal(["Rain over Nagrand"], digest.Weather);
        Assert.Equal([(32458L, "Ashes of Al'ar", 5)], digest.Loot);
        Assert.Equal([(Acquisition.Mount, "Ashes of Al'ar")], digest.Acquired);
        Assert.Equal([(4956L, "Loremaster of Kalimdor")], digest.Achievements);
        Assert.Equal(250_000, digest.SaleIncome);
        Assert.Equal([(Purpose.Repair, 9_000L)], digest.Spending);
        Assert.Equal([("Flask of Alchemical Chaos", 1L)], digest.Crafted);
        Assert.Equal(1, digest.Flights);
        Assert.Equal(["The Sinkhole (Tier 11)"], digest.Scenarios);
        Assert.Equal(["Flask of Alchemical Chaos"], digest.Learned);
        Assert.Equal([("Garrosh Hellscream", 1L)], digest.Questgivers);
        Assert.Equal([("Garrosh Hellscream", "The Mag'har will sing of this day.")], digest.Overheard);
        Assert.Equal(["Auction expired: Mycobloom"], digest.Expired);
        Assert.Equal([("Nisha", "The elements are restless in Nagrand.")], digest.Told);
        Assert.Equal([("Nagrand", (long?)872), ("Nagrand", null)], digest.Cutscenes);
        Assert.Equal(41_288, digest.Travelled);
        Assert.Equal(664, digest.LongestFight);
        Assert.Equal(["Velkurai"], digest.Companions);
        Assert.Equal(250_000, digest.Purse);
    }

    [Fact(DisplayName = "an_empty_string_is_read_back_as_absent")]
    public void An_empty_string_is_read_back_as_absent()
    {
        Assert.Equal(new Happening.Arrived("Nagrand", null, 107), Read(Sample)[0].Moments[0].What);
    }

    [Fact(DisplayName = "an_event_kind_this_version_does_not_know_is_silence_rather_than_a_failure")]
    public void An_event_kind_this_version_does_not_know_is_silence_rather_than_a_failure()
    {
        var sessions = Read("""
            ArmoryChronicleDB = { ["format"] = 1, ["sessions"] = { {
                ["startedAt"] = 1785000000, ["name"] = "Somechar", ["realm"] = "Emerald Dream",
                ["events"] = {
                    { 0, "zone", "Durotar", "", "" },
                    { 5, "somethingnew", "x", "", "" },
                    { 9, "death", "Durotar", "", "" },
                } } } }
            """);
        Assert.Equal(2, sessions[0].Moments.Count);
    }

    [Fact(DisplayName = "a_session_that_never_closed_ends_at_its_last_event")]
    public void A_session_that_never_closed_ends_at_its_last_event()
    {
        var sessions = Read("""
            ArmoryChronicleDB = { ["format"] = 1, ["sessions"] = { {
                ["startedAt"] = 1785000000, ["name"] = "Somechar", ["realm"] = "Emerald Dream",
                ["events"] = { { 3600, "zone", "Durotar", "", "" } } } } }
            """);
        Assert.Equal(60, (int)sessions[0].Duration.TotalMinutes);
    }

    [Fact(DisplayName = "someone_elses_saved_variables_are_recognised_as_not_ours")]
    public void Someone_elses_saved_variables_are_recognised_as_not_ours()
    {
        Assert.IsType<ChronicleReadError.NotChronicleData>(ChronicleReader.Read("""TradeSkillMasterDB = { ["x"] = 1 }""").Error);
    }

    [Fact(DisplayName = "a_newer_addon_says_so_rather_than_reading_as_broken")]
    public void A_newer_addon_says_so_rather_than_reading_as_broken()
    {
        var error = ChronicleReader.Read("""ArmoryChronicleDB = { ["format"] = 99 }""").Error;
        Assert.Equal(new ChronicleReadError.FromTheFuture(99), error);
        Assert.Contains("update Armory", error.ToString(), StringComparison.Ordinal);
    }

    [Fact(DisplayName = "a_file_with_no_sessions_yet_is_valid_and_empty")]
    public void A_file_with_no_sessions_yet_is_valid_and_empty()
    {
        Assert.Empty(Read("""ArmoryChronicleDB = { ["format"] = 1 }"""));
    }

    [Fact(DisplayName = "a_truncated_file_is_reported_rather_than_half_read")]
    public void A_truncated_file_is_reported_rather_than_half_read()
    {
        Assert.IsType<ChronicleReadError.Unparsable>(ChronicleReader.Read("""ArmoryChronicleDB = { ["sessions"] = { {""").Error);
    }
}
