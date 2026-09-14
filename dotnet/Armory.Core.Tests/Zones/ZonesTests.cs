using Armory.Chronicle;
using Armory.Roster;
using Armory.Zones;
using Xunit;
using Written = Armory.Zones.Written;

namespace Armory.Tests.Zones;

/// <summary>Ported from <c>core/src/place.rs</c>, <c>core/src/adventure.rs</c> and the guide half of <c>store.rs</c>.</summary>
public sealed class ZonesTests
{
    private static Moment At(long at, Happening what) => new() { At = at, What = what };

    private static Session ASession(params Moment[] moments) => new()
    {
        Character = new CharacterKey("emerald-dream", "Somechar"),
        DisplayName = "Somechar",
        RealmName = "Emerald Dream",
        Class = "Druid",
        Race = "Tauren",
        Faction = Faction.Horde,
        StartedAt = DateTimeOffset.UtcNow,
        EndedAt = DateTimeOffset.UtcNow,
        StartLevel = 70,
        EndLevel = 70,
        StartItemLevel = 600,
        EndItemLevel = 600,
        Moments = [.. moments],
    };

    private static Guide AGuide()
    {
        var guide = new Guide();
        guide.Instances[63] = new Instance { Id = 63, Name = "Deadmines", Map = 291, Description = "The Defias Brotherhood have taken the mines.", Expansion = "DUNGEON", Encounters = [89, 90] };
        guide.Encounters[89] = new Encounter { Id = 89, Name = "Glubtok", Description = "An ogre mage hired as head foreman.", Loot = [2169, 5195] };
        guide.Encounters[90] = new Encounter { Id = 90, Name = "Helix Gearbreaker", Description = "A goblin with a bomb.", Loot = [5444] };
        return guide;
    }

    [Fact(DisplayName = "a_quest_belongs_to_the_zone_the_character_was_standing_in")]
    public void A_quest_belongs_to_the_zone_the_character_was_standing_in()
    {
        var session = ASession(
            At(0, new Happening.Arrived("Nagrand", null, 107)),
            At(10, new Happening.Completed(1, "In Nagrand", null)),
            At(20, new Happening.Arrived("Zangarmarsh", null, 102)),
            At(30, new Happening.Completed(2, "In Zangarmarsh", null)));
        Assert.Equal(["In Nagrand"], Places.Visited(session, 107)!.Quests);
        Assert.Equal(["In Zangarmarsh"], Places.Visited(session, 102)!.Quests);
        Assert.Null(Places.Visited(session, 999));
    }

    [Fact(DisplayName = "our_words_are_used_only_where_blizzards_are_missing_and_are_marked")]
    public void Our_words_are_used_only_where_blizzards_are_missing_and_are_marked()
    {
        var guide = new Guide();
        var karazhan = new Instance { Id = 745, Name = "Karazhan", Map = 532, Description = "" };
        guide.Instances[745] = karazhan;
        var written = new Dictionary<long, Written>
        {
            [745] = new Written { Instance = "Karazhan", Journal = 745, History = "Medivh's tower, and what became of it.", Assumes = "That you know who Medivh was." },
        };
        var ours = Places.DelveOf(karazhan, guide, written);
        Assert.True(ours.Ours);
        Assert.Equal("Medivh's tower, and what became of it.", ours.Description);
        Assert.NotNull(ours.Assumes);

        var theirs = Places.DelveOf(karazhan with { Description = "The Defias have taken the mines." }, guide, written);
        Assert.False(theirs.Ours);
        Assert.Equal("The Defias have taken the mines.", theirs.Description);
        Assert.Null(theirs.Assumes);
    }

    [Fact(DisplayName = "the_corpus_compiles_in_and_joins_on_the_map")]
    public void The_corpus_compiles_in_and_joins_on_the_map()
    {
        var corpus = Places.Corpus();
        Assert.True(corpus.Count > 100, $"{corpus.Count} zones");
        var mapped = corpus.Where(lore => lore.Map is not null).Select(lore => lore.Map!.Value).ToList();
        Assert.Equal(mapped.Count, mapped.Distinct().Count());
        Assert.Single(corpus, lore => lore.Zone == "Nagrand");

        var raids = Places.Unwritten();
        Assert.Equal(21, raids.Count);
        Assert.NotNull(raids[745].Assumes);
    }

    [Fact(DisplayName = "only_what_can_actually_be_sold_is_worth_looking_for")]
    public void Only_what_can_actually_be_sold_is_worth_looking_for()
    {
        var guide = new Guide();
        guide.Instances[745] = new Instance { Id = 745, Name = "Karazhan", Map = 532, Description = "…", Encounters = [1] };
        guide.Encounters[1] = new Encounter { Id = 1, Name = "Attumen the Huntsman, ", Loot = [10, 20, 30, 40] };
        var items = new Dictionary<long, ItemFact>
        {
            [10] = new ItemFact("Fiery Warhorse's Reins", false),
            [20] = new ItemFact("Worn Cloak", true),
            [30] = new ItemFact("Rich Cloak", true),
        };
        var market = new Dictionary<long, (long, long)> { [10] = (5_000_000, 1), [20] = (1_000, 9), [30] = (90_000, 2), [40] = (400_000, 1) };
        var spoils = Places.SpoilsOf(guide.Instances[745], guide, items, market);
        Assert.Equal(2, spoils.Count);
        Assert.Equal("Rich Cloak", spoils[0].Name);
        Assert.Equal("Attumen the Huntsman", spoils[0].From);
        Assert.Equal("Worn Cloak", spoils[1].Name);
    }

    [Fact(DisplayName = "a_place_with_nothing_in_it_is_not_worth_a_page")]
    public void A_place_with_nothing_in_it_is_not_worth_a_page()
    {
        Assert.False(new Place().IsWorthShowing);
        Assert.True(new Place { Spent = 3_600 }.IsWorthShowing);
        Assert.True(new Place { Lore = new Lore() }.IsWorthShowing);
    }

    [Fact(DisplayName = "an_instance_is_found_by_the_map_somebody_is_standing_on")]
    public void An_instance_is_found_by_the_map_somebody_is_standing_on()
    {
        Assert.Equal("Deadmines", AGuide().At(291)?.Name);
        Assert.Null(AGuide().At(37));
    }

    [Fact(DisplayName = "an_item_is_traced_back_to_the_boss_and_the_place")]
    public void An_item_is_traced_back_to_the_boss_and_the_place()
    {
        var (instance, encounter) = AGuide().Drops(5195)!.Value;
        Assert.Equal("Deadmines", instance.Name);
        Assert.Equal("Glubtok", encounter.Name);
        Assert.Null(AGuide().Drops(99_999));
    }

    [Fact(DisplayName = "the_loot_set_is_what_the_market_is_filtered_against")]
    public void The_loot_set_is_what_the_market_is_filtered_against()
    {
        Assert.Equal(3, AGuide().Loot().Count);
    }

    [Fact(DisplayName = "the guide round trips through the store and says what is missing")]
    public void The_guide_round_trips_through_the_store_and_says_what_is_missing()
    {
        using var store = Armory.Store.Store.InMemory();
        var guide = AGuide();
        store.SaveInstance(guide.Instances[63]);
        store.SaveEncounter(guide.Encounters[89]);

        var held = store.GuideHeld().Value;
        Assert.Equal(guide.Instances[63], held.Instances[63]);
        Assert.Equal(guide.Encounters[89], held.Encounters[89]);

        var (instances, encounters) = store.GuideGaps([(63, "Deadmines"), (745, "Karazhan")]).Value;
        Assert.Equal([745L], instances);
        Assert.Equal([90L], encounters);
    }
}
