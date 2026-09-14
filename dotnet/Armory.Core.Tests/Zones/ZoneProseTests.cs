using Armory.Zones;
using Xunit;

namespace Armory.Tests.Zones;

/// <summary>Ported from the tests in <c>src/ui/zone_page.rs</c>.</summary>
public sealed class ZoneProseTests
{
    private static Visit Visit(params string[] deaths) => new()
    {
        Character = "Somechar",
        At = DateTimeOffset.UtcNow,
        Deaths = [.. deaths],
    };

    [Fact(DisplayName = "what_keeps_killing_you_is_counted_from_the_evenings")]
    public void What_keeps_killing_you_is_counted_from_the_evenings()
    {
        var place = new Place { Visits = [Visit("Gorian Warlock", "Warmaul Shaman"), Visit("Gorian Warlock")] };
        Assert.Equal([("Gorian Warlock", 2L), ("Warmaul Shaman", 1L)], ZoneProse.Killers(place));
    }

    [Fact(DisplayName = "a_tally_the_model_supplies_is_used_as_it_stands")]
    public void A_tally_the_model_supplies_is_used_as_it_stands()
    {
        var place = new Place { Killers = [("Something else", 9)], Visits = [Visit("Gorian Warlock")] };
        Assert.Equal([("Something else", 9L)], ZoneProse.Killers(place));
    }

    [Fact(DisplayName = "a_span_is_a_figure_and_not_a_sentence")]
    public void A_span_is_a_figure_and_not_a_sentence()
    {
        Assert.Equal("0m", ZoneProse.Span(0));
        Assert.Equal("41m", ZoneProse.Span(41 * 60));
        Assert.Equal("13h", ZoneProse.Span(13 * 3600 + 41 * 60));
    }

    [Fact(DisplayName = "an_evening_names_three_quests_and_counts_the_rest")]
    public void An_evening_names_three_quests_and_counts_the_rest()
    {
        string[] quests = ["A", "B", "C", "D", "E"];
        Assert.Equal("Turned in A, B, C and 2 more", ZoneProse.TurnedIn(quests));
        Assert.Equal("Turned in A, B", ZoneProse.TurnedIn(quests[..2]));
    }

    [Fact(DisplayName = "the same name twice is counted rather than repeated")]
    public void The_same_name_twice_is_counted_rather_than_repeated()
    {
        Assert.Equal([("Warlock", 2), ("Shaman", 1)], ZoneProse.Tallied(["Warlock", "Shaman", "Warlock"]));
        Assert.Equal("Died to Warlock ×2", ZoneProse.Counted("Died to", "Warlock", 2));
        Assert.Equal("Rare: Nok-Karosh", ZoneProse.Counted("Rare:", "Nok-Karosh", 1));
    }
}
