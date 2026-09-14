using Armory.Collections;
using Armory.Roster;
using Xunit;

namespace Armory.Tests.Collections;

/// <summary>Ported from the tests in <c>src/ui/collectible_dialog.rs</c>.</summary>
public sealed class LinksTests
{
    private static Collectible Entry() => new()
    {
        Kind = Kind.Mount,
        Id = 337,
        Name = "Rivendare's Deathcharger",
        Source = Source.Drop,
        Description = "Drop: Lord Aurius Rivendare\nLocation: Stratholme",
        Flavour = "A skeletal steed.",
        Icon = 132250,
        Display = 10995,
        LinkId = 17481,
    };

    [Fact(DisplayName = "a_mount_links_by_its_spell")]
    public void A_mount_links_by_its_spell()
    {
        Assert.Equal("https://www.wowhead.com/spell=17481", Entry().WowheadUrl());
    }

    [Fact(DisplayName = "the_wiki_link_searches_by_name_with_the_apostrophe_encoded")]
    public void The_wiki_link_searches_by_name_with_the_apostrophe_encoded()
    {
        // A raw apostrophe in a query string is a link that lands nowhere.
        var url = Entry().WikiUrl();
        Assert.Contains("Rivendare%27s%20Deathcharger", url, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "a_faction_locked_mount_is_not_missing_from_the_other_factions_collection")]
    public void A_faction_locked_mount_is_not_missing_from_the_other_factions_collection()
    {
        // Counting it as missing overstates the backlog by a few hundred, and
        // only the game knows about the restriction at all.
        var hordeOnly = Entry() with { Faction = Faction.Horde };

        Assert.True(hordeOnly.ObtainableBy(Faction.Horde));
        Assert.False(hordeOnly.ObtainableBy(Faction.Alliance));
        // No restriction means anyone.
        Assert.True(Entry().ObtainableBy(Faction.Alliance));
    }

    [Fact(DisplayName = "a_promotional_mount_is_obtainable_by_nobody")]
    public void A_promotional_mount_is_obtainable_by_nobody()
    {
        var promotion = Entry() with { Source = Source.Promotion };
        Assert.False(promotion.ObtainableBy(Faction.Horde));
    }
}
