using System.Globalization;
using Armory.Blizzard;
using Armory.Roster;
using Armory.Run;
using Xunit;

namespace Armory.Tests.Run;

/// <summary>Ported from <c>core/src/plan.rs</c>.</summary>
public sealed class PlanTests
{
    private static DateTimeOffset At(string text) => DateTimeOffset.Parse(text, CultureInfo.InvariantCulture);

    private static Baseline ABaseline() => new() { TakenAt = At("2026-06-01T00:00:00Z") };

    private static CharacterKey Somechar => new("emerald-dream", "Somechar");

    private static CharacterKey Aeltor => new("mannoroth", "Aeltor");

    private static Cohort ACohort() => new([Somechar]);

    /// <summary>An achievement made of three quests.</summary>
    private static AchievementProgress QuestAchievement(long id, string? completedAt) => new()
    {
        Id = id,
        CompletedAt = completedAt is null ? null : At(completedAt),
        Criteria = new Criterion
        {
            Id = 900,
            Required = 0,
            Children = [Criterion.Leaf(901, CriterionKind.Unknown, 1), Criterion.Leaf(902, CriterionKind.Unknown, 1), Criterion.Leaf(903, CriterionKind.Unknown, 1)],
        },
    };

    private static Dictionary<long, CriterionKind> QuestCriteria() => new()
    {
        [901] = CriterionKind.Quest(1),
        [902] = CriterionKind.Quest(2),
        [903] = CriterionKind.Quest(3),
    };

    private static Dictionary<CharacterKey, PrimaryData> Primary(params long[] quests) => new()
    {
        [Somechar] = new PrimaryData { Quests = [.. quests] },
    };

    [Fact(DisplayName = "an_unearned_achievement_is_planned_without_touching_the_expensive_path")]
    public void An_unearned_achievement_is_planned_without_touching_the_expensive_path()
    {
        var goals = Planner.Plan(ABaseline(), ACohort(), new Inputs { Progress = [QuestAchievement(1, null)] });
        Assert.IsType<Standing.Unearned>(goals[0].Standing);
        Assert.False(goals[0].Standing.IsPoisoned);
        Assert.Null(goals[0].Evaluation);
    }

    [Fact(DisplayName = "an_outsiders_achievement_is_poisoned_and_measured_from_primary_data")]
    public void An_outsiders_achievement_is_poisoned_and_measured_from_primary_data()
    {
        var inputs = new Inputs
        {
            Progress = [QuestAchievement(1, "2016-03-04T00:00:00Z")],
            Attributions = { [1] = Aeltor },
            Criteria = QuestCriteria(),
            Primary = Primary(1, 2),
        };
        var goals = Planner.Plan(ABaseline(), ACohort(), inputs);
        Assert.True(goals[0].Standing.IsPoisoned);
        Assert.IsType<Bucket.Observable>(goals[0].Bucket);
        var evaluation = goals[0].Evaluation!.Value;
        Assert.Equal(2, evaluation.Progress);
        Assert.Equal(3, evaluation.Required);
        Assert.False(goals[0].IsDone);
    }

    [Fact(DisplayName = "an_enrolled_characters_achievement_is_settled_and_never_measured")]
    public void An_enrolled_characters_achievement_is_settled_and_never_measured()
    {
        var inputs = new Inputs
        {
            Progress = [QuestAchievement(1, "2016-03-04T00:00:00Z")],
            Attributions = { [1] = Somechar },
            Criteria = QuestCriteria(),
            Primary = Primary(),
        };
        var goals = Planner.Plan(ABaseline(), ACohort(), inputs);
        Assert.True(goals[0].Standing.IsSettled);
        Assert.True(goals[0].IsDone);
        Assert.Null(goals[0].Evaluation);
    }

    [Fact(DisplayName = "without_the_addon_everything_already_earned_falls_to_the_expensive_path")]
    public void Without_the_addon_everything_already_earned_falls_to_the_expensive_path()
    {
        var inputs = new Inputs
        {
            Progress = [QuestAchievement(1, "2016-03-04T00:00:00Z"), QuestAchievement(2, "2017-03-04T00:00:00Z")],
            Criteria = QuestCriteria(),
            Primary = Primary(),
        };
        Assert.Equal(2, Planner.Plan(ABaseline(), ACohort(), inputs).Count(goal => goal.Standing.IsPoisoned));
    }

    [Fact(DisplayName = "a_criterion_the_catalogue_cannot_explain_sends_the_goal_to_attestation")]
    public void A_criterion_the_catalogue_cannot_explain_sends_the_goal_to_attestation()
    {
        var criteria = QuestCriteria();
        criteria.Remove(903);
        var inputs = new Inputs
        {
            Progress = [QuestAchievement(1, "2016-03-04T00:00:00Z")],
            Attributions = { [1] = Aeltor },
            Criteria = criteria,
            Primary = Primary(1, 2),
        };
        var goals = Planner.Plan(ABaseline(), ACohort(), inputs);
        Assert.IsType<Bucket.Attestable>(goals[0].Bucket);
        Assert.Null(goals[0].Fraction());
    }

