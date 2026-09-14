using System.Text;
using System.Text.Json.Nodes;
using Armory.Blizzard;
using Armory.Roster;
using Armory.Run;
using Xunit;

namespace Armory.Tests.Blizzard;

/// <summary>Ported from <c>core/src/source/blizzard/profile.rs</c>, fixtures verbatim.</summary>
public sealed class ProfileTests
{
    private static byte[] B(string text) => Encoding.UTF8.GetBytes(text);

    private static T Found<T>(Outcome<T> outcome) => Assert.IsType<Outcome<T>.Found>(outcome).Value;

    [Fact(DisplayName = "the_account_index_yields_every_character_across_every_licence")]
    public void The_account_index_yields_every_character_across_every_licence()
    {
        var roster = Found(Profile.ParseAccount(B("""
            {
              "id": 1,
              "wow_accounts": [
                {"id": 11, "characters": [
                  {"id": 5, "name": "Somechar", "level": 80,
                   "playable_class": {"name": "Druid"},
                   "playable_race": {"name": "Tauren"},
                   "faction": {"type": "HORDE"},
                   "realm": {"id": 61, "name": "Emerald Dream", "slug": "emerald-dream"}}
                ]},
                {"id": 12, "characters": [
                  {"id": 6, "name": "Aeltor", "level": 70,
                   "playable_class": {"name": "Warrior"},
                   "playable_race": {"name": "Orc"},
                   "faction": {"type": "HORDE"},
                   "realm": {"id": 13, "name": "Mannoroth", "slug": "mannoroth"}}
                ]}
              ]
            }
            """)));
        Assert.Equal(2, roster.Count);
        Assert.Equal("Somechar", roster.Characters[0].DisplayName);
        Assert.Equal("somechar", roster.Characters[0].Key.Name);
        Assert.Equal(12, roster.Characters[1].WowAccountId);
    }

    [Fact(DisplayName = "a_realm_with_no_slug_gets_one_derived")]
    public void A_realm_with_no_slug_gets_one_derived()
    {
        var roster = Found(Profile.ParseAccount(B("""{"wow_accounts":[{"id":1,"characters":[{"id":5,"name":"Velkurai","level":70,"realm":{"id":61,"name":"Emerald Dream"}}]}]}""")));
        Assert.Equal("emerald-dream", roster.Characters[0].Key.RealmSlug);
    }

    [Fact(DisplayName = "a_missing_wow_accounts_key_is_stale_not_empty")]
    public void A_missing_wow_accounts_key_is_stale_not_empty()
    {
        Assert.IsType<Outcome<Armory.Roster.Roster>.Stale>(Profile.ParseAccount(B("""{"id":1}""")));
    }

    [Fact(DisplayName = "achievements_carry_partial_criteria_progress")]
    public void Achievements_carry_partial_criteria_progress()
    {
        var list = Found(Profile.ParseAchievements(B("""
            {"achievements":[
                {"id": 1, "achievement": {"id": 1, "name": "Done"},
                 "completed_timestamp": 1467331200000,
                 "criteria": {"id": 10, "amount": 3, "is_completed": true,
                              "child_criteria": [
                                {"id": 11, "is_completed": true},
                                {"id": 12, "is_completed": false}]}},
                {"id": 2, "achievement": {"id": 2, "name": "Partway"}}
            ]}
            """)));
        Assert.Equal(2, list.Count);
        Assert.NotNull(list[0].CompletedAt);
        var criteria = list[0].Criteria!;
        Assert.Equal(3, criteria.Required);
        Assert.Equal(2, criteria.Children.Count);
        Assert.Null(list[1].CompletedAt);
    }

    [Fact(DisplayName = "the_profile_never_says_what_a_criterion_measures")]
    public void The_profile_never_says_what_a_criterion_measures()
    {
        var list = Found(Profile.ParseAchievements(B("""{"achievements":[{"id":1,"achievement":{"id":1},"criteria":{"id":10,"amount":3,"is_completed":false}}]}""")));
        Assert.Equal(CriterionKind.Unknown, list[0].Criteria!.Kind);
    }

