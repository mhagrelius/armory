using System.Text.Json;
using Armory.Chronicle;
using Armory.Roster;
using Xunit;

namespace Armory.Tests.Chronicle;

/// <summary>Ported from <c>core/src/chronicle.rs</c>.</summary>
public sealed class SessionTests
{
    private static Moment At(long seconds, Happening what) => new() { At = seconds, What = what };

    internal static Session ASession(params Moment[] moments) => new()
    {
        Character = new CharacterKey("emerald-dream", "Somechar"),
        DisplayName = "Somechar",
        RealmName = "Emerald Dream",
        Class = "Druid",
        Race = "Tauren",
        Faction = Faction.Horde,
        StartedAt = new DateTimeOffset(2026, 8, 3, 19, 0, 0, TimeSpan.Zero),
        EndedAt = new DateTimeOffset(2026, 8, 3, 21, 30, 0, TimeSpan.Zero),
        StartLevel = 70,
        EndLevel = 71,
        StartMoney = 1_000_000,
        EndMoney = 1_250_000,
        StartItemLevel = 600,
        EndItemLevel = 604,
        Moments = [.. moments],
    };

    [Fact(DisplayName = "a_route_keeps_its_order_and_collects_its_subzones")]
    public void A_route_keeps_its_order_and_collects_its_subzones()
    {
        var digest = ASession(
            At(0, new Happening.Arrived("Orgrimmar", null, null)),
            At(600, new Happening.Arrived("Durotar", "Razor Hill", null)),
            At(900, new Happening.Arrived("Durotar", "Echo Isles", null)),
            At(1800, new Happening.Arrived("Orgrimmar", null, null))).Digest();
        Assert.Equal(["Orgrimmar", "Durotar", "Orgrimmar"], digest.Route.Select(stop => stop.Zone));
        Assert.Equal(["Razor Hill", "Echo Isles"], digest.Route[1].Within);
    }

    [Fact(DisplayName = "a_quest_carries_the_text_the_player_actually_read")]
    public void A_quest_carries_the_text_the_player_actually_read()
    {
        var digest = ASession(
            At(10, new Happening.Accepted("The Battle for Gilneas", "The Forsaken are at the wall.")),
            At(400, new Happening.Completed(12345, "The Battle for Gilneas", "You have done Gilneas proud.")),
            At(400, new Happening.Paid(12345, 45_000, 1200))).Digest();
        var quest = Assert.Single(digest.Quests);
        Assert.Equal("The Forsaken are at the wall.", quest.Premise);
        Assert.Equal("You have done Gilneas proud.", quest.Story);
        Assert.Equal(45_000, quest.Money);
        Assert.Equal(0, digest.QuestIncome);
    }

    [Fact(DisplayName = "a_quest_taken_and_not_finished_is_reported_as_left_hanging")]
    public void A_quest_taken_and_not_finished_is_reported_as_left_hanging()
    {
        var digest = ASession(
            At(10, new Happening.Accepted("Into the Maw", null)),
            At(20, new Happening.Accepted("A Simple Errand", null)),
            At(30, new Happening.Completed(1, "A Simple Errand", null))).Digest();
        Assert.Equal(["Into the Maw"], digest.TakenUp);
    }

    [Fact(DisplayName = "a_boss_wiped_on_and_then_killed_counts_as_killed")]
    public void A_boss_wiped_on_and_then_killed_counts_as_killed()
    {
        var digest = ASession(
            At(100, new Happening.Fought("Sire Denathrius", false)),
            At(200, new Happening.Fought("Sire Denathrius", false)),
            At(300, new Happening.Fought("Sire Denathrius", true))).Digest();
        Assert.Equal(["Sire Denathrius"], digest.Felled);
        Assert.Empty(digest.LostTo);
    }

    [Fact(DisplayName = "a_wipe_with_no_kill_after_it_is_still_a_story")]
    public void A_wipe_with_no_kill_after_it_is_still_a_story()
    {
        var digest = ASession(At(100, new Happening.Fought("Fyrakk", false))).Digest();
        Assert.Equal(["Fyrakk"], digest.LostTo);
        Assert.Empty(digest.Felled);
        Assert.True(digest.IsWorthWriting());
    }

