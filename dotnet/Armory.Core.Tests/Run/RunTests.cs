using System.Text.Json;
using Armory.Roster;
using Armory.Run;
using Xunit;

namespace Armory.Tests.Run;

/// <summary>Ported from <c>core/src/run.rs</c>.</summary>
public sealed class RunTests
{
    private static CharacterKey Key(string realm, string name) => new(realm, name);

    private static Cohort ACohort() => new([Key("emerald-dream", "Somechar")]);

    private static readonly DateTimeOffset BaselineAt = DateTimeOffset.Parse("2026-08-01T00:00:00Z", System.Globalization.CultureInfo.InvariantCulture);
    private static readonly DateTimeOffset Before = DateTimeOffset.Parse("2016-03-04T00:00:00Z", System.Globalization.CultureInfo.InvariantCulture);
    private static readonly DateTimeOffset After = DateTimeOffset.Parse("2026-08-02T00:00:00Z", System.Globalization.CultureInfo.InvariantCulture);

    private static Goal PoisonedGoal(Bucket bucket) => new()
    {
        AchievementId = 1,
        Standing = new Standing.Poisoned(Key("mannoroth", "Aeltor")),
        Bucket = bucket,
    };

    private static Armory.Run.Run ARun(params Goal[] goals) => new()
    {
        Name = "Fresh start",
        Baseline = new Baseline { TakenAt = BaselineAt },
        Cohort = ACohort(),
        Goals = [.. goals],
    };

    [Fact(DisplayName = "an_unearned_achievement_needs_no_special_handling")]
    public void An_unearned_achievement_needs_no_special_handling()
    {
        var standing = Standing.Classify(null, null, ACohort(), BaselineAt);
        Assert.IsType<Standing.Unearned>(standing);
        Assert.False(standing.IsPoisoned);
    }

    [Fact(DisplayName = "anything_earned_since_the_baseline_belongs_to_the_run")]
    public void Anything_earned_since_the_baseline_belongs_to_the_run()
    {
        var standing = Standing.Classify(After, null, ACohort(), BaselineAt);
        Assert.True(standing.IsSettled);
        Assert.False(standing.IsPoisoned);
    }

    [Fact(DisplayName = "an_enrolled_character_having_earned_it_settles_it")]
    public void An_enrolled_character_having_earned_it_settles_it()
    {
        var standing = Standing.Classify(Before, Key("emerald-dream", "Somechar"), ACohort(), BaselineAt);
        Assert.True(standing.IsSettled);
        Assert.False(standing.IsPoisoned);
    }

    [Fact(DisplayName = "an_outsider_having_earned_it_poisons_it")]
    public void An_outsider_having_earned_it_poisons_it()
    {
        var standing = Standing.Classify(Before, Key("mannoroth", "Aeltor"), ACohort(), BaselineAt);
        Assert.True(standing.IsPoisoned);
        Assert.False(standing.IsSettled);
    }

    [Fact(DisplayName = "without_attribution_everything_already_earned_is_assumed_poisoned")]
    public void Without_attribution_everything_already_earned_is_assumed_poisoned()
    {
        Assert.Equal(new Standing.Poisoned(null), Standing.Classify(Before, null, ACohort(), BaselineAt));
    }

    [Fact(DisplayName = "an_excluded_goal_leaves_the_denominator_rather_than_sitting_in_it_as_a_zero")]
    public void An_excluded_goal_leaves_the_denominator_rather_than_sitting_in_it_as_a_zero()
    {
        var run = ARun(PoisonedGoal(new Bucket.Excluded(Exclusion.AlreadyOwned)), PoisonedGoal(new Bucket.Attestable()));
        var progress = run.Progress();
        Assert.Equal(1, progress.Excluded);
        Assert.Equal(1, progress.Counted);
        Assert.Equal(1, progress.AwaitingAttestation);
        Assert.Equal(0, progress.Done);
    }

    [Fact(DisplayName = "an_excluded_goal_is_gone_rather_than_done")]
    public void An_excluded_goal_is_gone_rather_than_done()
    {
        var goal = PoisonedGoal(new Bucket.Excluded(Exclusion.AlreadyOwned));
        Assert.False(goal.IsDone);
        Assert.False(goal.Counts);
    }

