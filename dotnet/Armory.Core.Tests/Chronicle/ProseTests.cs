using Armory.Chronicle;
using Xunit;

namespace Armory.Tests.Chronicle;

/// <summary>Ported from the prose tests in <c>src/ui/almanac.rs</c>; the colour, stylesheet and Pango tests there are GTK's and have no counterpart.</summary>
public sealed class ProseTests
{
    [Fact(DisplayName = "gold_totals_are_grouped_so_they_can_be_read_rather_than_counted")]
    public void Gold_totals_are_grouped_so_they_can_be_read_rather_than_counted()
    {
        // The GTK almanac groups with a narrow no-break space. The Rust core's
        // own `chronicle::money` groups with a comma, and so does everything
        // this shell shows, so the separator here is the core's rather than
        // the almanac's.
        Assert.Equal("0", Prose.Thousands(0));
        Assert.Equal("999", Prose.Thousands(999));
        Assert.Equal("1,000", Prose.Thousands(1_000));
        Assert.Equal("12,345,678", Prose.Thousands(12_345_678));
    }

    [Fact(DisplayName = "a_count_of_one_takes_the_singular")]
    public void A_count_of_one_takes_the_singular()
    {
        // "1 quests" reads as a bug in the journal rather than as one quest.
        Assert.Equal("1 quest", Prose.Plural(1, "quest", "quests"));
        Assert.Equal("0 quests", Prose.Plural(0, "quest", "quests"));
        Assert.Equal("12 quests", Prose.Plural(12, "quest", "quests"));
    }

    [Fact(DisplayName = "a_headline_spells_its_number_until_a_figure_is_easier")]
    public void A_headline_spells_its_number_until_a_figure_is_easier()
    {
        Assert.Equal("Nothing", Prose.Spelled(0));
        Assert.Equal("Eleven", Prose.Spelled(11));
        Assert.Equal("Twenty", Prose.Spelled(20));
        Assert.Equal("21", Prose.Spelled(21));
    }
}
