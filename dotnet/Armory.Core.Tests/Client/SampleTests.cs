using Armory.Addon;
using Armory.Blizzard;
using Armory.Client.Shell;
using Armory.Collections;
using Armory.Run;
using Xunit;

namespace Armory.Tests.Client;

/// <summary>
/// The preview's made-up account, seeded the way the preview seeds it. The
/// point of the sample is the states that are hard to reach on demand, so
/// what is asserted is that each of them is actually there once the store
/// and the planner have had their say — not the numbers, which are made up.
/// </summary>
public sealed class SampleTests : IDisposable
{
    private readonly string directory = Directory.CreateTempSubdirectory("armory-sample-").FullName;
    private readonly List<Account> accounts = [];

    public void Dispose()
    {
        foreach (var account in accounts)
        {
            account.Dispose();
        }
        Directory.Delete(directory, recursive: true);
    }

    private Account Seeded()
    {
        var store = Armory.Store.Store.InMemory();
        var seeded = Sample.Seed(store);
        Assert.True(seeded.IsOk, seeded.IsOk ? "" : seeded.Error.Message);
        var wow = Path.Combine(directory, "wow");
        Directory.CreateDirectory(wow);
        Sample.InstallRarity(wow);
        var settings = Path.Combine(directory, "settings.json");
        new Armory.Settings.Settings { WowPath = wow, AddonOnly = true, JournalAutomatic = false }.Save(settings);
        var account = new Account(new StoreWorker(store), new MemorySecrets(), settings, work => work().GetAwaiter().GetResult())
        {
            ArtDirectory = Path.Combine(directory, "cache"),
        };
        accounts.Add(account);
        return account;
    }

    [Fact(DisplayName = "the_sample_seeds_every_table_a_page_reads")]
    public void The_sample_seeds_every_table_a_page_reads()
    {
        using var store = Armory.Store.Store.InMemory();
        Assert.True(Sample.Seed(store).IsOk);

        Assert.Equal(7, store.RosterHeld().Value.Count);
        Assert.Equal(3, store.CohortHeld().Value.Count);
        Assert.Equal(3, store.Details().Value.Count);
        Assert.Equal(3, store.SessionsHeld(60).Value.Count);
        Assert.Single(store.EntriesHeld().Value);
        Assert.NotNull(store.CurrentRun().Value);
        foreach (var kind in Links.AllKinds)
        {
            var (catalogue, owned) = store.CollectiblesHeld(kind).Value;
            Assert.NotEmpty(catalogue);
            Assert.NotEmpty(owned);
        }
        Assert.Contains(Sample.Somechar, store.TalliesHeld().Value.Keys);
        Assert.Contains(Sample.Ulahae, store.ProvenanceHeld().Value.Keys);
        Assert.Equal(2, store.WatchedRealmList().Value.Count);
        Assert.Equal(2, store.WatchedItems().Value.Count);
        Assert.Equal(8, store.Snapshot(0).Value.Count);
        Assert.Equal(4, store.PriceSeries(Sample.EmeraldDream, Listing.CagedPet).Value.Count);
        Assert.NotEmpty(store.RecipesHeld().Value);
    }

    [Fact(DisplayName = "the_dump_plans_a_run_with_a_goal_in_every_bucket_and_standing")]
    public async Task The_dump_plans_a_run_with_a_goal_in_every_bucket_and_standing()
    {
        var account = Seeded();
        await account.Restore();
        await account.Collected(Result<Dump, ReadError>.Ok(Sample.Dump()));

        var goals = Assert.NotNull(account.Run).Run.Goals;
        Assert.Contains(goals, goal => goal.Standing is Standing.EarnedDuringRun);
        Assert.Contains(goals, goal => goal.Standing is Standing.EarnedByCohort);
        Assert.Contains(goals, goal => goal.Standing is Standing.Unearned);
        Assert.Contains(goals, goal => goal.Standing is Standing.Poisoned && goal.Bucket is Bucket.Observable && goal.Evaluation is { Observable: true, Progress: > 0 } evaluation && evaluation.Progress < evaluation.Required);
        Assert.Contains(goals, goal => goal.Standing is Standing.Poisoned && goal.Bucket is Bucket.Observable && goal.Evaluation is { IsComplete: true });
        Assert.Contains(goals, goal => goal.Bucket is Bucket.Attestable && goal.Attestation is not null);
        Assert.Contains(goals, goal => goal.Bucket is Bucket.Attestable && goal.Attestation is null);
        Assert.Contains(goals, goal => goal.Bucket is Bucket.Excluded { Why: Exclusion.Unrepeatable });
        Assert.Contains(goals, goal => goal.Bucket is Bucket.Excluded { Why: Exclusion.ByHand });
        // Nothing the planner produced is nameless.
        Assert.All(goals, goal => Assert.True(account.Inputs.Catalogue.ContainsKey(goal.AchievementId)));
    }

    [Fact(DisplayName = "the_sample_reaches_the_account_the_way_a_real_one_does")]
    public async Task The_sample_reaches_the_account_the_way_a_real_one_does()
    {
        var account = Seeded();
        await account.Restore();

        // Reputations out of the response cache, with the fresh alt's renown inherited.
        Assert.Contains(Sample.Ulahae, account.Reputations.Keys);
        Assert.Contains(account.Reputations[Sample.Ulahae], standing => standing.Inherited);
        Assert.DoesNotContain(account.Reputations[Sample.Somechar], standing => standing.Inherited);
        // The odds out of the Rarity fixture, joined on the summoning spell.
        Assert.Equal(20, account.Chances.OneIn(new Collectible { Kind = Kind.Mount, Id = 9, LinkId = 900 }));

        var market = await account.MarketNow();
        Assert.NotEmpty(market.Quotes);
        Assert.NotEmpty(market.Market);
        Assert.Contains(market.Market, listed => listed.Name is null);
        Assert.NotEmpty(market.Crafting.Worth);
        Assert.Equal(1, market.Crafting.Unmeasured.MissingOutput);
        Assert.Contains(market.Crafting.Worth, making => making.Held.Count > 0);
        Assert.Equal(3, market.Resale.Count);
        Assert.Contains(market.Resale, resale => resale.Ceiling > resale.Floor);
        Assert.Contains(market.Resale, resale => resale.Sold == 0);
    }
}