    [Fact(DisplayName = "a_character_who_has_completed_no_quests_is_empty_not_stale")]
    public void A_character_who_has_completed_no_quests_is_empty_not_stale()
    {
        Assert.IsType<Outcome<HashSet<long>>.Empty>(Profile.ParseCompletedQuests(B("{}")));
    }

    [Fact(DisplayName = "completed_quests_flatten_to_a_set_of_ids")]
    public void Completed_quests_flatten_to_a_set_of_ids()
    {
        var quests = Found(Profile.ParseCompletedQuests(B("""{"quests":[{"id":100},{"id":200},{"id":300}]}""")));
        Assert.Equal(3, quests.Count);
        Assert.Contains(200L, quests);
    }

    [Fact(DisplayName = "statistics_flatten_out_of_their_category_tree")]
    public void Statistics_flatten_out_of_their_category_tree()
    {
        var statistics = Found(Profile.ParseStatistics(B("""
            {"categories":[
                {"id": 1, "name": "Character",
                 "statistics": [{"id": 10, "quantity": 42.0}],
                 "sub_categories": [
                    {"id": 2, "statistics": [{"id": 20, "quantity": 7.0}],
                     "sub_categories": [
                        {"id": 3, "statistics": [{"id": 30, "quantity": 1.0}]}]}]}
            ]}
            """)));
        Assert.Equal(3, statistics.Count);
        Assert.Equal(1.0, statistics[30]);
    }

    [Fact(DisplayName = "renown_on_a_low_character_is_marked_inherited")]
    public void Renown_on_a_low_character_is_marked_inherited()
    {
        var reputations = Found(Profile.ParseReputations(B("""{"reputations":[{"faction": {"id": 2570}, "standing": {"raw": 4200, "renown_level": 20}}]}"""), 20));
        Assert.Contains(2570L, reputations.Inherited);
        Assert.Equal(4200, reputations.Standings[2570]);
    }

    [Fact(DisplayName = "renown_on_a_levelled_character_is_left_alone")]
    public void Renown_on_a_levelled_character_is_left_alone()
    {
        var reputations = Found(Profile.ParseReputations(B("""{"reputations":[{"faction": {"id": 2570}, "standing": {"raw": 4200, "renown_level": 20}}]}"""), 80));
        Assert.Empty(reputations.Inherited);
    }

    [Fact(DisplayName = "a_plain_reputation_with_no_renown_is_never_inherited")]
    public void A_plain_reputation_with_no_renown_is_never_inherited()
    {
        var reputations = Found(Profile.ParseReputations(B("""{"reputations":[{"faction": {"id": 69}, "standing": {"raw": 21000}}]}"""), 20));
        Assert.Empty(reputations.Inherited);
    }

    [Fact(DisplayName = "a_standing_keeps_its_words_as_well_as_its_number")]
    public void A_standing_keeps_its_words_as_well_as_its_number()
    {
        var reputations = Found(Profile.ParseReputations(B("""
            {"reputations":[
                {"faction": {"id": 69, "name": "Darnassus"},
                 "standing": {"raw": 21000, "value": 5000, "max": 21000, "name": "Revered"}},
                {"faction": {"id": 2570, "name": "Dream Wardens"},
                 "standing": {"raw": 4200, "renown_level": 20}}
            ]}
            """), 80));
        Assert.Equal(["Darnassus", "Dream Wardens"], reputations.Detail.Select(standing => standing.Name));
        var darnassus = reputations.Detail[0];
        Assert.Equal("Revered", darnassus.Tier);
        Assert.True(Math.Abs(darnassus.Fraction()!.Value - 5000.0 / 21000.0) < 1e-9);
        Assert.Equal("Renown 20", reputations.Detail[1].Tier);
        Assert.Equal(20, reputations.Detail[1].Renown);
    }

    [Fact(DisplayName = "a_maxed_standing_has_no_fraction_rather_than_a_full_bar")]
    public void A_maxed_standing_has_no_fraction_rather_than_a_full_bar()
    {
        var reputations = Found(Profile.ParseReputations(B("""{"reputations":[{"faction": {"id": 69, "name": "Darnassus"},"standing": {"raw": 42999, "value": 0, "max": 0, "name": "Exalted"}}]}"""), 80));
        Assert.Null(reputations.Detail[0].Fraction());
    }

