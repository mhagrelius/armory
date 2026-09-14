using Armory.Addon;
using Armory.Collections;
using Armory.Provenance;
using Armory.Roster;
using Armory.Run;
using Armory.Sharing;
using Armory.Store;
using Armory.Tally;
using Xunit;

namespace Armory.Tests.Store;

/// <summary>The roster, collected-data and collectible tests from <c>core/src/store.rs</c>, and the replica tests that waited on them.</summary>
public sealed class CollectedStoreTests
{
    private static readonly CharacterKey Somechar = new("emerald-dream", "Somechar");

    private static Character Someone(string realm, string name) => new()
    {
        Key = new CharacterKey(realm, name),
        Id = 5,
        RealmId = 61,
        DisplayName = name,
        RealmName = realm,
        Level = 80,
        Class = "Druid",
        Race = "Tauren",
        Faction = Faction.Horde,
        WowAccountId = 11,
    };

    private static Armory.Tally.Tally Flasks(long count) =>
        new() { Kind = Counting.Recipe, Key = "371637", Label = "Flask of Alchemical Chaos", Count = count };

    private static Collectible ACollectible(Kind kind, long id, string name) =>
        new() { Kind = kind, Id = id, Name = name, LinkId = id };

    [Fact(DisplayName = "a_roster_round_trips")]
    public void A_roster_round_trips()
    {
        using var store = Armory.Store.Store.InMemory();
        var roster = new Armory.Roster.Roster([Someone("emerald-dream", "Somechar"), Someone("mannoroth", "Aeltor")]);
        Assert.True(store.SaveRoster(roster).IsOk);
        Assert.Equal(roster, store.RosterHeld().Value);
    }

    [Fact(DisplayName = "saving_a_roster_replaces_rather_than_merges")]
    public void Saving_a_roster_replaces_rather_than_merges()
    {
        using var store = Armory.Store.Store.InMemory();
        store.SaveRoster(new Armory.Roster.Roster([Someone("emerald-dream", "Somechar"), Someone("gone", "Ghost")]));
        store.SaveRoster(new Armory.Roster.Roster([Someone("emerald-dream", "Somechar")]));
        Assert.Equal(1, store.RosterHeld().Value.Count);
    }

    [Fact(DisplayName = "a_cohort_round_trips")]
    public void A_cohort_round_trips()
    {
        using var store = Armory.Store.Store.InMemory();
        var cohort = new Cohort([new CharacterKey("emerald-dream", "Somechar"), new CharacterKey("dalaran", "Moodivh")]);
        store.SaveCohort(cohort);
        Assert.Equal(cohort, store.CohortHeld().Value);
    }

    [Fact(DisplayName = "a detail round trips and absorbs")]
    public void A_detail_round_trips_and_absorbs()
    {
        using var store = Armory.Store.Store.InMemory();
        store.SaveDetail(Somechar, new Detail { ItemLevel = 641, Spec = "Restoration" });
        var held = store.Details().Value[Somechar];
        Assert.Equal(641, held.ItemLevel);
        Assert.Equal("Restoration", held.Spec);
    }

    [Fact(DisplayName = "a_tally_survives_an_addon_folder_being_cleared")]
    public void A_tally_survives_an_addon_folder_being_cleared()
    {
        using var store = Armory.Store.Store.InMemory();
        var collected = new Collected();
        collected.Tallies[Somechar] = [Flasks(412)];
        Assert.True(store.SaveCollected(collected).IsOk);

        // The addon comes back from zero.
        collected.Tallies[Somechar] = [Flasks(1)];
        Assert.True(store.SaveCollected(collected).IsOk);

        Assert.Equal(412, store.TalliesHeld().Value[Somechar][0].Count);
    }

