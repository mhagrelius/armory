using Armory.Collections;
using Armory.Roster;
using Armory.Tally;
using Xunit;

namespace Armory.Tests.Collections;

/// <summary>Ported from <c>core/src/hunt.rs</c>.</summary>
public sealed class HuntTests
{
    private static Collectible Missing(long id, string name, string description) => new()
    {
        Kind = Kind.Mount,
        Id = id,
        Name = name,
        Source = Source.Drop,
        Description = description,
        LinkId = id,
    };

    private static Tallies Killed(params (string Name, long Count)[] who) => new()
    {
        [new CharacterKey("emerald-dream", "Somechar")] = who.Select(kill => new Armory.Tally.Tally { Kind = Counting.Victory, Key = kill.Name, Label = kill.Name, Count = kill.Count }).ToList(),
    };

    [Fact(DisplayName = "a_journal_sentence_names_the_creature_and_the_place")]
    public void A_journal_sentence_names_the_creature_and_the_place()
    {
        Assert.Equal(("Attumen the Huntsman", "Karazhan"), Hunt.DroppedBy("Drop: Attumen the Huntsman, Karazhan"));
        Assert.Equal(("Time-Lost Proto-Drake", (string?)null), Hunt.DroppedBy("Drop: Time-Lost Proto-Drake"));
        Assert.Null(Hunt.DroppedBy("Vendor: Katie Hunter, Elwynn Forest"));
        Assert.Null(Hunt.DroppedBy("Drop:"));
        Assert.Null(Hunt.DroppedBy("no colon at all"));
    }

    [Fact(DisplayName = "a_hunt_needs_a_thing_you_want_and_a_thing_you_have_fought")]
    public void A_hunt_needs_a_thing_you_want_and_a_thing_you_have_fought()
    {
        var catalogue = new[]
        {
            Missing(1, "Fiery Warhorse", "Drop: Attumen the Huntsman, Karazhan"),
            Missing(2, "Ashes of Al'ar", "Drop: Kael'thas Sunstrider, Tempest Keep"),
            Missing(3, "Swift White Hawkstrider", "Drop: Kael'thas Sunstrider, Magisters' Terrace"),
            Missing(4, "Invincible", "Drop: The Lich King, Icecrown Citadel"),
        };
        var hunts = Hunt.Hunting(catalogue, new HashSet<long> { 3 }, Killed(("Attumen the Huntsman", 47), ("Kael'thas Sunstrider", 12)));
        Assert.Equal(2, hunts.Count);
        Assert.Equal("Fiery Warhorse", hunts[0].Name);
        Assert.Equal(47, hunts[0].Attempts);
        Assert.Equal("Karazhan", hunts[0].Place);
        Assert.Equal(12, hunts[1].Attempts);
    }

    [Fact(DisplayName = "attempts_are_the_whole_account_because_the_collection_is")]
    public void Attempts_are_the_whole_account_because_the_collection_is()
    {
        var tallies = Killed(("Attumen the Huntsman", 30));
        tallies[new CharacterKey("mannoroth", "Aeltor")] = [new Armory.Tally.Tally { Kind = Counting.Victory, Key = "Attumen the Huntsman", Label = "Attumen the Huntsman", Count = 17 }];
        var hunts = Hunt.Hunting([Missing(1, "Fiery Warhorse", "Drop: Attumen the Huntsman, Karazhan")], new HashSet<long>(), tallies);
        Assert.Equal(47, hunts[0].Attempts);
    }

    [Fact(DisplayName = "an_account_with_no_journal_gets_nothing_rather_than_something_wrong")]
    public void An_account_with_no_journal_gets_nothing_rather_than_something_wrong()
    {
        var bare = Missing(1, "Fiery Warhorse", "") with { Description = null };
        Assert.Empty(Hunt.Hunting([bare], new HashSet<long>(), Killed(("Attumen the Huntsman", 47))));
    }
}
