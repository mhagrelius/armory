using Armory.Collections;
using Xunit;

namespace Armory.Tests.Collections;

/// <summary>Ported from <c>core/src/rarity.rs</c>.</summary>
public sealed class RarityTests
{
    private const string Mounts = """

        local addonName, addonTable = ...

        local L = LibStub("AceLocale-3.0"):GetLocale("Rarity")
        local CONSTANTS = addonTable.constants

        if LE_EXPANSION_LEVEL_CURRENT < LE_EXPANSION_LEGION then
        	return {}
        end

        local legionMounts = {
        	-- 7.0
        	["Cloudwing Hippogryph"] = {
        		cat = CONSTANTS.ITEM_CATEGORIES.LEGION,
        		type = CONSTANTS.ITEM_TYPES.MOUNT,
        		method = CONSTANTS.DETECTION_METHODS.USE,
        		name = L["Cloudwing Hippogryph"],
        		spellId = 242881,
        		itemId = 147806,
        		items = { 152102 },
        		chance = 20,
        		coords = { { m = CONSTANTS.UIMAPIDS.AZSUNA } },
        	},
        	["Deathcharger's Reins"] = {
        		method = CONSTANTS.DETECTION_METHODS.NPC,
        		name = L["Deathcharger's Reins"],
        		spellId = 17481,
        		itemId = 13335,
        		npcs = { 99999 },
        		tooltipNpcs = { 45412 },
        		chance = 100,
        		statisticId = { 1097 },
        		coords = { { m = 317, x = 38.6, y = 20, i = true } },
        	},
        }

        Rarity.ItemDB.MergeItems(Rarity.ItemDB.mounts, legionMounts)
        return legionMounts

        """;

    [Fact(DisplayName = "the_four_fields_come_out_and_the_rest_is_ignored")]
    public void The_four_fields_come_out_and_the_rest_is_ignored()
    {
        var read = Rarity.Parse(Mounts);
        Assert.Equal(2, read.Count);
        Assert.Equal("Cloudwing Hippogryph", read[0].Name);
        Assert.Equal(242_881, read[0].SpellId);
        Assert.Equal(147_806, read[0].ItemId);
        Assert.Equal(20, read[0].OneIn);
        Assert.Equal(100, read[1].OneIn);
    }

    [Fact(DisplayName = "a_number_inside_a_nested_table_is_never_read_as_a_field")]
    public void A_number_inside_a_nested_table_is_never_read_as_a_field()
    {
        var deathcharger = Rarity.Parse(Mounts)[1];
        Assert.Equal(13_335, deathcharger.ItemId);
        Assert.Null(deathcharger.CreatureId);
    }

    [Fact(DisplayName = "the_lua_around_the_table_is_not_evaluated_and_not_mistaken_for_data")]
    public void The_lua_around_the_table_is_not_evaluated_and_not_mistaken_for_data()
    {
        Assert.Equal(2, Rarity.Parse(Mounts).Count);
        Assert.Empty(Rarity.Parse("local x = LibStub(\"Ace\"):GetLocale(\"Rarity\")"));
    }

    [Fact(DisplayName = "a_field_that_is_not_a_bare_number_is_left_alone")]
    public void A_field_that_is_not_a_bare_number_is_left_alone()
    {
        var read = Rarity.Parse("\n\t[\"Odd One\"] = {\n\t\tspellId = CONSTANTS.SOMETHING,\n\t\titemId = 42,\n\t\tchance = 15,\n\t},\n");
        var only = Assert.Single(read);
        Assert.Null(only.SpellId);
        Assert.Equal(42, only.ItemId);
    }

    [Fact(DisplayName = "an_entry_with_no_chance_or_nothing_to_join_on_is_dropped")]
    public void An_entry_with_no_chance_or_nothing_to_join_on_is_dropped()
    {
        Assert.Empty(Rarity.Parse("\t[\"No Chance\"] = {\n\t\titemId = 42,\n\t},"));
        Assert.Empty(Rarity.Parse("\t[\"No Id\"] = {\n\t\tchance = 20,\n\t},"));
        Assert.Empty(Rarity.Parse("\t[\"Zero\"] = {\n\t\titemId = 42,\n\t\tchance = 0,\n\t},"));
    }

    [Fact(DisplayName = "a_trailing_comment_does_not_hide_the_number")]
    public void A_trailing_comment_does_not_hide_the_number()
    {
        var read = Rarity.Parse("\n\t[\"Arfus\"] = {\n\t\tspellId = 406225,\n\t\titemId = 211271,\n\t\titems = { 209024 },\n\t\tchance = 100, -- Blind guess\n\t\tcreatureId = 203463,\n\t},\n");
        var only = Assert.Single(read);
        Assert.Equal(100, only.OneIn);
        Assert.Equal(203_463, only.CreatureId);
    }

    [Fact(DisplayName = "a_fractional_chance_is_rounded_rather_than_refused")]
    public void A_fractional_chance_is_rounded_rather_than_refused()
    {
        Assert.Equal(3, Rarity.Parse("\t[\"Fel-Spotted Egg\"] = {\n\t\titemId = 1,\n\t\tchance = 2.5,\n\t},")[0].OneIn);
    }

    [Fact(DisplayName = "a_chance_is_one_in_that_many_and_not_a_percentage")]
    public void A_chance_is_one_in_that_many_and_not_a_percentage()
    {
        var read = Rarity.Parse(Mounts);
        Assert.Equal(100, read[1].OneIn);
        Assert.True(read[1].OneIn > 1);
    }

    [Fact(DisplayName = "chances join on the link id and refuse a guessed toy item")]
    public void Chances_join_on_the_link_id_and_refuse_a_guessed_toy_item()
    {
        var chances = new Chances(Rarity.Parse(Mounts));
        Assert.Equal(2, chances.Known);
        Assert.Equal(20, chances.OneIn(new Collectible { Kind = Kind.Mount, Id = 1, LinkId = 242_881 }));
        Assert.Null(chances.OneIn(new Collectible { Kind = Kind.Pet, Id = 1, LinkId = 242_881 }));
        // A toy whose link id is still the collection id is a guess.
        Assert.Null(chances.OneIn(new Collectible { Kind = Kind.Toy, Id = 147_806, LinkId = 147_806 }));
        Assert.Equal(20, chances.OneIn(new Collectible { Kind = Kind.Toy, Id = 5, LinkId = 147_806 }));
    }

    [Fact(DisplayName = "an install with no Rarity reads as no chances rather than failing")]
    public void An_install_with_no_rarity_reads_as_no_chances_rather_than_failing()
    {
        Assert.True(Rarity.Read(Path.Combine(Path.GetTempPath(), "nonexistent-" + Guid.NewGuid())).IsEmpty);
    }
}