    [Fact(DisplayName = "the_best_pull_is_the_number_a_wipe_keeps")]
    public void The_best_pull_is_the_number_a_wipe_keeps()
    {
        var digest = ASession(
            At(100, new Happening.Fought("Fyrakk", false)),
            At(100, new Happening.Wiped("Fyrakk", 40)),
            At(200, new Happening.Fought("Fyrakk", false)),
            At(200, new Happening.Wiped("Fyrakk", 4)),
            At(300, new Happening.Fought("Fyrakk", false)),
            At(300, new Happening.Wiped("Fyrakk", 22))).Digest();
        Assert.Equal(["Fyrakk (down to 4%)"], digest.LostTo);
    }

    [Fact(DisplayName = "a_wipe_number_does_not_outlive_the_kill")]
    public void A_wipe_number_does_not_outlive_the_kill()
    {
        var digest = ASession(
            At(100, new Happening.Fought("Fyrakk", false)),
            At(100, new Happening.Wiped("Fyrakk", 4)),
            At(200, new Happening.Fought("Fyrakk", true))).Digest();
        Assert.Equal(["Fyrakk"], digest.Felled);
        Assert.Empty(digest.LostTo);
    }

    [Fact(DisplayName = "a_world_tier_is_said_once_per_setting")]
    public void A_world_tier_is_said_once_per_setting()
    {
        var digest = ASession(
            At(0, new Happening.WorldTier("Heroic")),
            At(600, new Happening.WorldTier("Mythic")),
            At(900, new Happening.WorldTier("Heroic"))).Digest();
        Assert.Equal(["Heroic", "Mythic"], digest.WorldTiers);
        Assert.False(digest.IsWorthWriting());
    }

    [Fact(DisplayName = "the_weather_is_said_once_per_turn")]
    public void The_weather_is_said_once_per_turn()
    {
        var digest = ASession(
            At(100, new Happening.Weather("Rain", "Nagrand")),
            At(700, new Happening.Weather("Clear", "Nagrand")),
            At(900, new Happening.Weather("Rain", "Nagrand")),
            At(1200, new Happening.Weather("Snow", null))).Digest();
        Assert.Equal(["Rain over Nagrand", "Clear over Nagrand", "Snow"], digest.Weather);
        Assert.False(digest.IsWorthWriting());
    }

    [Fact(DisplayName = "checking_the_mail_is_not_an_evening")]
    public void Checking_the_mail_is_not_an_evening()
    {
        var quiet = ASession(At(0, new Happening.Arrived("Orgrimmar", null, null)));
        quiet = quiet with { EndedAt = quiet.StartedAt + TimeSpan.FromMinutes(4) };
        Assert.False(quiet.Digest().IsWorthWriting());
    }

    [Fact(DisplayName = "a_long_wander_with_no_events_is_still_an_evening")]
    public void A_long_wander_with_no_events_is_still_an_evening()
    {
        var wander = ASession(At(0, new Happening.Arrived("Nagrand", null, null)), At(1800, new Happening.Arrived("Zangarmarsh", null, null)));
        wander = wander with { EndedAt = wander.StartedAt + TimeSpan.FromMinutes(50) };
        Assert.True(wander.Digest().IsWorthWriting());
    }

    [Fact(DisplayName = "a_storyline_is_named_once_however_many_of_its_chapters_were_finished")]
    public void A_storyline_is_named_once_however_many_of_its_chapters_were_finished()
    {
        var digest = ASession(
            At(10, new Happening.Campaign("The Severed Threads", "The Nerubians are not finished.")),
            At(20, new Happening.Campaign("The Severed Threads", null))).Digest();
        var campaign = Assert.Single(digest.Campaigns);
        Assert.Equal("The Nerubians are not finished.", campaign.Summary);
    }

    [Fact(DisplayName = "a_death_carries_what_did_it_where_the_log_caught_it")]
    public void A_death_carries_what_did_it_where_the_log_caught_it()
    {
        var digest = ASession(
            At(100, new Happening.Died("Nagrand", "Halaa", "Gorian Warlock")),
            At(200, new Happening.Died("Nagrand", null, null))).Digest();
        Assert.Equal("Gorian Warlock", digest.Deaths[0].To);
        Assert.Null(digest.Deaths[1].To);
    }

    [Fact(DisplayName = "a_keystone_leads_the_headline_because_it_is_what_the_evening_was")]
    public void A_keystone_leads_the_headline_because_it_is_what_the_evening_was()
    {
        var digest = ASession(
            At(0, new Happening.Arrived("Dornogal", null, null)),
            At(60, new Happening.Entered("Ara-Kara, City of Echoes", "party, Mythic Keystone", 5)),
            At(2000, new Happening.Keystone("Ara-Kara, City of Echoes", 18, true, 2, 1_620))).Digest();
        Assert.StartsWith("+18 Ara-Kara", digest.Headline(), StringComparison.Ordinal);
        Assert.Single(digest.Instances);
        Assert.True(digest.Keystones[0].InTime);
    }

