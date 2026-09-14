using System.Text.Json;
using Armory.Chronicle;
using Armory.Roster;
using Microsoft.EntityFrameworkCore;

namespace Armory.Store;

/// <summary>Sessions, the entries written about them, and the evenings deliberately thrown away.</summary>
public sealed partial class Store
{
    /// <summary>
    /// File the sessions the addon has written, skipping any already held.
    /// Insert-or-ignore because a session is finished the moment it is
    /// written and never changes, but the addon keeps its last forty and
    /// rewrites the whole file at every logout. An evening thrown away, on
    /// this machine or another, is refused the moment that decision arrives.
    /// Returns how many were new.
    /// </summary>
    public Result<int, StoreError> SaveSessions(IEnumerable<Session> sessions) => Work(() =>
    {
        var added = 0;
        foreach (var session in sessions)
        {
            var realm = session.Character.RealmSlug;
            var name = session.Character.Name;
            var startedAt = Stamps.Rfc3339WithOffset(session.StartedAt);
            if (Context.Sessions.Any(row => row.RealmSlug == realm && row.Name == name && row.StartedAt == startedAt)
                || Context.Forgotten.Any(row => row.RealmSlug == realm && row.Name == name && row.StartedAt == startedAt))
            {
                continue;
            }
            Context.Sessions.Add(new SessionRow
            {
                RealmSlug = realm,
                Name = name,
                StartedAt = startedAt,
                EndedAt = Stamps.Rfc3339WithOffset(session.EndedAt),
                Json = JsonSerializer.Serialize(session),
            });
            added++;
        }
        return added;
    });

    /// <summary>The most recent sessions, newest first. Bounded because the page draws a card each. A row that will not deserialise is skipped.</summary>
    public Result<List<Session>, StoreError> SessionsHeld(int limit) => Work(() =>
    {
        var sessions = new List<Session>();
        foreach (var json in Context.Sessions.AsNoTracking()
                     .OrderByDescending(row => row.StartedAt).ThenBy(row => row.RealmSlug).ThenBy(row => row.Name)
                     .Take(limit)
                     .Select(row => row.Json))
        {
            try
            {
                if (JsonSerializer.Deserialize<Session>(json) is { } session)
                {
                    sessions.Add(session);
                }
            }
            catch (JsonException)
            {
                // One unreadable evening should not empty the journal.
            }
        }
        return sessions;
    });

    /// <summary>Every entry that has been written, keyed by the evening it is about.</summary>
    public Result<Dictionary<SessionId, Entry>, StoreError> EntriesHeld() => Work(() =>
    {
        var entries = new Dictionary<SessionId, Entry>();
        foreach (var row in Context.Entries.AsNoTracking())
        {
            if (ParseStamp(row.StartedAt) is not { } startedAt || ParseStamp(row.WrittenAt) is not { } writtenAt)
            {
                continue;
            }
            var id = new SessionId(new CharacterKey(row.RealmSlug, row.Name), startedAt);
            entries[id] = new Entry { Session = id, Title = row.Title, Body = row.Body, Model = row.Model, WrittenAt = writtenAt };
        }
        return entries;
    });

    /// <summary>Keep an entry, replacing any previous one for the same evening: asking for a second one is a deliberate act.</summary>
    public Result<Unit, StoreError> SaveEntry(Entry entry) => Work(() =>
    {
        var startedAt = Stamps.Rfc3339WithOffset(entry.Session.StartedAt);
        var held = Context.Entries.Find(entry.Session.Character.RealmSlug, entry.Session.Character.Name, startedAt);
        if (held is null)
        {
            Context.Entries.Add(new EntryRow
            {
                RealmSlug = entry.Session.Character.RealmSlug,
                Name = entry.Session.Character.Name,
                StartedAt = startedAt,
                Title = entry.Title,
                Body = entry.Body,
                Model = entry.Model,
                WrittenAt = Stamps.Rfc3339WithOffset(entry.WrittenAt),
            });
        }
        else
        {
            held.Title = entry.Title;
            held.Body = entry.Body;
            held.Model = entry.Model;
            held.WrittenAt = Stamps.Rfc3339WithOffset(entry.WrittenAt);
        }
    });

    /// <summary>
    /// Forget an evening: the record of it and anything written about it.
    /// The forgotten row is what makes it stick: without it the next addon
    /// read puts the evening straight back, which looks exactly like Forget
    /// doing nothing.
    /// </summary>
    public Result<Unit, StoreError> ForgetSession(SessionId id) => Work(() =>
    {
        var realm = id.Character.RealmSlug;
        var name = id.Character.Name;
        var startedAt = Stamps.Rfc3339WithOffset(id.StartedAt);
        Context.Entries.Where(row => row.RealmSlug == realm && row.Name == name && row.StartedAt == startedAt).ExecuteDelete();
        Context.Sessions.Where(row => row.RealmSlug == realm && row.Name == name && row.StartedAt == startedAt).ExecuteDelete();
        if (Context.Forgotten.Find(realm, name, startedAt) is null)
        {
            Context.Forgotten.Add(new ForgottenRow { RealmSlug = realm, Name = name, StartedAt = startedAt, At = Stamps.Rfc3339WithOffset(Clock.GetUtcNow()) });
        }
    });
}
