using System.Globalization;
using System.Text.Json;
using Armory.Run;
using Microsoft.EntityFrameworkCore;

namespace Armory.Store;

/// <summary>Runs and their goals.</summary>
public sealed partial class Store
{
    /// <summary>
    /// What a run is called between machines. Derived from the moment the
    /// baseline was taken, so the same run saved again is the same run and a
    /// replan does not rename it. Every other table on the wire is keyed by
    /// something the game already agreed on; this is the one that had to be
    /// given a key, because <c>run.id</c> is a local autoincrement.
    /// </summary>
    public static string RunKey(Run.Run run) =>
        string.Create(CultureInfo.InvariantCulture, $"run-{run.Baseline.TakenAt.ToUnixTimeSeconds()}");

    /// <summary>
    /// Save a run, replacing its goals, and mark it current. Returns the
    /// run's id. Saved on the key rather than as a plain insert: starting a
    /// run twice from the same baseline is the same run. The goals are
    /// reconciled within this run rather than deleted and rewritten, because
    /// a replan runs on every addon read and produces a list that is almost
    /// entirely the same one.
    /// </summary>
    public Result<long, StoreError> SaveRun(long? id, Run.Run run) => Work(() =>
    {
        var baseline = JsonSerializer.Serialize(run.Baseline);
        var cohort = JsonSerializer.Serialize(run.Cohort);
        var key = RunKey(run);

        RunRow row;
        if (id is { } given && Context.Runs.Find(given) is { } held)
        {
            held.Name = run.Name;
            held.Baseline = baseline;
            held.Cohort = cohort;
            held.Key = key;
            row = held;
        }
        else if (Context.Runs.FirstOrDefault(candidate => candidate.Key == key) is { } same)
        {
            same.Name = run.Name;
            same.Baseline = baseline;
            same.Cohort = cohort;
            same.IsCurrent = 1;
            row = same;
        }
        else
        {
            row = new RunRow { Name = run.Name, Baseline = baseline, Cohort = cohort, IsCurrent = 1, Key = key };
            Context.Runs.Add(row);
        }
        Context.SaveChanges();
        var runId = row.Id;

        // One run is the one being looked at, and two current runs would make
        // "the run" ambiguous everywhere it is read.
        foreach (var other in Context.Runs.Where(candidate => candidate.Id != runId && candidate.IsCurrent != 0))
        {
            other.IsCurrent = 0;
        }
        row.IsCurrent = 1;

        var goals = run.Goals.Select(goal => new GoalRow
        {
            RunId = runId,
            AchievementId = goal.AchievementId,
            Standing = JsonSerializer.Serialize(goal.Standing),
            Bucket = JsonSerializer.Serialize(goal.Bucket),
            Attestation = goal.Attestation is null ? null : JsonSerializer.Serialize(goal.Attestation),
        }).ToList();
        Reconcile(
            Context.Goals.Where(goal => goal.RunId == runId),
            goals,
            goal => goal.AchievementId,
            (existing, fresh) =>
            {
                existing.Standing = fresh.Standing;
                existing.Bucket = fresh.Bucket;
                existing.Attestation = fresh.Attestation;
            });
        return runId;
    });

    /// <summary>
    /// Forget a run and everything planned for it: the deliberate end of a
    /// run. The goals go first, or rows keyed to a run that no longer exists
    /// would be stranded on this machine with nothing to say why.
    /// </summary>
    public Result<Unit, StoreError> ForgetRun(long id) => Work(() =>
    {
        Context.Goals.Where(goal => goal.RunId == id).ExecuteDelete();
        Context.Runs.Where(run => run.Id == id).ExecuteDelete();
    });

    /// <summary>
    /// The run currently being looked at, if there is one. A run whose
    /// baseline will not parse is reported as no run: an empty one would look
    /// like the run had lost its progress.
    /// </summary>
    public Result<(long Id, Run.Run Run)?, StoreError> CurrentRun() => Work<(long Id, Run.Run Run)?>(() =>
    {
        var row = Context.Runs.AsNoTracking().FirstOrDefault(run => run.IsCurrent == 1);
        if (row is null)
        {
            return null;
        }
        Baseline? baseline;
        Roster.Cohort? cohort;
        try
        {
            baseline = JsonSerializer.Deserialize<Baseline>(row.Baseline);
            cohort = JsonSerializer.Deserialize<Roster.Cohort>(row.Cohort);
        }
        catch (JsonException)
        {
            return null;
        }
        if (baseline is null || cohort is null)
        {
            return null;
        }

        var goals = new List<Goal>();
        foreach (var goal in Context.Goals.AsNoTracking().Where(goal => goal.RunId == row.Id))
        {
            try
            {
                var standing = JsonSerializer.Deserialize<Standing>(goal.Standing);
                var bucket = JsonSerializer.Deserialize<Bucket>(goal.Bucket);
                if (standing is null || bucket is null)
                {
                    continue;
                }
                Attestation? attestation = null;
                if (goal.Attestation is { } said)
                {
                    try
                    {
                        attestation = JsonSerializer.Deserialize<Attestation>(said);
                    }
                    catch (JsonException)
                    {
                        attestation = null;
                    }
                }
                goals.Add(new Goal { AchievementId = goal.AchievementId, Standing = standing, Bucket = bucket, Attestation = attestation });
            }
            catch (JsonException)
            {
                // A goal a newer build wrote.
            }
        }
        return (row.Id, new Run.Run { Name = row.Name, Baseline = baseline, Cohort = cohort, Goals = goals });
    });
}