    [Fact(DisplayName = "a_profession_reports_the_best_it_reached_rather_than_every_step")]
    public void A_profession_reports_the_best_it_reached_rather_than_every_step()
    {
        var digest = ASession(At(10, new Happening.Practised("Alchemy", 84)), At(20, new Happening.Practised("Alchemy", 91))).Digest();
        Assert.Equal([("Alchemy", 91)], digest.Practised);
    }

    [Fact(DisplayName = "the_biggest_gear_upgrade_leads")]
    public void The_biggest_gear_upgrade_leads()
    {
        var digest = ASession(
            At(10, new Happening.Equipped("Cloak of Small Favours", 604, 2)),
            At(20, new Happening.Equipped("Sureki Zealot's Insignia", 639, 26))).Digest();
        Assert.Equal("Sureki Zealot's Insignia", digest.Equipped[0].Name);
    }

    [Fact(DisplayName = "a_rare_that_was_also_a_boss_kill_is_one_thing")]
    public void A_rare_that_was_also_a_boss_kill_is_one_thing()
    {
        var digest = ASession(At(10, new Happening.Felled("Doomwalker")), At(10, new Happening.Rare("Doomwalker", "worldboss"))).Digest();
        Assert.Equal(["Doomwalker"], digest.Felled);
        Assert.Empty(digest.Rares);
    }

    [Fact(DisplayName = "a_piece_of_gear_is_credited_to_what_dropped_it_and_to_nothing_else")]
    public void A_piece_of_gear_is_credited_to_what_dropped_it_and_to_nothing_else()
    {
        var digest = ASession(
            At(100, new Happening.Felled("Rasha'nan")),
            At(104, new Happening.Looted(221_023, "Wingcarver Sabatons", 4)),
            At(120, new Happening.Equipped("Wingcarver Sabatons", 639, 26)),
            At(200, new Happening.Felled("Nexus-Princess Ky'veza")),
            At(2_000, new Happening.Equipped("Cloak of Small Favours", 604, 2))).Digest();
        Assert.Equal("Rasha'nan", digest.Equipped.Single(gear => gear.Name == "Wingcarver Sabatons").From);
        Assert.Null(digest.Equipped.Single(gear => gear.Name == "Cloak of Small Favours").From);
    }

    [Fact(DisplayName = "the_ledger_is_the_only_set_of_books_money_is_counted_in")]
    public void The_ledger_is_the_only_set_of_books_money_is_counted_in()
    {
        var digest = ASession(
            At(100, new Happening.Completed(9_923, "Hero of the Mag'har", null)),
            At(100, new Happening.Paid(9_923, 84_500, 0)),
            At(100, new Happening.Coin(Purpose.Quest, 84_500, true)),
            At(200, new Happening.Coin(Purpose.Loot, 12_000, true)),
            At(300, new Happening.Coin(Purpose.Bid, 400_000, false)),
            At(310, new Happening.Coin(Purpose.Repair, 9_000, false))).Digest();
        Assert.Equal([(Purpose.Quest, 84_500L), (Purpose.Loot, 12_000L)], digest.Income);
        Assert.Equal([(Purpose.Bid, 400_000L), (Purpose.Repair, 9_000L)], digest.Spending);
        Assert.Equal(84_500, digest.QuestIncome);
        Assert.Equal(84_500, digest.Quests[0].Money);
    }

    [Fact(DisplayName = "gold_an_alt_sent_over_is_not_income_to_anybody")]
    public void Gold_an_alt_sent_over_is_not_income_to_anybody()
    {
        var digest = ASession(
            At(100, new Happening.Coin(Purpose.Sale, 250_000, true)),
            At(200, new Happening.Coin(Purpose.Transfer, 5_000_000, true))).Digest();
        Assert.Equal(250_000, digest.SaleIncome);
        Assert.Equal([(Purpose.Transfer, 5_000_000L), (Purpose.Sale, 250_000L)], digest.Income);
    }

    [Fact(DisplayName = "a_purpose_reads_differently_depending_which_way_the_money_went")]
    public void A_purpose_reads_differently_depending_which_way_the_money_went()
    {
        Assert.Equal("Sold to vendors", Purpose.Vendor.Label(true));
        Assert.Equal("Bought from vendors", Purpose.Vendor.Label(false));
        Assert.Equal(Purpose.Deposit, PurposeExtensions.FromToken("deposit"));
        Assert.Equal(Purpose.Unknown, PurposeExtensions.FromToken("something-new"));
    }

