using Armory.Provenance;
using Xunit;

namespace Armory.Tests.Provenance;

/// <summary>Ported from <c>core/src/provenance.rs</c>.</summary>
public sealed class ProvenanceTests
{
    [Fact(DisplayName = "a_currency_that_cannot_be_transferred_was_earned_here")]
    public void A_currency_that_cannot_be_transferred_was_earned_here()
    {
        var held = new EarnedCurrency { Gained = 400, Earned = 0, TracksEarned = false, AccountWide = false, Transferable = false };
        Assert.Equal(Origin.Earned, held.Origin);
        Assert.Equal(400, held.Creditable());
    }

    [Fact(DisplayName = "a_transferable_currency_is_believed_where_the_game_counts_earnings")]
    public void A_transferable_currency_is_believed_where_the_game_counts_earnings()
    {
        var mixed = new EarnedCurrency { Gained = 1_000, Earned = 600, TracksEarned = true, AccountWide = true, Transferable = true };
        Assert.Equal(Origin.Transferred, mixed.Origin);
        Assert.Equal(600, mixed.Creditable());

        var honest = mixed with { Earned = 1_000 };
        Assert.Equal(Origin.Earned, honest.Origin);
        Assert.Equal(1_000, honest.Creditable());
    }

    [Fact(DisplayName = "a_transferable_currency_with_no_earned_total_is_admitted_as_unknown")]
    public void A_transferable_currency_with_no_earned_total_is_admitted_as_unknown()
    {
        var ambiguous = new EarnedCurrency { Gained = 500, Earned = 0, TracksEarned = false, AccountWide = true, Transferable = true };
        Assert.Equal(Origin.Unclear, ambiguous.Origin);
        Assert.Equal(0, ambiguous.Creditable());
        Assert.False(Origin.Unclear.Counts());
    }

    [Fact(DisplayName = "a_currency_that_never_moved_was_already_there")]
    public void A_currency_that_never_moved_was_already_there()
    {
        Assert.Equal(Origin.Existing, new EarnedCurrency().Origin);
        Assert.Equal(0, new EarnedCurrency().Creditable());
    }

    [Fact(DisplayName = "earned_reputation_reaches_a_standing_of_its_own")]
    public void Earned_reputation_reaches_a_standing_of_its_own()
    {
        Assert.Equal((8, "Exalted"), Standings.StandingEarned(new EarnedReputation { Points = 42_000, AccountWide = true }));
        Assert.Equal((6, "Honored"), Standings.StandingEarned(new EarnedReputation { Points = 12_000 }));
        Assert.Equal((4, "Neutral"), Standings.StandingEarned(new EarnedReputation()));
    }

    [Fact(DisplayName = "renown_is_counted_in_levels_because_that_is_its_shape")]
    public void Renown_is_counted_in_levels_because_that_is_its_shape()
    {
        var renowned = new EarnedReputation { Points = 2_400, Renown = 9, RenownSeen = 25, AccountWide = true };
        Assert.Equal((9, "Renown"), Standings.StandingEarned(renowned));
        Assert.Equal(25, renowned.RenownSeen);
        Assert.Null(Standings.FractionEarned(renowned));
    }

    [Fact(DisplayName = "a_fraction_measures_progress_through_the_standing_being_worked_on")]
    public void A_fraction_measures_progress_through_the_standing_being_worked_on()
    {
        var halfway = new EarnedReputation { Points = 6_000 };
        Assert.Equal((5, "Friendly"), Standings.StandingEarned(halfway));
        Assert.Equal(0.5, Standings.FractionEarned(halfway));
    }

    [Fact(DisplayName = "a_character_who_has_done_nothing_with_a_faction_says_so")]
    public void A_character_who_has_done_nothing_with_a_faction_says_so()
    {
        var earned = new Earned();
        Assert.False(earned.HasTouched(2_600));
        earned.Reputation[2_600] = new EarnedReputation { Points = 250 };
        Assert.True(earned.HasTouched(2_600));
    }
}
