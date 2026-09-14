using Armory.Blizzard;
using Armory.Client.Factions;
using Armory.Provenance;
using Xunit;

namespace Armory.Tests.Client;

/// <summary>Ported from the tests in <c>src/ui/reputations_page.rs</c>.</summary>
public sealed class ReadingsTests
{
    private static FactionStanding Standing(bool inherited) => new()
    {
        Faction = 2_600,
        Name = "The Assembly of the Deeps",
        Tier = "Renown 19",
        Value = 4_200,
        Max = 8_500,
        Renown = 19,
        Inherited = inherited,
    };

    private static Earned Watched(long points, long renown)
    {
        var earned = new Earned();
        earned.Reputation[2_600] = new EarnedReputation { Points = points, Renown = renown, RenownSeen = 19, AccountWide = true };
        return earned;
    }

    [Fact(DisplayName = "a_standing_nobody_watched_is_never_drawn_as_a_bar")]
    public void A_standing_nobody_watched_is_never_drawn_as_a_bar()
    {
        // Without the addon there is no observation. A floor is not a
        // measurement, and a bar over one reads as the measurement it is not.
        Assert.Equal(Reading.Unclear, Readings.Of(Standing(false), null));
        Assert.False(Readings.Of(Standing(false), null).IsMeasured());
    }

    [Fact(DisplayName = "a_watched_zero_is_not_the_same_fact_as_an_unwatched_one")]
    public void A_watched_zero_is_not_the_same_fact_as_an_unwatched_one()
    {
        var nothing = new Earned();
        Assert.Equal(Reading.Nothing, Readings.Of(Standing(false), nothing));
        Assert.True(Readings.Of(Standing(false), nothing).IsMeasured());
    }

    [Fact(DisplayName = "an_inherited_standing_carries_no_gold_at_all")]
    public void An_inherited_standing_carries_no_gold_at_all()
    {
        Assert.Equal(Reading.Inherited, Readings.Of(Standing(true), new Earned()));
        // And the same with no addon: the flag is a positive fact and outranks the silence.
        Assert.Equal(Reading.Inherited, Readings.Of(Standing(true), null));
    }

    [Fact(DisplayName = "an_inherited_faction_somebody_is_grinding_anyway_is_the_interesting_row")]
    public void An_inherited_faction_somebody_is_grinding_anyway_is_the_interesting_row()
    {
        var mine = Watched(2_400, 9);
        Assert.Equal(Reading.Earned, Readings.Of(Standing(true), mine));
        Assert.Equal(9.0 / 19.0, Readings.Share(Standing(true), mine.With(2_600)));
    }

    [Fact(DisplayName = "the_share_is_of_where_the_account_stands_and_never_more")]
    public void The_share_is_of_where_the_account_stands_and_never_more()
    {
        var mine = Watched(0, 40);
        Assert.Equal(1.0, Readings.Share(Standing(false), mine.With(2_600)));
    }

    [Fact(DisplayName = "a_classic_ladder_is_measured_in_ranks")]
    public void A_classic_ladder_is_measured_in_ranks()
    {
        var classic = Standing(false) with { Renown = 0, Tier = "Exalted", Max = 0, Value = 0 };
        // Exalted from nothing is the whole ladder.
        Assert.Equal(1.0, Readings.Share(classic, new EarnedReputation { Points = 42_000 }));
        // Honored is two of the four ranks above Neutral, and the partial
        // progress towards Revered rides on top of it.
        Assert.Equal(0.5, Readings.Share(classic, new EarnedReputation { Points = 9_000 }));
        Assert.Equal(0.0, Readings.Share(classic, new EarnedReputation()));
    }

    [Fact(DisplayName = "the_cohort_counts_are_three_separate_claims")]
    public void The_cohort_counts_are_three_separate_claims()
    {
        var standings = new[] { Standing(false), Standing(true) };
        // Both factions share an id here, so the watched one answers for both.
        Assert.Equal((2, 0, 0), Readings.Tally(standings, Watched(2_400, 9)));
        // With nothing watching, the flag is all that is left to go on.
        Assert.Equal((0, 1, 1), Readings.Tally(standings, null));
    }

    [Fact(DisplayName = "the badge is this character's own standing, and the footer says the account's")]
    public void The_badge_is_this_characters_own_standing()
    {
        Assert.Equal("RENOWN 9", Readings.Badge(Reading.Earned, Watched(2_400, 9).With(2_600)));
        Assert.Equal("HONORED", Readings.Badge(Reading.Earned, new EarnedReputation { Points = 9_000 }));
        Assert.Equal("INHERITED", Readings.Badge(Reading.Inherited, new EarnedReputation()));
        Assert.Equal("Renown 19", Readings.Tier(Standing(false)));
        Assert.Equal("Nothing watched being earned by Somechar yet", Readings.EarnedLine(Standing(false), new EarnedReputation(), "Somechar"));
        Assert.Equal("9 of 19 renown earned by Somechar", Readings.EarnedLine(Standing(false), Watched(0, 9).With(2_600), "Somechar"));
    }
}
