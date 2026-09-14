using Armory.Roster;
using Armory.Tally;
using Xunit;

namespace Armory.Tests.Tally;

/// <summary>Ported from <c>core/src/tally.rs</c>.</summary>
public sealed class TallyTests
{
    private static Armory.Tally.Tally ATally(Counting kind, string label, long count) =>
        new() { Kind = kind, Key = label, Label = label, Count = count };

    private static Armory.Tally.Tally Attempt(string label, long count) => ATally(Counting.Attempt, label, count);

    [Fact(DisplayName = "one_kind_is_picked_out_of_the_flat_list_biggest_first")]
    public void One_kind_is_picked_out_of_the_flat_list_biggest_first()
    {
        var held = new[]
        {
            ATally(Counting.Recipe, "Algari Mana Potion", 6),
            ATally(Counting.Companion, "Velkurai", 34),
            ATally(Counting.Recipe, "Flask of Alchemical Chaos", 412),
        };
        var made = Counters.Of(held, Counting.Recipe);
        Assert.Equal(2, made.Count);
        Assert.Equal("Flask of Alchemical Chaos", made[0].Label);
        Assert.Equal("Algari Mana Potion", made[1].Label);
        Assert.Empty(Counters.Of(held, Counting.Zone));
    }

    [Fact(DisplayName = "a_kind_this_version_does_not_know_is_refused_rather_than_guessed")]
    public void A_kind_this_version_does_not_know_is_refused_rather_than_guessed()
    {
        Assert.Equal(Counting.Recipe, CountingExtensions.FromToken("recipe"));
        Assert.Null(CountingExtensions.FromToken("delve-tier"));
    }

    [Fact(DisplayName = "time_and_distance_are_said_the_way_a_person_says_them")]
    public void Time_and_distance_are_said_the_way_a_person_says_them()
    {
        Assert.Equal("10 seconds", Counters.Spent(10));
        Assert.Equal("1 second", Counters.Spent(1));
        Assert.Equal("59 seconds", Counters.Spent(59));
        Assert.Equal("1 minute", Counters.Spent(60));
        Assert.Equal("1 minute", Counters.Spent(90));
        Assert.Equal("1 hour", Counters.Spent(3_600));
        Assert.Equal("1 hr 30 min", Counters.Spent(5_400));
        Assert.Equal("55 hours", Counters.Spent(200_000));

        Assert.Equal("400 yards", Counters.Far(400));
        Assert.Equal("2.0 miles", Counters.Far(3_520));
        Assert.Equal("57 miles", Counters.Far(100_000));
    }

    [Fact(DisplayName = "a_drop_is_joined_to_the_boss_the_addon_counted_pulls_at")]
    public void A_drop_is_joined_to_the_boss_the_addon_counted_pulls_at()
    {
        var tallies = new[] { Attempt("Attumen the Huntsman", 31), Attempt("Vexie", 4) };
        var (fought, tries) = Counters.AttemptsAt("Drop: Attumen the Huntsman, Karazhan", tallies)!.Value;
        Assert.Equal("Attumen the Huntsman", fought);
        Assert.Equal(31, tries);
    }

    [Fact(DisplayName = "the_longest_name_wins_so_one_boss_is_not_credited_to_another")]
    public void The_longest_name_wins_so_one_boss_is_not_credited_to_another()
    {
        var tallies = new[] { Attempt("Halion", 9), Attempt("Halion the Twilight Destroyer", 2) };
        var (fought, tries) = Counters.AttemptsAt("Drop: Halion the Twilight Destroyer, Ruby Sanctum", tallies)!.Value;
        Assert.Equal("Halion the Twilight Destroyer", fought);
        Assert.Equal(2, tries);
    }

    [Fact(DisplayName = "a_short_name_is_not_matched_at_all")]
    public void A_short_name_is_not_matched_at_all()
    {
        Assert.Null(Counters.AttemptsAt("Drop: Sickly Gazelle, Mulgore", [Attempt("Ick", 40)]));
    }

    [Fact(DisplayName = "a_collectible_with_no_sentence_has_nothing_to_join_on")]
    public void A_collectible_with_no_sentence_has_nothing_to_join_on()
    {
        Assert.Null(Counters.AttemptsAt(null, [Attempt("Attumen the Huntsman", 31)]));
    }

    [Fact(DisplayName = "two_characters_raiding_the_same_boss_is_twice_the_rolls")]
    public void Two_characters_raiding_the_same_boss_is_twice_the_rolls()
    {
        var tallies = new Tallies
        {
            [new CharacterKey("emerald-dream", "Somechar")] = [Attempt("Vexie", 7)],
            [new CharacterKey("mannoroth", "Aeltor")] = [Attempt("Vexie", 5)],
        };
        var merged = Counters.AccountAttempts(tallies);
        Assert.Equal(12, Assert.Single(merged).Count);
    }
}
