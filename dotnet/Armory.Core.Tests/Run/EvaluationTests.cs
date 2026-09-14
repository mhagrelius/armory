using Armory.Provenance;
using Armory.Run;
using Xunit;

namespace Armory.Tests.Run;

/// <summary>Ported from <c>core/src/achievement.rs</c>.</summary>
public sealed class EvaluationTests
{
    private static PrimaryData Quests(params long[] ids) => new() { Quests = [.. ids] };

    private static Criterion Parent(long required, params Criterion[] children) =>
        new() { Id = 100, Kind = CriterionKind.Unknown, Required = required, Children = [.. children] };

    [Fact(DisplayName = "a_quest_criterion_reads_the_characters_own_completed_list")]
    public void A_quest_criterion_reads_the_characters_own_completed_list()
    {
        var criterion = Criterion.Leaf(1, CriterionKind.Quest(42), 1);
        Assert.True(Criteria.Evaluate(criterion, Quests(42)).IsComplete);
        Assert.False(Criteria.Evaluate(criterion, Quests(7)).IsComplete);
    }

    [Fact(DisplayName = "a_parent_counts_how_many_children_are_satisfied")]
    public void A_parent_counts_how_many_children_are_satisfied()
    {
        var criterion = Parent(2, Criterion.Leaf(1, CriterionKind.Quest(1), 1), Criterion.Leaf(2, CriterionKind.Quest(2), 1), Criterion.Leaf(3, CriterionKind.Quest(3), 1));
        var evaluation = Criteria.Evaluate(criterion, Quests(1, 3));
        Assert.Equal(2, evaluation.Progress);
        Assert.Equal(2, evaluation.Required);
        Assert.True(evaluation.IsComplete);
    }

    [Fact(DisplayName = "a_parent_with_no_requirement_wants_all_of_its_children")]
    public void A_parent_with_no_requirement_wants_all_of_its_children()
    {
        var criterion = Parent(0, Criterion.Leaf(1, CriterionKind.Quest(1), 1), Criterion.Leaf(2, CriterionKind.Quest(2), 1));
        var evaluation = Criteria.Evaluate(criterion, Quests(1));
        Assert.Equal(2, evaluation.Required);
        Assert.False(evaluation.IsComplete);
        Assert.True(Criteria.Evaluate(criterion, Quests(1, 2)).IsComplete);
    }

    [Fact(DisplayName = "one_unknown_child_makes_the_whole_tree_unobservable")]
    public void One_unknown_child_makes_the_whole_tree_unobservable()
    {
        var criterion = Parent(0, Criterion.Leaf(1, CriterionKind.Quest(1), 1), Criterion.Leaf(2, CriterionKind.Unknown, 1));
        var evaluation = Criteria.Evaluate(criterion, Quests(1));
        Assert.False(evaluation.Observable);
        Assert.False(evaluation.IsComplete);
    }

    [Fact(DisplayName = "an_inherited_reputation_is_never_progress")]
    public void An_inherited_reputation_is_never_progress()
    {
        var criterion = Criterion.Leaf(1, CriterionKind.Reputation(2170), 42000);
        var data = new PrimaryData { Reputations = { [2170] = 42000 }, InheritedReputations = { 2170 } };
        var evaluation = Criteria.Evaluate(criterion, data);
        Assert.True(evaluation.Inherited);
        Assert.False(evaluation.IsComplete);
    }

