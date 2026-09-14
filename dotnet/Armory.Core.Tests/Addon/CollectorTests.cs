using Armory.Addon;
using Armory.Collections;
using Armory.Provenance;
using Armory.Roster;
using Armory.Run;
using Armory.Tally;
using Xunit;

namespace Armory.Tests.Addon;

/// <summary>Ported from <c>core/src/addon/collector.rs</c>, sample files included.</summary>
public sealed class CollectorTests
{
    private const string Sample = """

        ArmoryCollectorDB = {
        	["format"] = 2,
        	["writtenAt"] = 1785000000,
        	["achievements"] = {
        		[4956] = "Aeltor-Mannoroth",
        		[1234] = "Somechar-Emerald Dream",
        	},
        	["completed"] = {
        		[4956] = 1457000000,
        		[1234] = true,
        	},
        	["tree"] = {
        		[4956] = { 12345, 12346 },
        	},
        	["criteria"] = {
        		[12345] = { 27, 5000 },
        		[12346] = { 46, 2170 },
        		[12347] = { 119, 9 },
        	},
        	["names"] = {
        		[4956] = { "Loremaster of Kalimdor", 50, "Quests", "Complete the Kalimdor quest achievements.", "", 236443 },
        		[1234] = { "Gone Forever", 10, "Feats of Strength", "", "", 0 },
        	},
        	["currencies"] = {
        		["Somechar-Emerald Dream"] = { [2245] = 4200 },
        	},
        	["warbandBank"] = { [190456] = 40 },
        	["recipes"] = {
        		["Somechar-Emerald Dream"] = {
        			[371637] = {
        				"Flask of Alchemical Chaos",
        				191318,
        				1,
        				{
        					{ 3, { 210796, 210797, 210798 } },
        					{ 1, { 212263 } },
        				},
        			},
        		},
        	},
        	["tally"] = {
        		["Somechar-Emerald Dream"] = {
        			["recipe"] = {
        				[371637] = { 412, "Flask of Alchemical Chaos" },
        				[370582] = { 6, "Algari Mana Potion" },
        			},
        			["companion"] = {
        				["Velkurai"] = { 34, "Velkurai" },
        			},
        			["zone"] = {
        				["Nagrand"] = { 68400, "Nagrand" },
        			},
        			["nonsense"] = {
        				["x"] = { 1, "x" },
        			},
        		},
        	},
        	["earned"] = {
        		["Somechar-Emerald Dream"] = {
        			["rep"] = {
        				[2170] = { 21000, 4, 25, 1 },
        			},
        			["currency"] = {
        				[3008] = { 1000, 600, 1, 1, 1 },
        				[2245] = { 4200, 0, 0, 0, 0 },
        			},
        		},
        	},
        	["mounts"] = {
        		[6] = { "Brown Horse", 1, "Vendor: Katie Hunter, Elwynn Forest", 2 },
        		[7] = { "Ashes of Al'ar", 0, "Drop: Kael'thas Sunstrider, Tempest Keep", 1 },
        	},
        	["toys"] = {
        		[128471] = { "Sturdy Love Fool", 0, "", 0 },
        	},
        	["decor"] = {
        		[1234] = { "Sturdy Oak Chair", 1, "Vendor: Furnisher, Stormwind", 0, 240001, "", 5321, 0, 2, -1, "", 6 },
        		[1235] = { "Gilded Lantern", 0, "Drop: Rasha'nan, Nerub-ar Palace", 0, 240002, "", 5322, 0, 3, -1, "", 0 },
        	},
        	["flavour"] = { 1, "12.1.0", 69814, 120100 },
        }

        """;

    private const string CharacterFile = """

        ArmoryCollectorCharDB = {
        	["format"] = 2,
        	["name"] = "Somechar",
        	["realm"] = "Emerald Dream",
        	["level"] = 80,
        	["money"] = 91234567,
        	["class"] = "DRUID",
        	["race"] = "Tauren",
        	["faction"] = "Horde",
        	["spec"] = "Restoration",
        	["guild"] = "Dream Team",
        	["itemLevel"] = 642.4,
        	["scannedAt"] = 1785000000,
        	["quests"] = { 100, 200, 300 },
        	["professions"] = {
        		{ "Alchemy", 84, 100, 1, { { "Potion Mastery", 1 }, { "Phial Mastery", 0 } }, 412 },
        		{ "Cooking", 40, 100, 0 },
        	},
        	["equipment"] = {
        		{ "HEAD", "Helm of the Broken", 639 },
        		{ "OFF_HAND", "Bulwark of the Kurenai", 612 },
        		{ "TABARD", "Tabard of the Kurenai", "" },
        	},
        	["raidLocks"] = {
        		{ "Liberation of Undermine", "Heroic", 2, 8, 15 },
        	},
        	["vault"] = {
        		{ 1, 1, 1, 1, 12, "" },
        		{ 1, 2, 4, 3, 10, "" },
        		{ 3, 1, 2, 2, 15, "Heroic" },
        		{ 6, 1, 2, 0, 8, "" },
        	},
        	["vaultReady"] = 1,
        	["flavour"] = { 1, "12.1.0", 69814, 120100 },
        }

        """;

