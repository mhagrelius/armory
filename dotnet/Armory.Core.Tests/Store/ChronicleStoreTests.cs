using Armory.Chronicle;
using Armory.Roster;
using Xunit;

namespace Armory.Tests.Store;

/// <summary>The chronicle tests from <c>core/src/store.rs</c>.</summary>
public sealed class ChronicleStoreTests
{
    private static Session Evening(string name, int daysAgo)
    {
        var startedAt = DateTimeOffset.UtcNow - TimeSpan.FromDays(daysAgo);
        // Whole seconds, because the session key is written to the second.
        startedAt = DateTimeOffset.FromUnixTimeSeconds(startedAt.ToUnixTimeSeconds());
        return new Session
        {
            Character = new CharacterKey("emerald-dream", name),
            DisplayName = name,
            RealmName = "Emerald Dream",
            Class = "Druid",
            Race = "Tauren",
            Faction = Faction.Horde,
            StartedAt = startedAt,
            EndedAt = startedAt + TimeSpan.FromHours(2),
            StartLevel = 70,
            EndLevel = 70,
            StartMoney = 100,
            EndMoney = 200,
            StartItemLevel = 600,
            EndItemLevel = 600,
            Moments = [new Moment { At = 0, What = new Happening.Arrived("Nagrand", null, null) }],
        };
    }

    private static Entry AnEntry(Session session, string title) => new()
    {
        Session = session.Id,
        Title = title,
        Body = "The wind off the plains.",
        Model = "claude-opus-5",
        WrittenAt = DateTimeOffset.UtcNow,
    };

    [Fact(DisplayName = "sessions_round_trip_newest_first")]
    public void Sessions_round_trip_newest_first()
    {
        using var store = Armory.Store.Store.InMemory();
        Assert.True(store.SaveSessions([Evening("Somechar", 3), Evening("Velkurai", 1)]).IsOk);
        var sessions = store.SessionsHeld(10).Value;
        Assert.Equal(2, sessions.Count);
        Assert.Equal("Velkurai", sessions[0].DisplayName);
        Assert.Single(sessions[0].Moments);
    }

    [Fact(DisplayName = "an_evening_already_held_is_not_written_again")]
    public void An_evening_already_held_is_not_written_again()
    {
        using var store = Armory.Store.Store.InMemory();
        var seen = Evening("Somechar", 2);
        Assert.Equal(1, store.SaveSessions([seen]).Value);
        Assert.Equal(0, store.SaveSessions([seen]).Value);
        Assert.Equal(1, store.SaveSessions([seen, Evening("Somechar", 1)]).Value);
        Assert.Equal(2, store.SessionsHeld(10).Value.Count);
    }

    [Fact(DisplayName = "an_entry_replaces_the_one_before_it")]
    public void An_entry_replaces_the_one_before_it()
    {
        using var store = Armory.Store.Store.InMemory();
        var session = Evening("Somechar", 1);
        store.SaveSessions([session]);
        store.SaveEntry(AnEntry(session, "First Light"));
        store.SaveEntry(AnEntry(session, "Second Thoughts"));
        var entries = store.EntriesHeld().Value;
        Assert.Single(entries);
        Assert.Equal("Second Thoughts", entries[session.Id].Title);
    }

    [Fact(DisplayName = "an_evening_thrown_away_does_not_come_back_on_the_next_addon_read")]
    public void An_evening_thrown_away_does_not_come_back_on_the_next_addon_read()
    {
        using var store = Armory.Store.Store.InMemory();
        var session = Evening("Mattydormu", 4);
        store.SaveSessions([session]);
        Assert.Single(store.SessionsHeld(10).Value);

        store.ForgetSession(session.Id);
        Assert.Empty(store.SessionsHeld(10).Value);

        Assert.Equal(0, store.SaveSessions([session]).Value);
        Assert.Empty(store.SessionsHeld(10).Value);
    }

    [Fact(DisplayName = "forgetting_one_evening_does_not_refuse_the_others")]
    public void Forgetting_one_evening_does_not_refuse_the_others()
    {
        using var store = Armory.Store.Store.InMemory();
        var gone = Evening("Mattydormu", 4);
        var kept = Evening("Mattydormu", 3);
        store.SaveSessions([gone, kept]);
        store.ForgetSession(gone.Id);
        store.SaveSessions([gone, kept]);
        var held = Assert.Single(store.SessionsHeld(10).Value);
        Assert.Equal(kept.StartedAt, held.StartedAt);
    }

    [Fact(DisplayName = "a_journal_is_never_purged_because_it_is_not_the_apis_to_take_back")]
    public void A_journal_is_never_purged_because_it_is_not_the_apis_to_take_back()
    {
        using var store = Armory.Store.Store.InMemory();
        var ancient = Evening("Somechar", Armory.Store.Store.MaxTtlDays + 400);
        store.SaveSessions([ancient]);
        store.SaveEntry(AnEntry(ancient, "A Long Time Ago") with { WrittenAt = DateTimeOffset.UtcNow - TimeSpan.FromDays(Armory.Store.Store.MaxTtlDays + 400) });
        store.Purge();
        Assert.Single(store.SessionsHeld(10).Value);
        Assert.Single(store.EntriesHeld().Value);
    }

    [Fact(DisplayName = "forgetting_an_evening_takes_the_entry_with_it")]
    public void Forgetting_an_evening_takes_the_entry_with_it()
    {
        using var store = Armory.Store.Store.InMemory();
        var session = Evening("Somechar", 1);
        store.SaveSessions([session]);
        store.SaveEntry(AnEntry(session, "Best Forgotten"));
        store.ForgetSession(session.Id);
        Assert.Empty(store.SessionsHeld(10).Value);
        Assert.Empty(store.EntriesHeld().Value);
    }

    [Fact(DisplayName = "an evening is keyed the way the Rust keys it, so both machines file it once")]
    public void An_evening_is_keyed_the_way_the_rust_keys_it()
    {
        using var store = Armory.Store.Store.InMemory();
        var session = Evening("Somechar", 1) with { StartedAt = new DateTimeOffset(2026, 8, 3, 19, 0, 0, TimeSpan.Zero) };
        store.SaveSessions([session]);
        var row = Assert.Single(store.Context.Sessions);
        Assert.Equal("2026-08-03T19:00:00+00:00", row.StartedAt);
    }
}