    [Fact(DisplayName = "attestation_is_what_completes_a_goal_nothing_can_measure")]
    public void Attestation_is_what_completes_a_goal_nothing_can_measure()
    {
        var goal = PoisonedGoal(new Bucket.Attestable());
        Assert.False(goal.IsDone);
        Assert.True((goal with { Attestation = new Attestation { Character = Key("emerald-dream", "Somechar"), At = After } }).IsDone);
    }

    [Fact(DisplayName = "an_observable_goal_completes_on_its_evaluation")]
    public void An_observable_goal_completes_on_its_evaluation()
    {
        var goal = PoisonedGoal(new Bucket.Observable()) with { Evaluation = new Evaluation(10, 10, true, false) };
        Assert.True(goal.IsDone);
        Assert.Equal(1.0, goal.Fraction());
    }

    [Fact(DisplayName = "an_inherited_evaluation_draws_no_progress_bar")]
    public void An_inherited_evaluation_draws_no_progress_bar()
    {
        var goal = PoisonedGoal(new Bucket.Observable()) with { Evaluation = new Evaluation(8, 10, true, true) };
        Assert.False(goal.IsDone);
        Assert.Null(goal.Fraction());
    }

    [Fact(DisplayName = "an_unobservable_evaluation_draws_no_progress_bar_either")]
    public void An_unobservable_evaluation_draws_no_progress_bar_either()
    {
        var goal = PoisonedGoal(new Bucket.Observable()) with { Evaluation = new Evaluation(3, 4, false, false) };
        Assert.Null(goal.Fraction());
    }

    [Fact(DisplayName = "only_poisoned_goals_reach_the_expensive_path")]
    public void Only_poisoned_goals_reach_the_expensive_path()
    {
        var run = ARun(new Goal { AchievementId = 1, Standing = new Standing.Unearned(), Bucket = new Bucket.Observable() }, PoisonedGoal(new Bucket.Observable()));
        Assert.Single(run.Poisoned());
    }

    [Fact(DisplayName = "a goal's columns and a baseline are the JSON the Rust writes")]
    public void A_goals_columns_and_a_baseline_are_the_json_the_rust_writes()
    {
        // `goal.standing`, `goal.bucket`, `run.baseline` and `run.cohort` are
        // JSON columns that travel.
        Assert.Equal("\"Unearned\"", JsonSerializer.Serialize<Standing>(new Standing.Unearned()));
        Assert.Equal("""{"Poisoned":{"by":null}}""", JsonSerializer.Serialize<Standing>(new Standing.Poisoned(null)));
        Assert.Equal("""{"EarnedByCohort":{"by":{"realm_slug":"mannoroth","name":"aeltor"}}}""", JsonSerializer.Serialize<Standing>(new Standing.EarnedByCohort(Key("mannoroth", "Aeltor"))));
        Assert.Equal("""{"EarnedDuringRun":{"at":"2026-08-02T00:00:00Z"}}""", JsonSerializer.Serialize<Standing>(new Standing.EarnedDuringRun(After)));
        Assert.Equal("\"Observable\"", JsonSerializer.Serialize<Bucket>(new Bucket.Observable()));
        Assert.Equal("""{"Excluded":"AlreadyOwned"}""", JsonSerializer.Serialize<Bucket>(new Bucket.Excluded(Exclusion.AlreadyOwned)));

        var baseline = new Baseline { TakenAt = BaselineAt, Collected = [6], Completed = [new Completed(1234, Before)] };
        var json = JsonSerializer.Serialize(baseline);
        Assert.Equal("""{"taken_at":"2026-08-01T00:00:00+00:00","collected":[6],"completed":[[1234,"2016-03-04T00:00:00Z"]]}""", json);
        Assert.Equal(baseline, JsonSerializer.Deserialize<Baseline>(json));

        foreach (var standing in new Standing[] { new Standing.Unearned(), new Standing.Poisoned(null), new Standing.Poisoned(Key("a", "B")), new Standing.EarnedDuringRun(After) })
        {
            Assert.Equal(standing, JsonSerializer.Deserialize<Standing>(JsonSerializer.Serialize(standing)));
        }
    }
}
