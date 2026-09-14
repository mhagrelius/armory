using Armory.Roster;
using Xunit;

namespace Armory.Tests.Roster;

/// <summary>Ported from <c>core/src/cohort.rs</c>.</summary>
public sealed class CohortTests
{
    private static Armory.Roster.Roster ARoster() => new(
    [
        CharacterTests.Someone("Emerald Dream", "Somechar"),
        CharacterTests.Someone("Mannoroth", "Aeltor"),
        CharacterTests.Someone("Dalaran", "Moodivh"),
    ]);

    [Fact(DisplayName = "enrolment_is_opt_in_so_a_new_cohort_is_empty")]
    public void Enrolment_is_opt_in_so_a_new_cohort_is_empty()
    {
        Assert.True(new Cohort().IsEmpty);
        Assert.Empty(new Cohort().MembersOf(ARoster()));
    }

    [Fact(DisplayName = "everyone_not_enrolled_is_kept_as_a_bystander")]
    public void Everyone_not_enrolled_is_kept_as_a_bystander()
    {
        var cohort = new Cohort();
        cohort.Enrol(new CharacterKey("emerald-dream", "Somechar"));
        Assert.Single(cohort.MembersOf(ARoster()));
        Assert.Equal(2, cohort.Bystanders(ARoster()).Count);
    }

    [Fact(DisplayName = "members_come_back_in_roster_order_not_enrolment_order")]
    public void Members_come_back_in_roster_order_not_enrolment_order()
    {
        var cohort = new Cohort();
        cohort.Enrol(new CharacterKey("mannoroth", "Aeltor"));
        cohort.Enrol(new CharacterKey("dalaran", "Moodivh"));
        Assert.Equal(["Moodivh", "Aeltor"], cohort.MembersOf(ARoster()).Select(character => character.DisplayName));
    }

    [Fact(DisplayName = "toggling_reports_what_it_became")]
    public void Toggling_reports_what_it_became()
    {
        var cohort = new Cohort();
        var key = new CharacterKey("emerald-dream", "Somechar");
        Assert.True(cohort.Toggle(key));
        Assert.True(cohort.Contains(key));
        Assert.False(cohort.Toggle(key));
        Assert.False(cohort.Contains(key));
    }

    [Fact(DisplayName = "pruning_drops_characters_who_have_left_the_account")]
    public void Pruning_drops_characters_who_have_left_the_account()
    {
        var cohort = new Cohort([new CharacterKey("emerald-dream", "Somechar"), new CharacterKey("gone", "Ghost")]);
        cohort.Prune(ARoster());
        Assert.Equal(1, cohort.Count);
        Assert.True(cohort.Contains(new CharacterKey("emerald-dream", "Somechar")));
    }

    [Fact(DisplayName = "a cohort's JSON is the shape the Rust writes")]
    public void A_cohorts_json_is_the_shape_the_rust_writes()
    {
        // `run.cohort` is a JSON column that travels.
        var cohort = new Cohort([new CharacterKey("mannoroth", "Aeltor")]);
        var json = System.Text.Json.JsonSerializer.Serialize(cohort);
        Assert.Equal("""{"members":[{"realm_slug":"mannoroth","name":"aeltor"}]}""", json);
        Assert.Equal(cohort, System.Text.Json.JsonSerializer.Deserialize<Cohort>(json));
    }
}