    [Fact(DisplayName = "a_summary_yields_the_fields_a_roster_row_shows")]
    public void A_summary_yields_the_fields_a_roster_row_shows()
    {
        var detail = Found(Profile.ParseSummary(B("""
            {"id": 5, "name": "Somechar", "level": 80,
             "average_item_level": 642, "equipped_item_level": 639,
             "achievement_points": 21450,
             "last_login_timestamp": 1785000000000,
             "active_spec": {"name": "Restoration"},
             "guild": {"name": "Dream Team"}}
            """)));
        Assert.Equal(642, detail.ItemLevel);
        Assert.Equal(639, detail.EquippedItemLevel);
        Assert.Equal("Restoration", detail.Spec);
        Assert.Equal("Dream Team", detail.Guild);
        Assert.Equal(21450, detail.AchievementPoints);
        Assert.NotNull(detail.LastLogin);
    }

    [Fact(DisplayName = "a_guildless_character_is_not_a_broken_summary")]
    public void A_guildless_character_is_not_a_broken_summary()
    {
        var detail = Found(Profile.ParseSummary(B("""{"id": 5, "average_item_level": 600}""")));
        Assert.Null(detail.Guild);
        Assert.Equal(600, detail.ItemLevel);
    }

    [Fact(DisplayName = "a_response_with_no_id_is_stale")]
    public void A_response_with_no_id_is_stale()
    {
        Assert.IsType<Outcome<Detail>.Stale>(Profile.ParseSummary(B("""{"name": "x"}""")));
    }

    [Fact(DisplayName = "only_the_current_tier_of_a_profession_is_kept")]
    public void Only_the_current_tier_of_a_profession_is_kept()
    {
        var professions = Found(Profile.ParseProfessions(B("""
            {"primaries": [
                {"profession": {"name": "Blacksmithing"},
                 "tiers": [
                   {"tier": {"name": "Classic Blacksmithing"}, "skill_points": 300,
                    "max_skill_points": 300},
                   {"tier": {"name": "Khaz Algar Blacksmithing"}, "skill_points": 84,
                    "max_skill_points": 100}]}],
              "secondaries": [
                {"profession": {"name": "Cooking"},
                 "tiers": [{"tier": {"name": "Khaz Algar Cooking"}, "skill_points": 40,
                            "max_skill_points": 100}]}]}
            """)));
        Assert.Equal(2, professions.Count);
        Assert.Equal("Blacksmithing", professions[0].Name);
        Assert.Equal("Khaz Algar Blacksmithing", professions[0].Tier);
        Assert.Equal(84, professions[0].Skill);
        Assert.True(professions[0].IsPrimary);
        Assert.False(professions[1].IsPrimary);
    }

    [Fact(DisplayName = "a_character_with_no_professions_is_empty")]
    public void A_character_with_no_professions_is_empty()
    {
        Assert.IsType<Outcome<List<Profession>>.Empty>(Profile.ParseProfessions(B("{}")));
    }

    [Fact(DisplayName = "no_keys_this_season_is_empty_rather_than_a_rating_of_zero")]
    public void No_keys_this_season_is_empty_rather_than_a_rating_of_zero()
    {
        Assert.IsType<Outcome<long>.Empty>(Profile.ParseMythicKeystone(B("""{"current_period": {}}""")));
        Assert.Equal(2418, Found(Profile.ParseMythicKeystone(B("""{"current_mythic_rating": {"rating": 2418.7}}"""))));
    }

    [Fact(DisplayName = "gold_comes_from_the_protected_endpoint_and_nowhere_else")]
    public void Gold_comes_from_the_protected_endpoint_and_nowhere_else()
    {
        Assert.Equal(91234567, Found(Profile.ParseProtected(B("""{"character": {}, "money": 91234567}"""))));
    }

