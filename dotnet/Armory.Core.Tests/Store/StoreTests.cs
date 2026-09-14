using Armory.Store;
using Microsoft.EntityFrameworkCore;
using Xunit;

namespace Armory.Tests.Store;

/// <summary>A clock that says what it is told to, for the tests that backdate.</summary>
internal sealed class FixedClock : TimeProvider
{
    public DateTimeOffset Now { get; set; } = new(2026, 9, 14, 12, 0, 0, TimeSpan.Zero);

    public override DateTimeOffset GetUtcNow() => Now;
}

/// <summary>
/// Ported from the response-cache and watch-list tests in
/// <c>core/src/store.rs</c>. The rest of that file's tests land with the
/// slices whose writers they exercise.
/// </summary>
public sealed class StoreTests
{
    private static readonly TimeSpan Zero = TimeSpan.Zero;

    [Fact(DisplayName = "a_fresh_body_comes_back_and_a_stale_one_does_not")]
    public void A_fresh_body_comes_back_and_a_stale_one_does_not()
    {
        using var store = Armory.Store.Store.InMemory();
        store.StoreResponse("https://example/x", "hello"u8.ToArray(), null);

        Assert.Equal("hello"u8.ToArray(), store.Response("https://example/x", TimeSpan.FromHours(1)).Value);
        // A zero-length window makes everything already stale.
        Assert.Null(store.Response("https://example/x", Zero).Value);
    }

    [Fact(DisplayName = "purging_drops_what_is_past_its_ttl_and_keeps_what_is_not")]
    public void Purging_drops_what_is_past_its_ttl_and_keeps_what_is_not()
    {
        using var store = Armory.Store.Store.InMemory();
        store.StoreResponse("https://example/fresh", "new"u8.ToArray(), null);
        store.StoreResponse("https://example/ancient", "old"u8.ToArray(), null);

        // Backdate one past the limit the terms set.
        var longAgo = Armory.Store.Store.Stamp(DateTimeOffset.UtcNow - TimeSpan.FromDays(Armory.Store.Store.MaxTtlDays + 1));
        store.Context.Responses
            .Where(row => row.Url == "https://example/ancient")
            .ExecuteUpdate(set => set.SetProperty(row => row.FetchedAt, longAgo));

        Assert.Equal(1, store.Purge().Value);
        Assert.NotNull(store.Response("https://example/fresh", TimeSpan.FromHours(1)).Value);
        Assert.Null(store.Response("https://example/ancient", TimeSpan.FromHours(1)).Value);
    }

    [Fact(DisplayName = "a_family_of_responses_comes_back_together_and_respects_the_term")]
    public void A_family_of_responses_comes_back_together_and_respects_the_term()
    {
        using var store = Armory.Store.Store.InMemory();
        foreach (var url in new[]
        {
            "https://us.api.blizzard.com/data/wow/media/item/1?namespace=static-us",
            "https://us.api.blizzard.com/data/wow/media/item/2?namespace=static-us",
            "https://us.api.blizzard.com/data/wow/media/achievement/9?namespace=static-us",
            "https://us.api.blizzard.com/data/wow/toy/1?namespace=static-us",
        })
        {
            store.StoreResponse(url, "{}"u8.ToArray(), null);
        }

        var items = store.ResponsesMatching("/data/wow/media/item/", TimeSpan.FromDays(30)).Value;
        Assert.Equal(2, items.Count);

        // Backdate one past the term. Artwork is not exempt from it.
        var longAgo = Armory.Store.Store.Stamp(DateTimeOffset.UtcNow - TimeSpan.FromDays(Armory.Store.Store.MaxTtlDays + 1));
        store.Context.Responses
            .Where(row => row.Url.Contains("/data/wow/media/item/1"))
            .ExecuteUpdate(set => set.SetProperty(row => row.FetchedAt, longAgo));

        items = store.ResponsesMatching("/data/wow/media/item/", TimeSpan.FromDays(30)).Value;
        var only = Assert.Single(items);
        Assert.Contains("/item/2", only.Url, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "a_stamp_outlives_the_body_it_arrived_with")]
    public void A_stamp_outlives_the_body_it_arrived_with()
    {
        // A body we consider stale is still the body the server will confirm
        // with a 304, so the stamp has to survive the freshness window.
        using var store = Armory.Store.Store.InMemory();
        store.StoreResponse("https://example/x", "hello"u8.ToArray(), "Wed, 21 Oct 2026 07:28:00 GMT");

        Assert.Null(store.Response("https://example/x", Zero).Value);
        Assert.Equal("Wed, 21 Oct 2026 07:28:00 GMT", store.LastModified("https://example/x").Value);
    }