    private static readonly CharacterKey Somechar = new("emerald-dream", "Somechar");

    private static Collected Read() => Collector.Read(Sample).Value;

    private static CollectedCharacter ReadCharacter() => Collector.ReadCharacter(CharacterFile).Value;

    [Fact(DisplayName = "attribution_is_still_what_the_addon_is_for")]
    public void Attribution_is_still_what_the_addon_is_for()
    {
        Assert.Equal(new CharacterKey("mannoroth", "Aeltor"), Read().EarnedBy[4956]);
    }

    [Fact(DisplayName = "the_addon_supplies_the_achievement_list_a_run_is_planned_from")]
    public void The_addon_supplies_the_achievement_list_a_run_is_planned_from()
    {
        var progress = Read().Progress();
        Assert.Equal(2, progress.Count);
        var withTree = progress.Single(entry => entry.Id == 4956);
        var criteria = withTree.Criteria!;
        Assert.Equal(2, criteria.Children.Count);
        Assert.Equal(CriterionKind.Quest(5000), criteria.Children[0].Kind);
        Assert.Equal(CriterionKind.Reputation(2170), criteria.Children[1].Kind);
    }

    [Fact(DisplayName = "the_addon_names_the_achievements_so_the_interface_need_not_say_a_number")]
    public void The_addon_names_the_achievements_so_the_interface_need_not_say_a_number()
    {
        var loremaster = Read().Catalogue[4956];
        Assert.Equal("Loremaster of Kalimdor", loremaster.Name);
        Assert.Equal(50, loremaster.Points);
        Assert.Equal("Quests", loremaster.Category);
        Assert.StartsWith("Complete the Kalimdor", loremaster.Description, StringComparison.Ordinal);
        Assert.False(loremaster.IsUnrepeatable);
    }

    [Fact(DisplayName = "a_recipe_book_carries_every_quality_of_every_reagent")]
    public void A_recipe_book_carries_every_quality_of_every_reagent()
    {
        var book = Read().Recipes[Somechar];
        var flask = Assert.Single(book);
        Assert.Equal("Flask of Alchemical Chaos", flask.Name);
        Assert.Equal(191_318, flask.Output);
        Assert.Equal(1, flask.Makes);
        Assert.Equal(3, flask.Reagents[0].Quantity);
        Assert.Equal([210_796L, 210_797L, 210_798L], flask.Reagents[0].Tiers);
        Assert.Equal([212_263L], flask.Reagents[1].Tiers);
    }

    [Fact(DisplayName = "the_counters_nothing_else_keeps_come_off_one_table")]
    public void The_counters_nothing_else_keeps_come_off_one_table()
    {
        var mine = Read().Tallies[Somechar];
        var made = Counters.Of(mine, Counting.Recipe);
        Assert.Equal("Flask of Alchemical Chaos", made[0].Label);
        Assert.Equal(412, made[0].Count);
        // Keyed by a spell id, which Lua hands back as a number.
        Assert.Equal("371637", made[0].Key);
        Assert.Equal(6, made[1].Count);
        Assert.Equal(34, Counters.Of(mine, Counting.Companion)[0].Count);
        Assert.Equal("Nagrand", Counters.Of(mine, Counting.Zone)[0].Label);
        // A kind this version does not know contributed nothing.
        Assert.Equal(4, mine.Count);
    }

    [Fact(DisplayName = "what_a_character_earned_is_read_apart_from_what_the_account_holds")]
    public void What_a_character_earned_is_read_apart_from_what_the_account_holds()
    {
        var earned = Read().Earned[Somechar];
        var with = earned.With(2170);
        Assert.Equal(21_000, with.Points);
        Assert.Equal(4, with.Renown);
        Assert.Equal(25, with.RenownSeen);
        Assert.True(with.AccountWide);
        Assert.Equal(Origin.Transferred, earned.Currency[3008].Origin);
        Assert.Equal(Origin.Earned, earned.Currency[2245].Origin);
    }

