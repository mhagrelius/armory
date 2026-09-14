using Armory.Blizzard;
using Armory.Provenance;
using Armory.Roster;

namespace Armory.Run;

/// <summary>
/// Everything the planner needs, gathered by the caller. A record rather than
/// eight arguments because every one of these is fetched separately and half
/// of them can be missing. Missing data must degrade the classification,
/// never stop it.
/// </summary>
public sealed record Inputs
{
    /// <summary>What the account has done.</summary>
    public List<AchievementProgress> Progress { get; init; } = [];

    /// <summary>Names, points, and whether a thing can be earned twice.</summary>
    public Dictionary<long, Achievement> Catalogue { get; init; } = [];

    /// <summary>Which character earned each account-wide achievement. Empty without the addon, which is what makes everything already earned poisoned.</summary>
    public Dictionary<long, CharacterKey> Attributions { get; init; } = [];

    /// <summary>Criterion id to what it measures.</summary>
    public Dictionary<long, CriterionKind> Criteria { get; init; } = [];

    /// <summary>Per-character primary data, for the enrolled cohort.</summary>
    public Dictionary<CharacterKey, PrimaryData> Primary { get; init; } = [];

    /// <summary>
    /// What each character has personally been observed earning. Merged into
    /// the primary data at the last moment rather than baked in, because the
    /// two arrive from different places at different times.
    /// </summary>
    public Earnings Provenance { get; init; } = [];

    /// <summary>Collectible ids the account already owns, from the baseline.</summary>
    public HashSet<long> Owned { get; init; } = [];

    /// <summary>Goals the user has excluded by hand. A decision a person made survives a resync.</summary>
    public HashSet<long> ExcludedByHand { get; init; } = [];
}

/// <summary>
/// Building a run's goals, and deciding what each one costs to track. Pure
/// functions over data that has already been fetched. Standing is decided
/// first, because it is cheap and settles most goals outright; only the
/// poisoned ones reach <see cref="Classify"/>.
/// </summary>
public static class Planner
{
    /// <summary>
    /// How many times to re-resolve dependency chains. Warcraft's chains are
    /// two or three deep, so four passes reaches a fixpoint with room to
    /// spare, and the bound is what stops a cycle in the data spinning forever.
    /// </summary>
    private const int ChainPasses = 4;

    /// <summary>
    /// Build the goal list for a run. Two phases: classify every goal and
    /// measure the ones that stand alone, then resolve dependency chains by
    /// feeding the parts' outcomes back in until nothing more moves.
    /// </summary>
    public static List<Goal> Plan(Baseline baseline, Cohort cohort, Inputs inputs)
    {
        var goals = PlanOnce(baseline, cohort, inputs, []);
        for (var pass = 0; pass < ChainPasses; pass++)
        {
            var done = goals.Where(goal => goal.IsDone).Select(goal => goal.AchievementId).ToHashSet();
            var next = PlanOnce(baseline, cohort, inputs, done);
            var settled = next.Where(goal => goal.IsDone).Select(goal => goal.AchievementId).ToHashSet();
            goals = next;
            if (settled.SetEquals(done))
            {
                break;
            }
        }
        return goals;
    }

    /// <summary>One pass, against a fixed idea of what the run has already done.</summary>
    private static List<Goal> PlanOnce(Baseline baseline, Cohort cohort, Inputs inputs, HashSet<long> done)
    {
        var completed = baseline.Completed.ToDictionary(entry => entry.Id, entry => entry.At);
        return inputs.Progress.Select(progress =>
        {
            // The baseline's record of when something was finished beats the
            // live response: standing has to be decided against a fixed point
            // or a goal could un-poison itself halfway through.
            var completedAt = completed.TryGetValue(progress.Id, out var at) ? at : progress.CompletedAt;
            var standing = Standing.Classify(completedAt, inputs.Attributions.GetValueOrDefault(progress.Id), cohort, baseline.TakenAt);
            // Never read for an unpoisoned goal. Observable is the neutral value.
            Bucket bucket = standing.IsPoisoned ? Classify(progress, inputs) : new Bucket.Observable();
            var evaluation = bucket is Bucket.Observable && standing.IsPoisoned ? BestEvaluation(progress, cohort, inputs, done) : null;
            return new Goal
            {
                AchievementId = progress.Id,
                Standing = standing,
                Bucket = bucket,
                Attestation = null,
                Nearest = evaluation?.Who,
                Evaluation = evaluation?.Evaluation,
            };
        }).ToList();
    }