    [Fact(DisplayName = "crafting_is_counted_per_recipe_because_nothing_in_the_game_does")]
    public void Crafting_is_counted_per_recipe_because_nothing_in_the_game_does()
    {
        var digest = ASession(
            At(10, new Happening.Crafted(1, "Flask of Alchemical Chaos")),
            At(20, new Happening.Crafted(1, "Flask of Alchemical Chaos")),
            At(30, new Happening.Crafted(2, "Algari Mana Potion"))).Digest();
        Assert.Equal([("Flask of Alchemical Chaos", 2L), ("Algari Mana Potion", 1L)], digest.Crafted);
        Assert.True(digest.IsWorthWriting());
    }

    [Fact(DisplayName = "a_screenshot_is_matched_to_the_moment_that_asked_for_it")]
    public void A_screenshot_is_matched_to_the_moment_that_asked_for_it()
    {
        var session = ASession(At(600, new Happening.Pictured("achievement", "Loremaster of Kalimdor")));
        var asked = session.StartedAt + TimeSpan.FromSeconds(600);
        var taken = new[]
        {
            (session.StartedAt, "/wow/Screenshots/early.jpg"),
            (asked + TimeSpan.FromSeconds(2), "/wow/Screenshots/wanted.jpg"),
            (asked + TimeSpan.FromSeconds(600), "/wow/Screenshots/later.jpg"),
        };
        var picture = Assert.Single(session.Digest().Pictures(taken));
        Assert.Equal("/wow/Screenshots/wanted.jpg", picture.Path);
        Assert.Equal("achievement: Loremaster of Kalimdor", picture.Subject);
    }

    [Fact(DisplayName = "a_picture_taken_before_the_moment_is_somebody_elses")]
    public void A_picture_taken_before_the_moment_is_somebody_elses()
    {
        var session = ASession(At(600, new Happening.Pictured("rare", "Time-Lost Proto-Drake")));
        var asked = session.StartedAt + TimeSpan.FromSeconds(600);
        Assert.Empty(session.Digest().Pictures([(asked - TimeSpan.FromSeconds(3), "/before.jpg")]));
    }

    [Fact(DisplayName = "companions_are_deduplicated_because_the_roster_event_repeats")]
    public void Companions_are_deduplicated_because_the_roster_event_repeats()
    {
        var digest = ASession(At(1, new Happening.Alongside("Velkurai")), At(2, new Happening.Alongside("Velkurai")), At(3, new Happening.Alongside("Aeltor"))).Digest();
        Assert.Equal(["Aeltor", "Velkurai"], digest.Companions);
    }

    [Fact(DisplayName = "a_purse_that_went_down_says_so")]
    public void A_purse_that_went_down_says_so()
    {
        var spent = ASession() with { StartMoney = 500_000, EndMoney = 100_000 };
        Assert.Equal(-400_000, spent.Digest().Purse);
        Assert.Equal("−40g 00s 00c", Prose.Purse(-400_000));
        Assert.Equal("+40g 00s 00c", Prose.Purse(400_000));
    }

    [Fact(DisplayName = "money_reads_the_way_a_person_says_it")]
    public void Money_reads_the_way_a_person_says_it()
    {
        Assert.Equal("1,204g 30s 05c", Prose.Money(12_043_005));
        Assert.Equal("42s 05c", Prose.Money(4_205));
        Assert.Equal("7c", Prose.Money(7));
    }

    [Fact(DisplayName = "a_headline_summarises_before_anything_is_written")]
    public void A_headline_summarises_before_anything_is_written()
    {
        var digest = ASession(
            At(0, new Happening.Arrived("Nagrand", null, null)),
            At(60, new Happening.Arrived("Shadowmoon Valley", null, null)),
            At(120, new Happening.Completed(1, "A Task", null)),
            At(200, new Happening.Levelled(71, "Nagrand"))).Digest();
        Assert.Equal("Nagrand and 1 more · 1 quest · level 71", digest.Headline());
    }

    [Fact(DisplayName = "further_reading_links_out_and_never_fetches")]
    public void Further_reading_links_out_and_never_fetches()
    {
        var digest = ASession(At(0, new Happening.Arrived("Zul'Drak", null, null)), At(10, new Happening.Completed(12345, "The Storm King's Vengeance", null))).Digest();
        var links = digest.FurtherReading();
        Assert.Contains(links, link => link.Url == "https://www.wowhead.com/quest=12345");
        Assert.Contains(links, link => link.Url == "https://warcraft.wiki.gg/wiki/Zul%27Drak");
        Assert.Contains(links, link => link.Sort == Reading.Watch && link.Url.Contains("Nobbel87", StringComparison.Ordinal));
        Assert.All(links, link => Assert.DoesNotContain("api.php", link.Url, StringComparison.Ordinal));
    }