    [Fact(DisplayName = "earned_reputation_survives_an_addon_that_started_counting_again")]
    public void Earned_reputation_survives_an_addon_that_started_counting_again()
    {
        using var store = Armory.Store.Store.InMemory();
        var collected = new Collected();
        collected.Earned[Somechar] = new Earned { Reputation = { [2170] = new EarnedReputation { Points = 21_000, Renown = 4, RenownSeen = 25, AccountWide = true } } };
        store.SaveCollected(collected);

        var fresh = new Collected();
        fresh.Earned[Somechar] = new Earned { Reputation = { [2170] = new EarnedReputation { Points = 500, Renown = 0, RenownSeen = 25, AccountWide = true } } };
        store.SaveCollected(fresh);

        var with = store.ProvenanceHeld().Value[Somechar].With(2170);
        Assert.Equal(21_000, with.Points);
        Assert.Equal(4, with.Renown);
    }

    [Fact(DisplayName = "currency_provenance_round_trips_with_its_flags")]
    public void Currency_provenance_round_trips_with_its_flags()
    {
        using var store = Armory.Store.Store.InMemory();
        var collected = new Collected();
        collected.Earned[Somechar] = new Earned
        {
            Currency = { [3_008] = new EarnedCurrency { Gained = 1_000, Earned = 600, TracksEarned = true, AccountWide = true, Transferable = true } },
        };
        store.SaveCollected(collected);

        var currency = store.ProvenanceHeld().Value[Somechar].Currency[3_008];
        Assert.True(currency.TracksEarned);
        Assert.True(currency.Transferable);
        Assert.Equal(Origin.Transferred, currency.Origin);
        Assert.Equal(600, currency.Creditable());
    }

    [Fact(DisplayName = "everything the collector reads round trips through the store")]
    public void Everything_the_collector_reads_round_trips_through_the_store()
    {
        using var store = Armory.Store.Store.InMemory();
        var collected = new Collected();
        collected.EarnedBy[1234] = Somechar;
        collected.Criteria[99] = CriterionKind.Quest(5);
        collected.Currencies[Somechar] = new Dictionary<long, long> { [1602] = 300 };
        collected.WarbandBank[2589] = 40;
        collected.PetsHeld[42] = 2;
        collected.Recipes[Somechar] =
        [
            new Armory.Market.Recipe { Id = 371_637, Name = "Flask", Output = 212_283, Makes = 1, Reagents = [new Armory.Market.Reagent { Quantity = 3, Tiers = [210_796, 210_799] }] },
        ];
        store.SaveCollected(collected);

        Assert.Equal(Somechar, store.Attributions().Value[1234]);
        Assert.Equal(CriterionKind.Quest(5), store.CriteriaKinds().Value[99]);
        Assert.Equal(300, store.CurrenciesHeld().Value[Somechar][1602]);
        Assert.Equal(40, store.WarbandBank().Value[2589]);
        Assert.Equal(2, store.PetsHeldCounts().Value[42]);
        var recipe = Assert.Single(store.RecipesHeld().Value[Somechar]);
        Assert.Equal([210_796L, 210_799L], recipe.Reagents[0].Tiers);
        Assert.Equal([210_796L, 210_799L, 212_283L], store.RecipeItems().Value.Order());
    }

    [Fact(DisplayName = "an_index_sync_does_not_take_the_artwork_off_a_mount")]
    public void An_index_sync_does_not_take_the_artwork_off_a_mount()
    {
        using var store = Armory.Store.Store.InMemory();
        var fromJournal = ACollectible(Kind.Mount, 6, "Brown Horse") with { Display = 2404, Icon = 132261, Description = "Vendor: Unger Statforth", LinkId = 458 };
        store.SaveCollectibles([fromJournal]);
        // The index knows a name and nothing else.
        store.SaveCollectibles([ACollectible(Kind.Mount, 6, "Brown Horse")]);

        var (catalogue, _) = store.CollectiblesHeld(Kind.Mount).Value;
        var entry = Assert.Single(catalogue);
        Assert.Equal(2404, entry.Display);
        Assert.Equal(132261, entry.Icon);
        Assert.Equal(458, entry.LinkId);
        Assert.NotNull(entry.Description);
    }