    [Fact(DisplayName = "a_feat_of_strength_is_recognised_by_its_category")]
    public void A_feat_of_strength_is_recognised_by_its_category()
    {
        Assert.True(Read().Catalogue[1234].IsUnrepeatable);
    }

    [Fact(DisplayName = "an_achievement_with_no_date_still_counts_as_earned_long_ago")]
    public void An_achievement_with_no_date_still_counts_as_earned_long_ago()
    {
        var collected = Read();
        Assert.Equal(DateTimeOffset.UnixEpoch, collected.Completed[1234]);
        Assert.True(collected.Completed[4956] > DateTimeOffset.UnixEpoch);
    }

    [Fact(DisplayName = "collections_carry_a_sentence_rather_than_one_word")]
    public void Collections_carry_a_sentence_rather_than_one_word()
    {
        var collected = Read();
        var ashes = collected.Collectibles.Single(entry => entry.Id == 7);
        Assert.Equal("Ashes of Al'ar", ashes.Name);
        Assert.Equal(Source.Drop, ashes.Source);
        Assert.DoesNotContain((Kind.Mount, 7L), collected.Owned);
        Assert.Contains((Kind.Mount, 6L), collected.Owned);
    }

    [Fact(DisplayName = "the_journals_markup_is_stripped_to_the_sentence_underneath")]
    public void The_journals_markup_is_stripped_to_the_sentence_underneath()
    {
        Assert.Equal(
            "Drop: Lord Aurius Rivendare\nLocation: Stratholme",
            Collector.StripMarkup("|cFFFFD200Drop:|r Lord Aurius Rivendare|n|cFFFFD200Location:|r Stratholme"));
        Assert.Equal("Legacy", Collector.StripMarkup("|cFFFFD200Legacy|r"));
    }

    [Fact(DisplayName = "an_inline_texture_leaves_nothing_behind")]
    public void An_inline_texture_leaves_nothing_behind()
    {
        Assert.Equal(
            "Vendor: Harb Clawhoof\nCost: 1",
            Collector.StripMarkup("|cFFFFD200Vendor: |rHarb Clawhoof|n|cFFFFD200Cost: |r1|TINTERFACE\\MONEYFRAME\\UI-GOLDICON.BLP:0|t"));
    }

    [Fact(DisplayName = "a_hyperlink_keeps_its_text_and_drops_its_payload")]
    public void A_hyperlink_keeps_its_text_and_drops_its_payload()
    {
        Assert.Equal("Quest: The Battle for Gilneas", Collector.StripMarkup("Quest: |Hquest:12345|hThe Battle for Gilneas|h"));
    }

    [Fact(DisplayName = "a_literal_pipe_survives")]
    public void A_literal_pipe_survives()
    {
        Assert.Equal("a | b", Collector.StripMarkup("a || b"));
    }

    [Fact(DisplayName = "a_collection_written_as_a_sparse_array_reads_at_the_right_ids")]
    public void A_collection_written_as_a_sparse_array_reads_at_the_right_ids()
    {
        var collected = Collector.Read("""
            ArmoryCollectorDB = { ["format"] = 2, ["mounts"] = {
                nil, nil,
                { "Third Mount", 1, "|cFFFFD200Vendor:|r Someone" },
                [382] = { "Far Mount", 0, "" },
            } }
            """).Value;
        Assert.Equal(2, collected.Collectibles.Count);
        var third = collected.Collectibles.Single(entry => entry.Id == 3);
        Assert.Equal("Third Mount", third.Name);
        Assert.Equal("Vendor: Someone", third.Description);
        Assert.Contains((Kind.Mount, 3L), collected.Owned);
        Assert.Contains(collected.Collectibles, entry => entry.Id == 382 && entry.Name == "Far Mount");
    }

    [Fact(DisplayName = "a_source_with_no_text_is_unknown_rather_than_guessed")]
    public void A_source_with_no_text_is_unknown_rather_than_guessed()
    {
        Assert.Equal(Source.Unknown, Read().Collectibles.Single(entry => entry.Kind == Kind.Toy).Source);
    }

    [Fact(DisplayName = "a_character_file_is_a_whole_roster_row")]
    public void A_character_file_is_a_whole_roster_row()
    {
        var read = ReadCharacter();
        Assert.Equal("Somechar", read.Character.DisplayName);
        Assert.Equal("emerald-dream", read.Character.Key.RealmSlug);
        Assert.Equal(80, read.Character.Level);
        Assert.Equal("Druid", read.Character.Class);
        Assert.Equal(Faction.Horde, read.Character.Faction);
        Assert.Equal(642, read.Detail.ItemLevel);
        Assert.Equal("Restoration", read.Detail.Spec);
        Assert.Equal(91_234_567, read.Detail.Money);
        Assert.Equal(3, read.Quests.Count);
        Assert.Equal(2, read.Detail.Professions.Count);
        Assert.True(read.Detail.Professions[0].IsPrimary);
        Assert.False(read.Detail.Professions[1].IsPrimary);

        var alchemy = read.Detail.Professions[0];
        Assert.Equal(412, alchemy.Knowledge);
        Assert.Equal([new Specialisation("Potion Mastery", true), new Specialisation("Phial Mastery", false)], alchemy.Specialisations);
        // An older addon writes four columns, and that is silence.
        Assert.Empty(read.Detail.Professions[1].Specialisations);
    }