    [Fact(DisplayName = "an_achievement_with_no_criteria_at_all_is_attestable_not_excluded")]
    public void An_achievement_with_no_criteria_at_all_is_attestable_not_excluded()
    {
        var inputs = new Inputs
        {
            Progress = [new AchievementProgress { Id = 1, CompletedAt = At("2016-03-04T00:00:00Z"), Criteria = null }],
            Attributions = { [1] = Aeltor },
        };
        Assert.IsType<Bucket.Attestable>(Planner.Plan(ABaseline(), ACohort(), inputs)[0].Bucket);
    }

    [Fact(DisplayName = "a_feat_of_strength_leaves_the_run_entirely")]
    public void A_feat_of_strength_leaves_the_run_entirely()
    {
        var inputs = new Inputs
        {
            Progress = [QuestAchievement(1, "2016-03-04T00:00:00Z")],
            Attributions = { [1] = Aeltor },
            Catalogue = { [1] = new Achievement { Id = 1, Name = "Gone", Category = "Feats of Strength", IsUnrepeatable = true } },
        };
        var goals = Planner.Plan(ABaseline(), ACohort(), inputs);
        Assert.Equal(new Bucket.Excluded(Exclusion.Unrepeatable), goals[0].Bucket);
        Assert.False(goals[0].Counts);
    }

    [Fact(DisplayName = "a_hand_exclusion_beats_everything_else")]
    public void A_hand_exclusion_beats_everything_else()
    {
        var inputs = new Inputs
        {
            Progress = [QuestAchievement(1, "2016-03-04T00:00:00Z")],
            Attributions = { [1] = Aeltor },
            Criteria = QuestCriteria(),
            Primary = Primary(1, 2, 3),
            ExcludedByHand = { 1 },
        };
        Assert.Equal(new Bucket.Excluded(Exclusion.ByHand), Planner.Plan(ABaseline(), ACohort(), inputs)[0].Bucket);
    }

    [Fact(DisplayName = "the_furthest_along_character_in_the_cohort_is_the_one_that_counts")]
    public void The_furthest_along_character_in_the_cohort_is_the_one_that_counts()
    {
        var cohort = new Cohort([Somechar, Aeltor]);
        var inputs = new Inputs
        {
            Progress = [QuestAchievement(1, "2016-03-04T00:00:00Z")],
            Attributions = { [1] = new CharacterKey("dalaran", "Moodivh") },
            Criteria = QuestCriteria(),
            Primary =
            {
                [Somechar] = new PrimaryData { Quests = { 1 } },
                [Aeltor] = new PrimaryData { Quests = { 1, 2, 3 } },
            },
        };
        var goals = Planner.Plan(ABaseline(), cohort, inputs);
        Assert.True(goals[0].IsDone, "Aeltor finished it during the run");
        Assert.Equal(Aeltor, goals[0].Nearest);
    }

    [Fact(DisplayName = "a_baseline_completion_beats_the_live_response")]
    public void A_baseline_completion_beats_the_live_response()
    {
        var baseline = ABaseline() with { Completed = [new Completed(1, At("2016-03-04T00:00:00Z"))] };
        var inputs = new Inputs
        {
            Progress = [QuestAchievement(1, "2026-07-01T00:00:00Z")],
            Attributions = { [1] = Aeltor },
            Criteria = QuestCriteria(),
        };
        Assert.True(Planner.Plan(baseline, ACohort(), inputs)[0].Standing.IsPoisoned);
    }

    [Fact(DisplayName = "remeasuring_moves_progress_and_leaves_decisions_alone")]
    public void Remeasuring_moves_progress_and_leaves_decisions_alone()
    {
        var inputs = new Inputs
        {
            Progress = [QuestAchievement(1, "2016-03-04T00:00:00Z")],
            Attributions = { [1] = Aeltor },
            Criteria = QuestCriteria(),
            Primary = Primary(1),
        };
        var run = new Armory.Run.Run { Name = "Fresh start", Baseline = ABaseline(), Cohort = ACohort(), Goals = Planner.Plan(ABaseline(), ACohort(), inputs) };
        Assert.Equal(1, run.Goals[0].Evaluation!.Value.Progress);

        var later = inputs with { Primary = Primary(1, 2, 3) };
        var measured = Planner.Remeasure(run, later);
        Assert.Equal(3, measured.Goals[0].Evaluation!.Value.Progress);
        Assert.True(measured.Goals[0].IsDone);
        Assert.True(measured.Goals[0].Standing.IsPoisoned, "standing never moves");
    }

