using Armory.Blizzard;
using Armory.Client.Collections;
using Armory.Collections;
using Armory.Roster;
using Xunit;

namespace Armory.Tests.Client;

/// <summary>Ported from the tests in <c>src/ui/collection_page.rs</c> and <c>tests/artwork.rs</c>: the page's pure half.</summary>
public sealed class CollectionCardsTests
{
    private static Collectible ACollectible(long id, string name, Source source) => new()
    {
        Kind = Kind.Mount,
        Id = id,
        Name = name,
        Source = source,
        Display = 1000 + id,
        LinkId = id,
    };

    /// <summary>
    /// A toy, named and backed by an item. The two ids differ the way they
    /// really do: the toy box knows an item in the hundreds of thousands and
    /// the web API a toy in the low thousands.
    /// </summary>
    private static Collectible Toy(long id, long itemId, string name) => new()
    {
        Kind = Kind.Toy,
        Id = id,
        Name = name,
        Source = Source.Drop,
        // A toy has no creature display, which is the whole reason its picture has to be bought.
        Display = null,
        LinkId = itemId,
    };

    [Fact(DisplayName = "a_tooltip_prefers_the_journals_sentence_to_blizzards_one_word")]
    public void A_tooltip_prefers_the_journals_sentence_to_blizzards_one_word()
    {
        var entry = ACollectible(6, "Brown Horse", Source.Vendor);
        Assert.Equal("Brown Horse\nVendor", CollectionCards.Tooltip(entry));

        entry = entry with { Description = "Vendor: Unger Statforth\nZone: Wetlands" };
        Assert.Equal("Brown Horse\nVendor: Unger Statforth\nZone: Wetlands", CollectionCards.Tooltip(entry));
    }

    [Fact(DisplayName = "an_entry_with_no_source_says_so_rather_than_showing_the_word_unknown")]
    public void An_entry_with_no_source_says_so_rather_than_showing_the_word_unknown()
    {
        // "Unknown" reads as a property of the mount. It is a gap in Blizzard's
        // data, and the rail's caveat is where that is explained.
        var entry = ACollectible(12, "Reins of the Raven Lord", Source.Unknown);
        Assert.EndsWith("No source recorded", CollectionCards.Tooltip(entry), StringComparison.Ordinal);
        Assert.Equal("Unrecorded", SourceGroups.RailLabel(Source.Unknown));
    }

    [Fact(DisplayName = "every_source_has_a_group_to_be_read_under")]
    public void Every_source_has_a_group_to_be_read_under()
    {
        // A source missing from the grouping is a set of entries with no
        // heading over them and no row in the rail, reachable only by search,
        // which is exactly the silent hole the old "Any source" escape hatch
        // existed to prevent.
        foreach (var source in Enum.GetValues<Source>())
        {
            Assert.True(SourceGroups.Groups.Contains(source), $"{source} has no group");
        }
    }

    [Fact(DisplayName = "nothing_claims_a_chance_it_cannot_measure")]
    public void Nothing_claims_a_chance_it_cannot_measure()
    {
        // The whole of the deviation from the design, pinned: a drop has no
        // gold line because Armory has no idea what it drops at, and a vendor
        // does because "you buy it" is true without a rate.
        Assert.Null(SourceGroups.Certainty(Source.Drop));
        Assert.Null(SourceGroups.NoChance(Source.Drop));
        Assert.Null(SourceGroups.NoChance(Source.Promotion));
        Assert.NotNull(SourceGroups.NoChance(Source.Vendor));
        // And the ranking puts the ones with no chance in them first.
        Assert.True(SourceGroups.Certainty(Source.Vendor) < SourceGroups.Certainty(Source.Pvp));
    }

    [Fact(DisplayName = "the_reset_day_is_the_regions_own")]
    public void The_reset_day_is_the_regions_own()
    {
        // A collector plans a raid week around this, and quoting Tuesday at
        // somebody in Europe is worse than saying nothing.
        Assert.Equal("TUESDAY", CollectionCards.ResetDay(Region.Us));
        Assert.Equal("WEDNESDAY", CollectionCards.ResetDay(Region.Eu));
        Assert.Equal("THURSDAY", CollectionCards.ResetDay(Region.Kr));
    }

    [Fact(DisplayName = "the_art_budget_is_spent_on_what_the_grid_is_showing")]
    public void The_art_budget_is_spent_on_what_the_grid_is_showing()
    {
        // Deliberately the shape `collapse_toys` produces: descending item id,
        // in an order that has nothing to do with the alphabet.
        var catalogue = new[]
        {
            Toy(3, 300_003, "Zephyr"),
            Toy(2, 200_002, "Muradin's Favor"),
            Toy(1, 100_001, "Ancient Amber"),
        };
        var owned = new HashSet<long>();
        var shown = CollectionCards.Shown(catalogue, owned, Faction.Horde, Showing.Missing, "");

        // Two of the three, so the cap is doing something and this is not
        // simply everything in whatever order it was found.
        Assert.Equal([100_001L, 200_002L], CollectionCards.ArtWanted(shown, catalogue, _ => false, 2));

        // The model decides the order; it does not decide the extent. Nothing
        // here is owned, so the collected view is empty, and "Fetch Missing
        // Artwork" still has to mean every picture that is missing rather than
        // every picture on the tab somebody happens to be looking at.
        var collected = CollectionCards.Shown(catalogue, owned, Faction.Horde, Showing.Collected, "");
        Assert.Empty(collected);
        Assert.Equal(3, CollectionCards.ArtWanted(collected, catalogue, _ => false, int.MaxValue).Count);

        // And nothing is asked for twice, however many models it appears in.
        var asked = CollectionCards.ArtWanted(shown, catalogue, _ => false, int.MaxValue);
        Assert.Equal(3, asked.Count);

        // What a launch restored has to come off the list, or the first sync of
        // every session spends its whole budget re-earning URLs already in hand.
        Assert.Equal([200_002L, 300_003L], CollectionCards.ArtWanted(shown, catalogue, entry => entry.LinkId == 100_001, 120));
    }
}
