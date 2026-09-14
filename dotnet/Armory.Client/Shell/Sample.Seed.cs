using Armory.Blizzard;
using Armory.Collections;
using Armory.Store;

namespace Armory.Client.Shell;

/// <summary>
/// Writing the sample into a store through the store's own writers, so the
/// pages read it exactly as they read a real account. Every step is one the
/// application already takes — the addon read, a journal entry landing, a
/// sync recording a dump, a watch being added — and none of them is
/// bypassed with a raw insert.
/// </summary>
public static partial class Sample
{
    /// <summary>
    /// Seed one store with the whole sample. The first writer to refuse
    /// stops the seeding and is the answer; a half-seeded store would paint
    /// as a page with something missing, which is the one thing a preview
    /// must not do quietly.
    /// </summary>
    public static Result<Unit, StoreError> Seed(Store.Store store)
    {
        foreach (var step in Steps(store))
        {
            var outcome = step();
            if (!outcome.IsOk)
            {
                return outcome;
            }
        }
        return Result<Unit, StoreError>.Ok(Unit.Value);
    }

    private static IEnumerable<Func<Result<Unit, StoreError>>> Steps(Store.Store store)
    {
        // The roster and who is enrolled, then the detail the fan-out would have fetched.
        yield return () => store.SaveRoster(Roster());
        yield return () => store.SaveCohort(Cohort());
        foreach (var (key, detail) in Details())
        {
            yield return () => store.SaveDetail(key, detail);
        }

        // The catalogue and the collections, owned sets per kind — the
        // addon's read writes only the kinds it described, and so does this.
        var collected = Collected();
        yield return () => store.SaveAchievements(collected.Catalogue.Values);
        yield return () => store.SaveCollectibles(collected.Collectibles);
        foreach (var kind in Links.AllKinds)
        {
            yield return () => store.SaveOwned(kind, collected.Owned.Where(owned => owned.Kind == kind).Select(owned => owned.Id).ToHashSet());
        }
        yield return () => store.SaveCollected(collected);

        // The evenings, and the one that has been written up.
        var sessions = Sessions();
        yield return () => store.SaveSessions(sessions).Map(_ => Unit.Value);
        foreach (var entry in Entries(sessions).Values)
        {
            yield return () => store.SaveEntry(entry);
        }

        // The run, with the decisions a person made on it.
        yield return () => store.SaveRun(null, Run()).Map(_ => Unit.Value);

        // Reputations reach the account out of the response cache, so that
        // is where they go: one body per enrolled character, in Blizzard's shape.
        foreach (var (key, standings) in Standings())
        {
            yield return () => store.StoreResponse(Profile.ReputationsOf(Region.Us, key).Url, ReputationsBody(standings), null);
        }

        // The market: watches, names, the commodity snapshot, and nine days of prices.
        foreach (var (id, name) in WatchedRealms)
        {
            yield return () => store.WatchRealm(id, name);
        }
        foreach (var (id, name) in WatchedItems)
        {
            yield return () => store.WatchItem(id, name);
        }
        foreach (var (id, name) in ItemNames)
        {
            yield return () => store.NameFoundItem(id, name);
        }
        var books = PriceBooks();
        yield return () => store.RecordSnapshot(0, Listed(), books[^1].At);
        foreach (var (realm, at, book) in books)
        {
            yield return () => store.RecordPrices(realm, book, at).Map(_ => Unit.Value);
        }
    }
}