    [Fact(DisplayName = "quests_from_the_character_file_are_what_a_poisoned_goal_is_measured_against")]
    public void Quests_from_the_character_file_are_what_a_poisoned_goal_is_measured_against()
    {
        Assert.Contains(200L, ReadCharacter().Primary().Quests);
    }

    [Fact(DisplayName = "someone_elses_saved_variables_are_recognised_as_not_ours")]
    public void Someone_elses_saved_variables_are_recognised_as_not_ours()
    {
        Assert.IsType<ReadError.NotCollectorData>(Collector.Read("""TradeSkillMasterDB = { ["x"] = 1 }""").Error);
        Assert.IsType<ReadError.NotCollectorData>(Collector.ReadCharacter("""TradeSkillMasterDB = { ["x"] = 1 }""").Error);
    }

    [Fact(DisplayName = "a_newer_addon_says_so_rather_than_reading_as_broken")]
    public void A_newer_addon_says_so_rather_than_reading_as_broken()
    {
        var error = Collector.Read("""ArmoryCollectorDB = { ["format"] = 99 }""").Error;
        Assert.Equal(new ReadError.FromTheFuture(99), error);
        Assert.Contains("update Armory", error.ToString(), StringComparison.Ordinal);
    }

    [Fact(DisplayName = "a_truncated_file_is_reported_rather_than_half_read")]
    public void A_truncated_file_is_reported_rather_than_half_read()
    {
        Assert.IsType<ReadError.Unparsable>(Collector.Read("""ArmoryCollectorDB = { ["achievements"] = { [1] = """).Error);
    }

    [Fact(DisplayName = "a_character_file_with_no_name_is_not_a_character")]
    public void A_character_file_with_no_name_is_not_a_character()
    {
        Assert.IsType<ReadError.NotCollectorData>(Collector.ReadCharacter("""ArmoryCollectorCharDB = { ["format"] = 2 }""").Error);
    }

    [Fact(DisplayName = "an_empty_dump_is_a_valid_dump")]
    public void An_empty_dump_is_a_valid_dump()
    {
        var collected = Collector.Read("""ArmoryCollectorDB = { ["format"] = 2 }""").Value;
        Assert.Empty(collected.EarnedBy);
        Assert.Empty(collected.Completed);
        Assert.Empty(collected.Tree);
        Assert.Empty(collected.Criteria);
        Assert.Empty(collected.Catalogue);
        Assert.Empty(collected.Currencies);
        Assert.Empty(collected.WarbandBank);
        Assert.Empty(collected.Collectibles);
        Assert.Empty(collected.Owned);
        Assert.Empty(collected.Earned);
        Assert.Empty(collected.Recipes);
        Assert.Empty(collected.Tallies);
        Assert.Empty(collected.PetsHeld);
        Assert.Null(collected.WrittenAt);
        Assert.Null(collected.Client);
    }

    [Fact(DisplayName = "the_addon_answers_what_a_character_is_wearing")]
    public void The_addon_answers_what_a_character_is_wearing()
    {
        var worn = ReadCharacter().Detail.Equipment!;
        Assert.Equal(3, worn.Count);
        var offHand = worn.Single(item => item.Slot == "OFF_HAND");
        Assert.Equal("Off Hand", offHand.SlotName);
        Assert.Equal(612, offHand.Level);
        // A tabard is worn and is not gear: nothing, not nought.
        var tabard = worn.Single(item => item.Slot == "TABARD");
        Assert.Null(tabard.Level);
        Assert.True(tabard.IsCosmetic);
    }

    [Fact(DisplayName = "the_addon_answers_this_weeks_lockouts_and_not_a_lifetime")]
    public void The_addon_answers_this_weeks_lockouts_and_not_a_lifetime()
    {
        var read = ReadCharacter();
        var locks = read.Detail.RaidLocks!;
        var only = Assert.Single(locks);
        Assert.Equal(2, only.Defeated);
        Assert.Equal(8, only.Total);
        Assert.Null(read.Detail.Raids);
    }