    [Fact(DisplayName = "a_not_modified_restarts_the_clock_without_rewriting_the_body")]
    public void A_not_modified_restarts_the_clock_without_rewriting_the_body()
    {
        var clock = new FixedClock();
        using var store = Armory.Store.Store.InMemory(clock);
        store.StoreResponse("https://example/x", "hello"u8.ToArray(), "stamp");

        clock.Now += TimeSpan.FromDays(10);
        Assert.Null(store.Response("https://example/x", TimeSpan.FromDays(1)).Value);

        store.TouchResponse("https://example/x");
        Assert.Equal("hello"u8.ToArray(), store.Response("https://example/x", TimeSpan.FromDays(1)).Value);
    }

    [Fact(DisplayName = "the ttl a caller asks for is capped at the term")]
    public void The_ttl_a_caller_asks_for_is_capped_at_the_term()
    {
        var clock = new FixedClock();
        using var store = Armory.Store.Store.InMemory(clock);
        store.StoreResponse("https://example/x", "hello"u8.ToArray(), null);
        clock.Now += TimeSpan.FromDays(Armory.Store.Store.MaxTtlDays + 1);
        Assert.Null(store.Response("https://example/x", TimeSpan.FromDays(365)).Value);
    }

    [Fact(DisplayName = "watching_is_opt_in_and_round_trips")]
    public void Watching_is_opt_in_and_round_trips()
    {
        using var store = Armory.Store.Store.InMemory();
        Assert.Empty(store.WatchedItems().Value);
        Assert.Empty(store.WatchedRealmList().Value);

        store.WatchItem(197794, "Mycobloom");
        store.WatchRealm(61, "Emerald Dream");
        Assert.Equal([(197794L, "Mycobloom")], store.WatchedItems().Value);
        Assert.Equal([(61L, "Emerald Dream")], store.WatchedRealmList().Value);

        store.UnwatchItem(197794);
        store.UnwatchRealm(61);
        Assert.Empty(store.WatchedItems().Value);
        Assert.Empty(store.WatchedRealmList().Value);
    }

    [Fact(DisplayName = "a_store_survives_being_reopened")]
    public void A_store_survives_being_reopened()
    {
        var directory = Directory.CreateTempSubdirectory("armory-");
        try
        {
            var path = Path.Combine(directory.FullName, "armory.db");
            using (var store = Armory.Store.Store.Open(path).Value)
            {
                store.WatchItem(4306, "Silk Cloth");
            }
            using var reopened = Armory.Store.Store.Open(path).Value;
            Assert.Single(reopened.WatchedItems().Value);
            Assert.True(reopened.IsRecording());
        }
        finally
        {
            directory.Delete(recursive: true);
        }
    }

    [Fact(DisplayName = "a database that cannot be opened is an error rather than an exception")]
    public void A_database_that_cannot_be_opened_is_an_error_rather_than_an_exception()
    {
        var opened = Armory.Store.Store.Open(Path.Combine(Path.GetTempPath(), "no-such-dir-" + Guid.NewGuid(), "armory.db"));
        Assert.False(opened.IsOk);
    }
}