    [Fact(DisplayName = "a_toy_known_to_both_sources_is_one_toy")]
    public void A_toy_known_to_both_sources_is_one_toy()
    {
        using var store = Armory.Store.Store.InMemory();
        var fromJournal = ACollectible(Kind.Toy, 86571, "Kang's Bindstone") with { Icon = 134458 };
        var fromApi = ACollectible(Kind.Toy, 1153, "Kang's Bindstone");
        store.SaveCollectibles([fromJournal, fromApi]);
        store.SaveOwned(Kind.Toy, new HashSet<long> { 1153 });

        var (catalogue, owned) = store.CollectiblesHeld(Kind.Toy).Value;
        var entry = Assert.Single(catalogue);
        Assert.Equal(86571, entry.Id);
        Assert.Equal(134458, entry.Icon);
        Assert.Contains(86571L, owned);
        Assert.DoesNotContain(1153L, owned);
    }

    [Fact(DisplayName = "two_mounts_that_share_a_name_stay_two_mounts")]
    public void Two_mounts_that_share_a_name_stay_two_mounts()
    {
        using var store = Armory.Store.Store.InMemory();
        store.SaveCollectibles([ACollectible(Kind.Mount, 8, "White Stallion"), ACollectible(Kind.Mount, 9, "White Stallion")]);
        Assert.Equal(2, store.CollectiblesHeld(Kind.Mount).Value.Catalogue.Count);
    }

    [Fact(DisplayName = "owning_something_the_catalogue_has_not_reached_is_still_recorded")]
    public void Owning_something_the_catalogue_has_not_reached_is_still_recorded()
    {
        using var store = Armory.Store.Store.InMemory();
        store.SaveOwned(Kind.Toy, new HashSet<long> { 1, 2 });
        var (catalogue, owned) = store.CollectiblesHeld(Kind.Toy).Value;
        Assert.Empty(catalogue);
        Assert.Equal(2, owned.Count);
    }

    [Fact(DisplayName = "a_toy_is_joined_to_its_item_even_with_no_name_to_match_on")]
    public void A_toy_is_joined_to_its_item_even_with_no_name_to_match_on()
    {
        var fromJournal = ACollectible(Kind.Toy, 32566, "Muradin's Favor") with { Icon = 134458 };
        var fromApi = ACollectible(Kind.Toy, 4, "") with { LinkId = 32566 };
        var catalogue = new List<Collectible> { fromApi, fromJournal };
        var owned = new HashSet<long> { 4 };
        Armory.Store.Store.CollapseToys(catalogue, owned);
        var only = Assert.Single(catalogue);
        Assert.Equal(32566, only.Id);
        Assert.Equal("Muradin's Favor", only.Name);
        Assert.Equal([32566L], owned);
    }

    [Fact(DisplayName = "nameless_toys_are_not_folded_into_each_other")]
    public void Nameless_toys_are_not_folded_into_each_other()
    {
        var catalogue = new List<Collectible> { ACollectible(Kind.Toy, 1, ""), ACollectible(Kind.Toy, 2, ""), ACollectible(Kind.Toy, 3, "Kang's Bindstone") };
        var owned = new HashSet<long> { 1 };
        Armory.Store.Store.CollapseToys(catalogue, owned);
        Assert.Equal(3, catalogue.Count);
        Assert.Equal([1L], owned);
    }

    [Fact(DisplayName = "owning a thing and then not owning it enqueues each change once")]
    public void Owning_a_thing_and_then_not_owning_it_enqueues_each_change_once()
    {
        using var store = Armory.Store.Store.InMemory();
        store.SetMachine("one");
        store.SaveCollectibles([ACollectible(Kind.Mount, 6, "Brown Horse"), ACollectible(Kind.Mount, 7, "Ashes")]);
        store.Drain(store.HighWater());
        store.SaveOwned(Kind.Mount, new HashSet<long> { 6 });
        Assert.Equal(1, store.Queued().Value.Single().Count);
        store.Drain(store.HighWater());
        // The same owned set again is not news.
        store.SaveOwned(Kind.Mount, new HashSet<long> { 6 });
        Assert.Empty(store.Queued().Value);
    }