    [Fact(DisplayName = "a_session_is_identified_by_a_character_and_a_start")]
    public void A_session_is_identified_by_a_character_and_a_start()
    {
        Assert.Equal("emerald-dream/somechar@2026-08-03T19:00:00+00:00", ASession().Id.ToString());
    }

    [Fact(DisplayName = "a_spell_of_time_reads_in_hours_and_minutes")]
    public void A_spell_of_time_reads_in_hours_and_minutes()
    {
        Assert.Equal("45 min", Prose.Spell(TimeSpan.FromMinutes(45)));
        Assert.Equal("2 hr", Prose.Spell(TimeSpan.FromMinutes(120)));
        Assert.Equal("2 hr 30 min", Prose.Spell(TimeSpan.FromMinutes(150)));
    }

    [Fact(DisplayName = "a session's JSON is the shape the Rust writes")]
    public void A_sessions_json_is_the_shape_the_rust_writes()
    {
        // `session.json` travels between machines. This is serde's output for
        // a session with every kind of field in it, from the Rust build.
        const string rust = """{"character":{"realm_slug":"emerald-dream","name":"somechar"},"display_name":"Somechar","realm_name":"Emerald Dream","class":"Druid","race":"Tauren","faction":"Horde","started_at":"2026-08-03T19:00:00Z","ended_at":"2026-08-03T21:30:00Z","start_level":70,"end_level":71,"start_money":1000000,"end_money":1250000,"start_item_level":600,"end_item_level":604,"moments":[{"at":0,"what":{"Arrived":{"zone":"Nagrand","subzone":null,"map":107}}},{"at":12,"what":{"Coin":{"purpose":"Sale","amount":250000,"incoming":true}}},{"at":20,"what":{"Keystone":{"dungeon":"Ara-Kara","level":18,"in_time":true,"upgrades":2,"seconds":1620}}},{"at":30,"what":{"Acquired":{"kind":"Mount","name":"Ashes of Al'ar"}}},{"at":40,"what":{"Weather":{"kind":"Rain","zone":"Nagrand"}}},{"at":50,"what":{"Wiped":{"name":"Fyrakk","remaining":4}}},{"at":60,"what":{"WorldTier":{"tier":"Heroic"}}}],"risen":[["The Severed Threads",7]],"travelled":41288,"longest_fight":664}""";
        var session = JsonSerializer.Deserialize<Session>(rust)!;
        Assert.Equal("somechar", session.Character.Name);
        Assert.Equal(new Happening.Arrived("Nagrand", null, 107), session.Moments[0].What);
        Assert.Equal(new Happening.Coin(Purpose.Sale, 250000, true), session.Moments[1].What);
        Assert.Equal(new Happening.Keystone("Ara-Kara", 18, true, 2, 1620), session.Moments[2].What);
        Assert.Equal(new Happening.Acquired(Acquisition.Mount, "Ashes of Al'ar"), session.Moments[3].What);
        Assert.Equal(new Happening.Wiped("Fyrakk", 4), session.Moments[5].What);
        Assert.Equal(new Risen("The Severed Threads", 7), Assert.Single(session.Risen));

        var again = JsonSerializer.Serialize(session);
        Assert.Contains("\"what\":{\"Arrived\":{\"zone\":\"Nagrand\",\"subzone\":null,\"map\":107}}", again, StringComparison.Ordinal);
        Assert.Contains("\"risen\":[[\"The Severed Threads\",7]]", again, StringComparison.Ordinal);
        Assert.Equal(session, JsonSerializer.Deserialize<Session>(again));

        // A session from before the three session totals existed still reads.
        var older = JsonSerializer.Deserialize<Session>("""{"character":{"realm_slug":"a","name":"b"},"display_name":"B","realm_name":"A","class":"Druid","race":"Tauren","faction":"Horde","started_at":"2026-08-03T19:00:00Z","ended_at":"2026-08-03T21:30:00Z","start_level":1,"end_level":1,"start_money":0,"end_money":0,"start_item_level":0,"end_item_level":0,"moments":[]}""")!;
        Assert.Empty(older.Risen);
        Assert.Equal(0, older.Travelled);
    }
}