    [Fact(DisplayName = "encounters_flatten_out_of_their_nesting")]
    public void Encounters_flatten_out_of_their_nesting()
    {
        var encounters = Found(Profile.ParseEncounters(B("""
            {"expansions": [
                {"expansion": {"name": "Current"},
                 "instances": [
                   {"instance": {"name": "A Dungeon"},
                    "modes": [
                      {"difficulty": {"type": "MYTHIC"},
                       "progress": {"encounters": [
                         {"encounter": {"id": 2600}, "completed_count": 3},
                         {"encounter": {"id": 2601}, "completed_count": 1}]}}]}]}]}
            """)));
        Assert.Contains(2600L, encounters);
        Assert.Contains(2601L, encounters);
        Assert.Equal(2, encounters.Count);
    }

    [Fact(DisplayName = "an_encounter_listed_but_never_killed_does_not_count_as_cleared")]
    public void An_encounter_listed_but_never_killed_does_not_count_as_cleared()
    {
        Assert.IsType<Outcome<HashSet<long>>.Empty>(Profile.ParseEncounters(B("""{"expansions": [{"instances": [{"modes": [{"progress": {"encounters": [{"encounter": {"id": 2600}, "completed_count": 0}]}}]}]}]}""")));
    }

    [Fact(DisplayName = "a_character_who_has_cleared_nothing_is_empty_not_stale")]
    public void A_character_who_has_cleared_nothing_is_empty_not_stale()
    {
        Assert.IsType<Outcome<HashSet<long>>.Empty>(Profile.ParseEncounters(B("""{"character": {}}""")));
    }

    [Fact(DisplayName = "renown_reports_the_highest_the_account_reached")]
    public void Renown_reports_the_highest_the_account_reached()
    {
        var value = JsonNode.Parse("""{"reputations": [{"faction": {"id": 1}, "standing": {"renown_level": 12}},{"faction": {"id": 2}, "standing": {"renown_level": 25}},{"faction": {"id": 3}, "standing": {"raw": 21000}}]}""")!;
        Assert.Equal(25, Profile.HighestRenown(value));
    }

    [Fact(DisplayName = "requests_are_addressed_the_way_the_endpoints_want")]
    public void Requests_are_addressed_the_way_the_endpoints_want()
    {
        var request = Profile.Achievements(Region.Us, new CharacterKey("emerald-dream", "Somechar"));
        Assert.Contains("/profile/wow/character/emerald-dream/somechar/achievements", request.Url, StringComparison.Ordinal);
        Assert.Contains("namespace=profile-us", request.Url, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "the_protected_endpoint_is_addressed_by_numbers_not_by_name")]
    public void The_protected_endpoint_is_addressed_by_numbers_not_by_name()
    {
        var character = new Character
        {
            Key = new CharacterKey("emerald-dream", "Somechar"),
            Id = 12345,
            RealmId = 3684,
            DisplayName = "Somechar",
            RealmName = "Emerald Dream",
            Level = 80,
            Class = "Druid",
            Race = "Tauren",
            Faction = Faction.Horde,
            WowAccountId = 1,
        };
        Assert.Contains("/profile/user/wow/protected-character/3684-12345", Profile.ProtectedCharacter(Region.Us, character).Url, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "equipment_reads_the_slot_the_name_and_the_level")]
    public void Equipment_reads_the_slot_the_name_and_the_level()
    {
        var worn = Found(Profile.ParseEquipment(B("""
            {"equipped_items": [
                {"slot": {"type": "HEAD", "name": "Head"}, "name": "Helm of the Broken",
                 "level": {"value": 639, "display_string": "Item Level 639"}},
                {"slot": {"type": "FINGER_1", "name": "Ring 1"}, "name": "Band of Oshu'gun",
                 "level": {"value": 626}}
            ]}
            """)));
        Assert.Equal(2, worn.Count);
        Assert.Equal("HEAD", worn[0].Slot);
        Assert.Equal("Helm of the Broken", worn[0].Name);
        Assert.Equal(639, worn[0].Level);
        Assert.Equal("Ring 1", worn[1].SlotName);
    }

    [Fact(DisplayName = "a_cosmetic_slot_has_no_item_level_rather_than_a_zero")]
    public void A_cosmetic_slot_has_no_item_level_rather_than_a_zero()
    {
        var worn = Found(Profile.ParseEquipment(B("""{"equipped_items": [{"slot": {"type": "TABARD", "name": "Tabard"}, "name": "Tabard of the Kurenai"}]}""")));
        Assert.Null(worn[0].Level);
        Assert.True(worn[0].IsCosmetic);
    }

