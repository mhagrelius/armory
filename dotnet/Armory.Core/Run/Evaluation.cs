namespace Armory.Run;

/// <summary>What evaluating a criteria tree against one character produced.</summary>
public readonly record struct Evaluation(long Progress, long Required, bool Observable, bool Inherited)
{
    /// <summary>
    /// <see cref="Observable"/> false means the progress is a floor, not a
    /// measurement, and must not be drawn as a bar. <see cref="Inherited"/>
    /// means part of the answer came from a value the cohort did not earn: a
    /// reputation satisfied by an alt from 2023 is not progress, and reporting
    /// it as progress is the failure that would make the run meaningless.
    /// </summary>
    public bool IsComplete => Observable && !Inherited && Progress >= Required;

    public double Fraction => Required == 0 ? 0.0 : Math.Clamp(Progress / (double)Required, 0.0, 1.0);
}

public static class Criteria
{
    /// <summary>
    /// Measure a criteria tree against one character's own data. A parent
    /// node's progress is how many of its children are satisfied, which is
    /// how Blizzard's own counters behave: "complete 10 of these quests" is a
    /// parent requiring 10 with a child per quest.
    /// </summary>
    public static Evaluation Evaluate(Criterion criterion, PrimaryData data)
    {
        if (criterion.Children.Count == 0)
        {
            return EvaluateLeaf(criterion, data);
        }
        long satisfied = 0;
        var observable = true;
        var inherited = false;
        foreach (var child in criterion.Children)
        {
            var evaluation = Evaluate(child, data);
            observable &= evaluation.Observable;
            inherited |= evaluation.Inherited;
            if (evaluation.IsComplete)
            {
                satisfied++;
            }
        }
        // A parent with no explicit requirement needs all of its children: a
        // meta-achievement lists the things it is made of and wants every one.
        var required = criterion.Required == 0 ? criterion.Children.Count : criterion.Required;
        return new Evaluation(satisfied, required, observable, inherited);
    }

    private static Evaluation EvaluateLeaf(Criterion criterion, PrimaryData data)
    {
        var required = criterion.Threshold;
        var kind = criterion.Kind;
        switch (kind.Type)
        {
            case CriterionType.Quest:
                return new Evaluation(data.Quests.Contains(kind.Asset) ? 1 : 0, required, true, false);
            case CriterionType.Encounter:
                return new Evaluation(data.Encounters.Contains(kind.Asset) ? 1 : 0, required, true, false);
            case CriterionType.Statistic:
                // A missing statistic is a character who has never done the
                // thing at all. That is still an observation.
                return new Evaluation(
                    data.Statistics.TryGetValue(kind.Asset, out var value) ? (long)Math.Max(value, 0.0) : 0,
                    required, true, false);
            case CriterionType.Reputation:
                {
                    var standing = data.Reputations.GetValueOrDefault(kind.Asset);
                    // An inherited standing is not a measurement and cannot
                    // become one: it was at the ceiling before the run began.
                    // What can move is what this character has been observed
                    // earning. Where nothing has been observed, falling back
                    // to the account's standing would be exactly the inflation
                    // this field exists to prevent.
                    if (data.InheritedReputations.Contains(kind.Asset))
                    {
                        var observed = data.EarnedReputations.TryGetValue(kind.Asset, out var earned) ? earned.Points : 0;
                        // No longer inherited once this character's own work
                        // covers the requirement.
                        return new Evaluation(observed, required, true, observed < required);
                    }
                    return new Evaluation(standing, required, true, false);
                }
            case CriterionType.Achievement:
                // A meta's criteria are other achievements, and whether the
                // run has those is a question about the run.
                return new Evaluation(data.AchievementsDone.Contains(kind.Asset) ? 1 : 0, required, true, false);
            default:
                return new Evaluation(0, required, false, false);
        }
    }

    /// <summary>
    /// Attach catalogue kinds to a criteria tree read from the profile
    /// response. The profile gives structure and progress; the catalogue
    /// gives meaning. A criterion the catalogue has never heard of keeps
    /// <c>Unknown</c> rather than borrowing its parent's kind.
    /// </summary>
    public static Criterion WithCatalogue(Criterion criterion, IReadOnlyDictionary<long, CriterionKind> catalogue) => new()
    {
        Id = criterion.Id,
        Kind = catalogue.GetValueOrDefault(criterion.Id, CriterionKind.Unknown),
        Required = criterion.Required,
        Children = criterion.Children.Select(child => WithCatalogue(child, catalogue)).ToList(),
    };
}