    /// <summary>
    /// Decide how a poisoned goal can be tracked. The order of the checks is
    /// the order of cost: an exclusion is a lookup, observability needs the
    /// criteria tree, and attestation is what is left.
    /// </summary>
    public static Bucket Classify(AchievementProgress progress, Inputs inputs)
    {
        if (inputs.ExcludedByHand.Contains(progress.Id))
        {
            return new Bucket.Excluded(Exclusion.ByHand);
        }
        // A Feat of Strength can never be earned again by anybody.
        if (inputs.Catalogue.TryGetValue(progress.Id, out var achievement) && achievement.IsUnrepeatable)
        {
            return new Bucket.Excluded(Exclusion.Unrepeatable);
        }
        if (inputs.Owned.Contains(progress.Id))
        {
            return new Bucket.Excluded(Exclusion.AlreadyOwned);
        }
        // With no criteria tree there is nothing to measure. That is
        // attestable rather than excluded: a person may well remember doing
        // it, and excluding it would decide on their behalf.
        if (progress.Criteria is not { } criteria)
        {
            return new Bucket.Attestable();
        }
        return IsObservable(criteria, inputs.Criteria) ? new Bucket.Observable() : new Bucket.Attestable();
    }

    /// <summary>Whether every leaf of a criteria tree resolves to something measurable. One unknown leaf is enough to make the whole tree unobservable.</summary>
    private static bool IsObservable(Criterion criterion, Dictionary<long, CriterionKind> catalogue) =>
        criterion.Children.Count == 0
            ? catalogue.GetValueOrDefault(criterion.Id, CriterionKind.Unknown).IsObservable
            : criterion.Children.All(child => IsObservable(child, catalogue));

    /// <summary>
    /// Measure a goal against whichever enrolled character is furthest along,
    /// and say which one that was. Taking the maximum is the only reading
    /// that does not punish having a roster, and the figure is meaningless
    /// without the name.
    /// </summary>
    private static (Evaluation Evaluation, CharacterKey Who)? BestEvaluation(AchievementProgress progress, Cohort cohort, Inputs inputs, HashSet<long> done)
    {
        if (progress.Criteria is not { } criteria)
        {
            return null;
        }
        var tree = Criteria.WithCatalogue(criteria, inputs.Criteria);
        (Evaluation Evaluation, CharacterKey Who)? best = null;
        foreach (var key in cohort.Keys)
        {
            if (!inputs.Primary.TryGetValue(key, out var held))
            {
                continue;
            }
            // Two things folded into each character's own data at the last
            // moment: what the run has done, because a meta does not care
            // which character earned its parts; and what this character
            // personally earned, the only thing that can measure a
            // reputation the account maxed out before the run began.
            var data = held with
            {
                AchievementsDone = [.. done],
                EarnedReputations = inputs.Provenance.TryGetValue(key, out var earned) ? new Dictionary<long, EarnedReputation>(earned.Reputation) : [],
            };
            var evaluation = Criteria.Evaluate(tree, data);
            if (best is null || Better(evaluation, best.Value.Evaluation))
            {
                best = (evaluation, key);
            }
        }
        return best;
    }

    /// <summary>Complete beats incomplete; an inherited evaluation loses to anything genuine; otherwise the further along wins.</summary>
    private static bool Better(Evaluation a, Evaluation b)
    {
        if (a.IsComplete != b.IsComplete)
        {
            return a.IsComplete;
        }
        if (a.Inherited != b.Inherited)
        {
            return !a.Inherited;
        }
        return a.Fraction > b.Fraction;
    }

    /// <summary>
    /// Re-measure an existing run's observable goals against fresh primary
    /// data. Standing and bucket are left alone: poisoning is decided once at
    /// baseline, a bucket the user has overruled must not be silently
    /// re-decided, and attestations are a person's word.
    /// </summary>
    public static Run Remeasure(Run run, Inputs inputs)
    {
        // Chains again: a meta's parts may have been finished since the last
        // pass, and re-measuring the meta without them would leave it stuck.
        var done = run.Goals.Where(goal => goal.IsDone).Select(goal => goal.AchievementId).ToHashSet();
        var byId = inputs.Progress.ToDictionary(progress => progress.Id);
        var goals = new List<Goal>(run.Goals.Count);
        foreach (var goal in run.Goals)
        {
            if (goal.Bucket is not Bucket.Observable || !goal.Standing.IsPoisoned || !byId.TryGetValue(goal.AchievementId, out var progress))
            {
                goals.Add(goal);
                continue;
            }
            var best = BestEvaluation(progress, run.Cohort, inputs, done);
            var measured = goal with { Nearest = best?.Who, Evaluation = best?.Evaluation };
            if (measured.IsDone)
            {
                done.Add(measured.AchievementId);
            }
            goals.Add(measured);
        }
        return run with { Goals = goals };
    }

    /// <summary>Take a baseline from what the account currently has.</summary>
    public static Baseline TakeBaseline(IEnumerable<AchievementProgress> progress, IEnumerable<long> collected, DateTimeOffset at) => new()
    {
        TakenAt = at,
        Collected = collected.Distinct().OrderBy(id => id).ToList(),
        Completed = progress
            .Where(entry => entry.CompletedAt is not null)
            .Select(entry => new Completed(entry.Id, entry.CompletedAt!.Value))
            .OrderBy(entry => entry.Id).ThenBy(entry => entry.At)
            .ToList(),
    };
}
