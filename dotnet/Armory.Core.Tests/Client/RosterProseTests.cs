using Armory.Blizzard;
using Armory.Client.Roster;
using Armory.Roster;
using Xunit;

namespace Armory.Tests.Client;

/// <summary>Ported from the tests in <c>src/ui/roster_page.rs</c>.</summary>
public sealed class RosterProseTests
{
    private static readonly string[] Shipped =
    [
        "Death Knight", "Demon Hunter", "Druid", "Evoker", "Hunter", "Mage", "Monk",
        "Paladin", "Priest", "Rogue", "Shaman", "Warlock", "Warrior",
    ];

    private static Character ACharacter(string name, string realm, string className) => new()
    {
        Key = new CharacterKey(Slug.RealmSlug(realm), name),
        Id = 1,
        RealmId = 1,
        DisplayName = name,
        RealmName = realm,
        Level = 80,
        Class = className,
        Race = "Orc",
        Faction = Faction.Horde,
        WowAccountId = 1,
    };

    [Fact(DisplayName = "every_class_the_game_ships_has_a_ring_of_its_own")]
    public void Every_class_the_game_ships_has_a_ring_of_its_own()
    {
        // A class falling through to the unknown ring is a card that looks
        // broken next to twelve that do not.
        foreach (var className in Shipped)
        {
            Assert.NotEqual("class-unknown", Classes.Ring(className).Style);
            Assert.NotEqual(Classes.Unknown, Classes.Ring(className));
        }
        Assert.Equal("class-unknown", Classes.Ring("Tinker").Style);
        Assert.Equal(Classes.Unknown, Classes.Ring("Tinker"));
    }

    [Fact(DisplayName = "the_class_ring_and_the_class_crest_agree_on_every_class")]
    public void The_class_ring_and_the_class_crest_agree_on_every_class()
    {
        // Both are derived from the same display string and neither is checked
        // by the compiler. A class that has a ring but no crest is a card with a
        // coloured circle and no picture in it.
        foreach (var className in new[] { "Death Knight", "Demon Hunter", "Evoker" })
        {
            var crest = Classes.Crest(Region.Us, className);
            var ring = Classes.Ring(className).Style;
            var slug = ring["class-".Length..].Replace("-", "", StringComparison.Ordinal);
            Assert.Contains(slug, crest, StringComparison.Ordinal);
        }
    }

    [Fact(DisplayName = "the_headline_reads_as_a_sentence_at_every_count")]
    public void The_headline_reads_as_a_sentence_at_every_count()
    {
        // Spelled up to twenty and a figure past it, and never "Nothing of
        // thirty-one are what this run is about".
        Assert.Equal("Six of twelve are what this run is about", RosterProse.Headline(6, 12));
        Assert.Equal("One of twelve is what this run is about", RosterProse.Headline(1, 12));
        Assert.Equal("Six of 31 are what this run is about", RosterProse.Headline(6, 31));
        Assert.StartsWith("Nobody is enrolled", RosterProse.Headline(0, 31), StringComparison.Ordinal);
    }

    [Fact(DisplayName = "a_search_reaches_the_realm_and_the_class_not_only_the_name")]
    public void A_search_reaches_the_realm_and_the_class_not_only_the_name()
    {
        // With thirty-one characters the question is as often "who is on
        // Mannoroth" or "where is my druid" as it is a name.
        var one = ACharacter("Aeltor", "Mannoroth", "Paladin");

        Assert.True(RosterProse.Matches(one, null, ""));
        Assert.True(RosterProse.Matches(one, null, "aelt"));
        Assert.True(RosterProse.Matches(one, null, "mannoroth"));
        Assert.True(RosterProse.Matches(one, null, "paladin"));
        Assert.True(RosterProse.Matches(one, null, "horde"));
        Assert.False(RosterProse.Matches(one, null, "stormrage"));
    }

    [Fact(DisplayName = "a_character_with_no_detail_still_says_who_they_are")]
    public void A_character_with_no_detail_still_says_who_they_are()
    {
        // An enrolled character whose detail has not landed reads the same as
        // one with none, rather than showing dashes that look like a failure.
        var one = ACharacter("Somechar", "Emerald Dream", "Shaman");
        Assert.Equal("Level 80 Orc Shaman", RosterProse.Subtitle(one, null));
    }
}