    [Fact(DisplayName = "an_empty_slot_is_absent_rather_than_reported_empty")]
    public void An_empty_slot_is_absent_rather_than_reported_empty()
    {
        var worn = Found(Profile.ParseEquipment(B("""{"equipped_items": [{"slot": {"type": "HEAD", "name": "Head"}, "name": "Helm", "level": {"value": 600}}]}""")));
        Assert.Single(worn);
        Assert.DoesNotContain(worn, item => item.Slot == "OFF_HAND");
    }

    [Fact(DisplayName = "a_naked_character_is_empty_and_a_broken_response_is_stale")]
    public void A_naked_character_is_empty_and_a_broken_response_is_stale()
    {
        Assert.IsType<Outcome<List<Equipped>>.Empty>(Profile.ParseEquipment(B("""{"equipped_items": []}""")));
        Assert.IsType<Outcome<List<Equipped>>.Stale>(Profile.ParseEquipment(B("""{"character": {}}""")));
    }

    [Fact(DisplayName = "raids_are_read_per_instance_and_per_difficulty")]
    public void Raids_are_read_per_instance_and_per_difficulty()
    {
        var tiers = Found(Profile.ParseRaids(B("""
            {"expansions": [{"expansion": {"name": "The War Within"},
              "instances": [{"instance": {"id": 1296, "name": "Liberation of Undermine"},
                "modes": [
                  {"difficulty": {"type": "NORMAL", "name": "Normal"}, "status": {"type": "COMPLETE"},
                   "progress": {"completed_count": 8, "total_count": 8, "encounters": [
                     {"encounter": {"id": 2639, "name": "Vexie"}, "completed_count": 3,
                      "last_kill_timestamp": 1750000000000},
                     {"encounter": {"id": 2640, "name": "Mug'Zee"}, "completed_count": 1,
                      "last_kill_timestamp": 1753800000000}]}},
                  {"difficulty": {"type": "HEROIC", "name": "Heroic"}, "status": {"type": "IN_PROGRESS"},
                   "progress": {"completed_count": 2, "total_count": 8, "encounters": [
                     {"encounter": {"id": 2639, "name": "Vexie"}, "completed_count": 1,
                      "last_kill_timestamp": 1754000000000}]}}]}]}]}
            """)));
        var tier = Assert.Single(tiers);
        Assert.Equal("Liberation of Undermine", tier.Name);
        Assert.Equal("The War Within", tier.Expansion);
        Assert.Equal(2, tier.Difficulties.Count);
        Assert.Equal(2, tier.Difficulties[1].Defeated);
        Assert.Equal(8, tier.Difficulties[1].Total);
        var (boss, _, difficulty) = tier.LastKill()!.Value;
        Assert.Equal("Vexie", boss);
        Assert.Equal("Heroic", difficulty);
    }

    [Fact(DisplayName = "a_difficulty_never_entered_carries_no_last_kill")]
    public void A_difficulty_never_entered_carries_no_last_kill()
    {
        var tiers = Found(Profile.ParseRaids(B("""
            {"expansions": [{"expansion": {"name": "The War Within"},
              "instances": [{"instance": {"name": "Liberation of Undermine"},
                "modes": [{"difficulty": {"type": "MYTHIC", "name": "Mythic"},
                  "progress": {"completed_count": 0, "total_count": 8, "encounters": [
                    {"encounter": {"id": 2639, "name": "Vexie"}, "completed_count": 0}]}}]}]}]}
            """)));
        Assert.Equal(0, tiers[0].Difficulties[0].Defeated);
        Assert.Null(tiers[0].Difficulties[0].LastKill);
        Assert.Null(tiers[0].LastKill());
    }

    [Fact(DisplayName = "a_character_who_has_never_raided_is_empty")]
    public void A_character_who_has_never_raided_is_empty()
    {
        Assert.IsType<Outcome<List<RaidTier>>.Empty>(Profile.ParseRaids(B("""{"character": {}}""")));
    }
}