    [Fact(DisplayName = "remeasuring_never_overwrites_an_attestation")]
    public void Remeasuring_never_overwrites_an_attestation()
    {
        var inputs = new Inputs
        {
            Progress = [new AchievementProgress { Id = 1, CompletedAt = At("2016-03-04T00:00:00Z"), Criteria = null }],
            Attributions = { [1] = Aeltor },
        };
        var planned = Planner.Plan(ABaseline(), ACohort(), inputs);
        planned[0] = planned[0] with { Attestation = new Attestation { Character = Somechar, At = At("2026-07-20T00:00:00Z") } };
        var run = new Armory.Run.Run { Name = "Fresh start", Baseline = ABaseline(), Cohort = ACohort(), Goals = planned };

        var measured = Planner.Remeasure(run, inputs);
        Assert.NotNull(measured.Goals[0].Attestation);
        Assert.True(measured.Goals[0].IsDone);
    }

    [Fact(DisplayName = "a_meta_resolves_once_its_parts_do")]
    public void A_meta_resolves_once_its_parts_do()
    {
        static AchievementProgress Part(long id) => new()
        {
            Id = id,
            CompletedAt = At("2016-03-04T00:00:00Z"),
            Criteria = Criterion.Leaf(id * 10, CriterionKind.Unknown, 1),
        };
        var inputs = new Inputs
        {
            Progress =
            [
                Part(1),
                Part(2),
                new AchievementProgress
                {
                    Id = 3,
                    CompletedAt = At("2016-03-04T00:00:00Z"),
                    Criteria = new Criterion { Id = 30, Required = 0, Children = [Criterion.Leaf(31, CriterionKind.Unknown, 1), Criterion.Leaf(32, CriterionKind.Unknown, 1)] },
                },
            ],
            Attributions = { [1] = Aeltor, [2] = Aeltor, [3] = Aeltor },
            Criteria =
            {
                [10] = CriterionKind.Quest(101),
                [20] = CriterionKind.Quest(102),
                [31] = CriterionKind.Achievement(1),
                [32] = CriterionKind.Achievement(2),
            },
            Primary = Primary(101, 102),
        };
        var goals = Planner.Plan(ABaseline(), ACohort(), inputs);
        Assert.True(goals.Single(goal => goal.AchievementId == 1).IsDone);
        Assert.True(goals.Single(goal => goal.AchievementId == 2).IsDone);
        Assert.True(goals.Single(goal => goal.AchievementId == 3).IsDone, "the meta follows its parts");
    }

    [Fact(DisplayName = "a_meta_stays_open_while_a_part_is_outstanding")]
    public void A_meta_stays_open_while_a_part_is_outstanding()
    {
        var inputs = new Inputs
        {
            Progress =
            [
                new AchievementProgress { Id = 1, CompletedAt = At("2016-03-04T00:00:00Z"), Criteria = Criterion.Leaf(10, CriterionKind.Unknown, 1) },
                new AchievementProgress
                {
                    Id = 3,
                    CompletedAt = At("2016-03-04T00:00:00Z"),
                    Criteria = new Criterion { Id = 30, Required = 0, Children = [Criterion.Leaf(31, CriterionKind.Unknown, 1)] },
                },
            ],
            Attributions = { [1] = Aeltor, [3] = Aeltor },
            Criteria = { [10] = CriterionKind.Quest(101), [31] = CriterionKind.Achievement(1) },
            Primary = Primary(),
        };
        Assert.False(Planner.Plan(ABaseline(), ACohort(), inputs).Single(goal => goal.AchievementId == 3).IsDone);
    }

    [Fact(DisplayName = "a_cycle_in_the_chain_terminates_rather_than_spinning")]
    public void A_cycle_in_the_chain_terminates_rather_than_spinning()
    {
        var inputs = new Inputs
        {
            Progress =
            [
                new AchievementProgress { Id = 1, CompletedAt = At("2016-03-04T00:00:00Z"), Criteria = Criterion.Leaf(10, CriterionKind.Unknown, 1) },
                new AchievementProgress { Id = 2, CompletedAt = At("2016-03-04T00:00:00Z"), Criteria = Criterion.Leaf(20, CriterionKind.Unknown, 1) },
            ],
            Attributions = { [1] = Aeltor, [2] = Aeltor },
            Criteria = { [10] = CriterionKind.Achievement(2), [20] = CriterionKind.Achievement(1) },
            Primary = Primary(),
        };
        var goals = Planner.Plan(ABaseline(), ACohort(), inputs);
        Assert.Equal(2, goals.Count);
        Assert.All(goals, goal => Assert.False(goal.IsDone));
    }

    [Fact(DisplayName = "a_baseline_records_what_was_finished_and_when")]
    public void A_baseline_records_what_was_finished_and_when()
    {
        var baseline = Planner.TakeBaseline([QuestAchievement(1, "2016-03-04T00:00:00Z"), QuestAchievement(2, null)], [100, 200], At("2026-06-01T00:00:00Z"));
        var only = Assert.Single(baseline.Completed);
        Assert.Equal(1, only.Id);
        Assert.Equal([100L, 200L], baseline.Collected);
    }
}
