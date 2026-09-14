using System.Text.Json.Nodes;
using Armory.Roster;
using Armory.Run;
using Armory.Sharing;
using Armory.Store;
using Microsoft.EntityFrameworkCore;
using Xunit;

namespace Armory.Tests.Store;

/// <summary>The run tests from <c>core/src/store.rs</c> and the two replica tests that waited on <c>save_run</c>.</summary>
public sealed class RunStoreTests
{
    private static Armory.Run.Run ARun(string name, long second) => new()
    {
        Name = name,
        Baseline = new Baseline { TakenAt = DateTimeOffset.FromUnixTimeSeconds(second) },
        Cohort = new Cohort(),
        Goals = [new Goal { AchievementId = 1234, Standing = new Standing.Unearned(), Bucket = new Bucket.Observable() }],
    };

    private static Armory.Store.Store StoreNamed(string machine)
    {
        var store = Armory.Store.Store.InMemory();
        store.SetMachine(machine);
        return store;
    }

    private static Applied Carry(Armory.Store.Store from, Armory.Store.Store to)
    {
        var (parcel, through) = from.Outbox(10_000).Value;
        var applied = to.Apply(parcel, Recording.Off).Value;
        from.Drain(through);
        return applied;
    }

    [Fact(DisplayName = "a run round trips with its goals and is the current one")]
    public void A_run_round_trips_with_its_goals_and_is_the_current_one()
    {
        using var store = Armory.Store.Store.InMemory();
        var run = ARun("The Second Time", 200) with
        {
            Goals =
            [
                new Goal { AchievementId = 1, Standing = new Standing.Poisoned(new CharacterKey("mannoroth", "Aeltor")), Bucket = new Bucket.Attestable(), Attestation = new Attestation { Character = new CharacterKey("emerald-dream", "Somechar"), At = DateTimeOffset.FromUnixTimeSeconds(300) } },
                new Goal { AchievementId = 2, Standing = new Standing.Unearned(), Bucket = new Bucket.Excluded(Exclusion.ByHand) },
            ],
        };
        var id = store.SaveRun(null, run).Value;
        var (heldId, held) = store.CurrentRun().Value!.Value;
        Assert.Equal(id, heldId);
        Assert.Equal(run, held);
        // Saved again under its id, it is the same run.
        Assert.Equal(id, store.SaveRun(id, run).Value);
        Assert.Equal(id, store.SaveRun(null, run).Value);
    }

    [Fact(DisplayName = "forgetting_a_run_takes_its_goals_with_it")]
    public void Forgetting_a_run_takes_its_goals_with_it()
    {
        using var store = Armory.Store.Store.InMemory();
        var id = store.SaveRun(null, ARun("Mine", 100)).Value;
        Assert.NotNull(store.CurrentRun().Value);
        store.ForgetRun(id);
        Assert.Null(store.CurrentRun().Value);
        Assert.Equal(0, store.Context.Goals.Count());
    }

    [Fact(DisplayName = "a_run_and_its_goals_travel_and_stay_one_run")]
    public void A_run_and_its_goals_travel_and_stay_one_run()
    {
        // `run.id` is a local autoincrement, so the naive version gives the
        // two machines different ids for the same run and hangs one machine's
        // goals off the other's run.
        using var one = StoreNamed("one");
        using var two = StoreNamed("two");

        two.SaveRun(null, ARun("Something Else", 100));
        two.Drain(two.HighWater());

        one.SaveRun(null, ARun("The Second Time", 200));
        Carry(one, two);

        Assert.Equal(2, two.Context.Runs.Count());
        var (id, current) = two.CurrentRun().Value!.Value;
        Assert.Equal("The Second Time", current.Name);
        Assert.Equal(1234, Assert.Single(current.Goals).AchievementId);
        Assert.Equal(1, two.Context.Goals.Count(goal => goal.RunId == id));

        // And sending it again changes nothing on either side.
        one.SaveRun(null, ARun("The Second Time", 200));
        Assert.Empty(one.Outbox(100).Value.Parcel.Rows);
    }

    [Fact(DisplayName = "a_goal_whose_run_never_arrived_is_dropped_rather_than_hung_off_another")]
    public void A_goal_whose_run_never_arrived_is_dropped_rather_than_hung_off_another()
    {
        using var two = StoreNamed("two");
        two.SaveRun(null, ARun("Mine", 100));

        var parcel = new Parcel
        {
            Rows =
            [
                new Row
                {
                    Scope = "goal",
                    Key = [JsonValue.Create("a-run-this-machine-has-never-seen"), JsonValue.Create(999)],
                    Fields = [JsonValue.Create("\"Unearned\""), JsonValue.Create("\"Observable\""), null],
                },
            ],
        };
        var applied = two.Apply(parcel, Recording.Off).Value;
        Assert.Equal(1, applied.Unreadable);
        Assert.Equal(0, applied.Written);
        Assert.Equal(1, two.Context.Goals.Count());
    }

    [Fact(DisplayName = "saving the same run again enqueues nothing")]
    public void Saving_the_same_run_again_enqueues_nothing()
    {
        // The run's half of `a_second_identical_write_enqueues_nothing`.
        using var store = StoreNamed("one");
        var run = ARun("The Second Time", 200);
        var id = store.SaveRun(null, run).Value;
        Assert.NotEmpty(store.Queued().Value);
        store.Drain(store.HighWater());
        store.SaveRun(id, run);
        Assert.Empty(store.Queued().Value);
    }
}