    [Fact(DisplayName = "a_counter_never_goes_backwards_whichever_side_is_behind")]
    public void A_counter_never_goes_backwards_whichever_side_is_behind()
    {
        // The rule the tallies exist under, across the wire.
        using var one = Armory.Store.Store.InMemory();
        one.SetMachine("one");
        using var two = Armory.Store.Store.InMemory();
        two.SetMachine("two");

        var ahead = new Collected();
        ahead.Tallies[Somechar] = [Flasks(412)];
        two.SaveCollected(ahead);

        var behind = new Collected();
        behind.Tallies[Somechar] = [Flasks(1)];
        one.SaveCollected(behind);

        var (parcel, through) = one.Outbox(10_000).Value;
        two.Apply(parcel, Recording.Off);
        one.Drain(through);

        Assert.Equal(412, two.TalliesHeld().Value[Somechar][0].Count);
    }

    /// <summary>
    /// The rule the whole log stands on: doing the same thing twice enqueues
    /// nothing the second time. The run and the snapshot join this test when
    /// their slices land.
    /// </summary>
    [Fact(DisplayName = "a_second_identical_write_enqueues_nothing")]
    public void A_second_identical_write_enqueues_nothing()
    {
        using var store = Armory.Store.Store.InMemory();
        store.SetMachine("one");
        var roster = new Armory.Roster.Roster([Someone("emerald-dream", "Somechar") with { Id = 1, RealmId = 1, RealmName = "Emerald Dream", Class = "Shaman", Race = "Orc", WowAccountId = 7 }]);

        var collected = new Collected();
        collected.Recipes[Somechar] =
        [
            new Armory.Market.Recipe { Id = 371_637, Name = "Flask of Alchemical Chaos", Output = 212_283, Makes = 1, Reagents = [new Armory.Market.Reagent { Quantity = 3, Tiers = [210_796, 210_799] }] },
        ];
        collected.EarnedBy[1234] = Somechar;
        collected.Criteria[99] = CriterionKind.Quest(5);
        collected.WarbandBank[2589] = 40;
        collected.PetsHeld[42] = 2;
        collected.Currencies[Somechar] = new Dictionary<long, long> { [1602] = 300 };
        collected.Tallies[Somechar] = [new Armory.Tally.Tally { Kind = Counting.Recipe, Key = "371637", Label = "Flask", Count = 12 }];

        var book = new[] { new Armory.Blizzard.Depth { ItemId = 197_794, Variant = "", Cheapest = 56_523, Quantity = 400, Listings = 3, Tenth = 56_523, Median = 57_000 } };
        var at = new DateTimeOffset(2026, 9, 14, 12, 0, 0, TimeSpan.Zero);

        store.SaveRoster(roster);
        store.SaveCollected(collected);
        store.SaveOwned(Kind.Mount, new HashSet<long> { 6 });
        store.RecordSnapshot(61, book, at);

        // Everything above is genuinely new, so it is genuinely queued.
        Assert.NotEmpty(store.Queued().Value);
        store.Drain(store.HighWater());

        // The same day's data arriving again is not news.
        store.SaveRoster(roster);
        store.SaveCollected(collected);
        store.SaveOwned(Kind.Mount, new HashSet<long> { 6 });
        // The same book an hour later: the stamp moves, and a stamp is never
        // the reason a row travels.
        store.RecordSnapshot(61, book, at.AddHours(1));

        var queued = store.Queued().Value;
        Assert.True(queued.Count == 0, "a repeat write enqueued " + string.Join(", ", queued.Select(q => $"{q.Scope}:{q.Count}")));
    }
}