    [Fact(DisplayName = "the_vault_is_the_clients_to_answer")]
    public void The_vault_is_the_clients_to_answer()
    {
        var read = ReadCharacter();
        var vault = read.Detail.Vault!;
        Assert.Equal(4, vault.Count);
        Assert.Equal(VaultRow.Dungeons, vault[0].Row);
        Assert.True(vault[0].IsUnlocked);
        Assert.Equal("+12", vault[0].Reward());
        Assert.False(vault[1].IsUnlocked, "three of four is not a slot");
        Assert.Equal(VaultRow.Raids, vault[2].Row);
        Assert.Equal("Heroic", vault[2].Reward());
        Assert.Equal(VaultRow.World, vault[3].Row);
        Assert.Equal("Tier 8", vault[3].Reward());
        Assert.True(read.Detail.VaultReady);
    }

    [Fact(DisplayName = "the_file_says_which_game_it_came_out_of")]
    public void The_file_says_which_game_it_came_out_of()
    {
        var client = Read().Client!;
        Assert.True(client.IsMainline);
        Assert.Equal("12.1.0", client.Version);
        Assert.Equal(120_100, client.Interface);
        Assert.Equal(client, ReadCharacter().Client);
    }

    [Fact(DisplayName = "decor_is_a_collection_the_addon_now_reads")]
    public void Decor_is_a_collection_the_addon_now_reads()
    {
        var collected = Read();
        var chair = collected.Collectibles.Single(entry => entry.Kind == Kind.Decor && entry.Id == 1234);
        Assert.Equal("Sturdy Oak Chair", chair.Name);
        Assert.Equal(240_001, chair.LinkId);
        Assert.Equal(5321, chair.Icon);
        Assert.Contains((Kind.Decor, 1234L), collected.Owned);
        Assert.DoesNotContain((Kind.Decor, 1235L), collected.Owned);
    }

    [Fact(DisplayName = "a_collector_file_from_before_the_gear_scan_is_silence_not_a_naked_character")]
    public void A_collector_file_from_before_the_gear_scan_is_silence_not_a_naked_character()
    {
        var read = Collector.ReadCharacter("""

            ArmoryCollectorCharDB = {
            	["format"] = 2,
            	["name"] = "Somechar",
            	["realm"] = "Emerald Dream",
            	["class"] = "DRUID",
            }

            """).Value;
        Assert.Null(read.Detail.Equipment);
        Assert.Null(read.Detail.RaidLocks);
        Assert.Null(read.Detail.Vault);
        Assert.Null(read.Client);
    }

    [Fact(DisplayName = "the_journals_sentence_classifies_by_its_first_clause")]
    public void The_journals_sentence_classifies_by_its_first_clause()
    {
        Assert.Equal(Source.Drop, Sources.FromText("Drop: Attumen the Huntsman, Karazhan"));
        Assert.Equal(Source.Vendor, Sources.FromText("Vendor: Katie Hunter, Elwynn Forest"));
        Assert.Equal(Source.Promotion, Sources.FromText("Trading Card Game"));
        Assert.Equal(Source.Unknown, Sources.FromText(""));
    }

    [Fact(DisplayName = "a_vendor_near_a_drop_zone_is_still_a_vendor")]
    public void A_vendor_near_a_drop_zone_is_still_a_vendor()
    {
        Assert.Equal(Source.Vendor, Sources.FromText("Vendor: sold near the Drop Zone"));
    }

    [Fact(DisplayName = "the_trading_post_is_a_vendor_rather_than_a_dead_end")]
    public void The_trading_post_is_a_vendor_rather_than_a_dead_end()
    {
        Assert.Equal(Source.Vendor, Sources.FromText("Trading Post"));
        Assert.True(Sources.FromText("Trading Post").IsRepeatable());
    }

    [Fact(DisplayName = "a criterion kind's JSON is the shape the Rust writes")]
    public void A_criterion_kinds_json_is_the_shape_the_rust_writes()
    {
        // `criterion.kind` is a column that travels.
        Assert.Equal("""{"Quest":5000}""", System.Text.Json.JsonSerializer.Serialize(CriterionKind.Quest(5000)));
        Assert.Equal("\"Unknown\"", System.Text.Json.JsonSerializer.Serialize(CriterionKind.Unknown));
        Assert.Equal(CriterionKind.Reputation(2170), System.Text.Json.JsonSerializer.Deserialize<CriterionKind>("""{"Reputation":2170}"""));
        Assert.Equal(CriterionKind.Unknown, System.Text.Json.JsonSerializer.Deserialize<CriterionKind>("\"Unknown\""));
    }
}