    [Fact(DisplayName = "a_character_can_earn_the_equivalent_of_a_standing_the_account_already_had")]
    public void A_character_can_earn_the_equivalent_of_a_standing_the_account_already_had()
    {
        var criterion = Criterion.Leaf(1, CriterionKind.Reputation(2170), 42000);
        var partway = new PrimaryData
        {
            Reputations = { [2170] = 42000 },
            InheritedReputations = { 2170 },
            EarnedReputations = { [2170] = new EarnedReputation { Points = 21_000, AccountWide = true } },
        };
        var evaluation = Criteria.Evaluate(criterion, partway);
        Assert.True(evaluation.Inherited);
        Assert.False(evaluation.IsComplete);
        Assert.Equal(21_000, evaluation.Progress);

        var done = partway with { EarnedReputations = new Dictionary<long, EarnedReputation> { [2170] = new EarnedReputation { Points = 42_000, AccountWide = true } } };
        evaluation = Criteria.Evaluate(criterion, done);
        Assert.False(evaluation.Inherited);
        Assert.True(evaluation.IsComplete);
    }

    [Fact(DisplayName = "an_inherited_standing_with_nothing_observed_stays_worth_nothing")]
    public void An_inherited_standing_with_nothing_observed_stays_worth_nothing()
    {
        var criterion = Criterion.Leaf(1, CriterionKind.Reputation(2170), 42000);
        var data = new PrimaryData { Reputations = { [2170] = 42000 }, InheritedReputations = { 2170 } };
        var evaluation = Criteria.Evaluate(criterion, data);
        Assert.Equal(0, evaluation.Progress);
        Assert.True(evaluation.Inherited);
        Assert.False(evaluation.IsComplete);
    }

    [Fact(DisplayName = "an_uninherited_reputation_counts_normally")]
    public void An_uninherited_reputation_counts_normally()
    {
        var criterion = Criterion.Leaf(1, CriterionKind.Reputation(2170), 42000);
        Assert.True(Criteria.Evaluate(criterion, new PrimaryData { Reputations = { [2170] = 42000 } }).IsComplete);
    }

    [Fact(DisplayName = "inheritance_propagates_up_the_tree")]
    public void Inheritance_propagates_up_the_tree()
    {
        var criterion = Parent(0, new Criterion { Id = 10, Required = 0, Children = [Criterion.Leaf(1, CriterionKind.Reputation(5), 1)] });
        var data = new PrimaryData { Reputations = { [5] = 100 }, InheritedReputations = { 5 } };
        Assert.True(Criteria.Evaluate(criterion, data).Inherited);
    }

    [Fact(DisplayName = "a_missing_statistic_is_zero_and_still_an_observation")]
    public void A_missing_statistic_is_zero_and_still_an_observation()
    {
        var evaluation = Criteria.Evaluate(Criterion.Leaf(1, CriterionKind.Statistic(1337), 10), new PrimaryData());
        Assert.True(evaluation.Observable);
        Assert.Equal(0, evaluation.Progress);
    }

    [Fact(DisplayName = "the_catalogue_supplies_meaning_and_never_invents_it")]
    public void The_catalogue_supplies_meaning_and_never_invents_it()
    {
        var tree = Parent(0, Criterion.Leaf(1, CriterionKind.Unknown, 1), Criterion.Leaf(2, CriterionKind.Unknown, 1));
        var joined = Criteria.WithCatalogue(tree, new Dictionary<long, CriterionKind> { [1] = CriterionKind.Quest(500) });
        Assert.Equal(CriterionKind.Quest(500), joined.Children[0].Kind);
        Assert.Equal(CriterionKind.Unknown, joined.Children[1].Kind);
    }

    [Fact(DisplayName = "only_the_criteria_types_we_have_confirmed_are_claimed")]
    public void Only_the_criteria_types_we_have_confirmed_are_claimed()
    {
        Assert.Equal(CriterionKind.Quest(500), CriterionKind.FromCatalogue(27, 500));
        Assert.Equal(CriterionKind.Reputation(2170), CriterionKind.FromCatalogue(46, 2170));
        Assert.Equal(CriterionKind.Unknown, CriterionKind.FromCatalogue(119, 9));
    }

    [Fact(DisplayName = "a_fraction_never_exceeds_one")]
    public void A_fraction_never_exceeds_one()
    {
        Assert.Equal(1.0, new Evaluation(30, 10, true, false).Fraction);
    }
}
