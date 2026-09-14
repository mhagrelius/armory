using Armory.Blizzard;
using Armory.Chronicle;
using Armory.Client.Blizzard;
using Armory.Settings;

namespace Armory.Client.Shell;

/// <summary>
/// The chronicle: the evenings the addon recorded, and the journal written
/// about them, by a local llama-server or by the Claude Code command-line
/// signed in on this machine. The port of the GTK application's journal
/// half, with the continuations as awaited results.
/// </summary>
public sealed partial class Account
{
    /// <summary>How many evenings a page reads. Bounded because the page draws a card each.</summary>
    public const int SessionsShown = 60;

    private readonly List<SessionId> queued = [];
    private readonly HashSet<SessionId> writing = [];

    /// <summary>Runs a <see cref="Command"/> for the journal. The subprocess seam; a test hands in a fake.</summary>
    private readonly Func<Command, Task<Result<byte[], Reason>>> run;

    /// <summary>Whether something answered the readiness check: a model at the server's <c>/props</c>, or a signed-in Claude Code. Decides whether the page offers to write or to fix the setup.</summary>
    public bool JournalReady { get; private set; }

    /// <summary>Who answered: the model the server said it is running, or the plan and account Claude Code is signed in to.</summary>
    public string? JournalModel { get; private set; }

    /// <summary>Whether an entry for this evening is being written right now.</summary>
    public bool IsWriting(SessionId id) => writing.Contains(id);

    /// <summary>How many evenings are queued behind the one being written.</summary>
    public int QueuedEntries => queued.Count;

    /// <summary>
    /// The most recent evenings, newest first. Read back out of storage rather
    /// than held in memory: a journal is written once and read for years, and
    /// a copy of it beside the store buys nothing.
    /// </summary>
    public Task<List<Session>> Sessions() => Store.On(store => store.SessionsHeld(SessionsShown).Match(held => held, _ => []));

    /// <summary>Every entry written, keyed by the evening it is about.</summary>
    public Task<Dictionary<SessionId, Entry>> Entries() => Store.On(store => store.EntriesHeld().Match(held => held, _ => []));

    /// <summary>
    /// Screenshots taken since <paramref name="oldest"/>, and no earlier: a
    /// folder that has been filling up for a decade is thousands of files,
    /// and only these evenings' can possibly match.
    /// </summary>
    public List<(DateTimeOffset At, string Path)> Shots(DateTimeOffset oldest) =>
        Settings.WowPath is { } wow ? AddonWatch.ScreenshotsSince(wow, oldest) : [];

    /// <summary>
    /// The evenings, before anything else the addon wrote: they are the only
    /// thing the addon is the sole record of. The collector's tables can be
    /// rebuilt from a later logout, but the addon keeps its last forty
    /// sessions and drops the rest, so a session not filed now can be gone
    /// for good.
    /// </summary>
    private async Task KeepSessions(List<Session> sessions)
    {
        if (sessions.Count == 0)
        {
            return;
        }
        var kept = await Store.On(store => store.SaveSessions(sessions));
        if (!kept.IsOk)
        {
            Toasted?.Invoke($"Could not keep the sessions: {kept.Error}");
            return;
        }
        if (kept.Value == 0)
        {
            return;
        }
        Toasted?.Invoke(kept.Value == 1 ? "One new evening in the Chronicle" : $"{kept.Value} new evenings in the Chronicle");
        // Only if somebody asked for that. Spending the machine as a side
        // effect of the game writing a file is not something to do because
        // it would be convenient.
        if (Settings.JournalAutomatic)
        {
            await WriteAll();
        }
    }

    /// <summary>
    /// Ask the server what it is running, and remember the answer. One call,
    /// at startup and whenever the address changes. It is what puts a
    /// model's name on an entry: llama-server serves whatever it was
    /// launched with and ignores the request's <c>model</c> field entirely.
    /// Also the readiness check: nothing else can tell "no server running"
    /// from "server running and slow".
    /// </summary>
    /// <param name="server">An address to try instead of the saved one, so somebody can check one before committing to it.</param>
    /// <param name="backend">A backend to try instead of the saved one, for the same reason.</param>
    public async Task<string?> IdentifyJournal(string? server = null, JournalBackend? backend = null)
    {
        string? named;
        if ((backend ?? Settings.JournalBackend) == JournalBackend.ClaudeCode)
        {
            // Whether anybody is signed in, and who. The CLI holds the login;
            // this asks it once and never sees a token.
            named = (await run(Journal.AuthStatus())).Match(Journal.ParseAuthStatus, _ => null);
        }
        else
        {
            var outcome = await JournalHttp.Fetch(Journal.Identify(server ?? Settings.JournalServer));
            named = outcome is Outcome<Response>.Found found ? Journal.ParseIdentity(found.Value.Body) : null;
        }
        JournalReady = named is not null;
        JournalModel = named;
        Changed?.Invoke();
        return named;
    }

    /// <summary>Keep a new address and automatic flag, and a new backend and model where given, and find out what is answering now.</summary>
    public Task SaveJournalSettings(string server, bool automatic, JournalBackend? backend = null, string? model = null)
    {
        SaveSettings(Settings with
        {
            JournalServer = server.Trim().Length == 0 ? Journal.DefaultServer : server.Trim(),
            JournalAutomatic = automatic,
            JournalBackend = backend ?? Settings.JournalBackend,
            JournalModel = model is null ? Settings.JournalModel : model.Trim().Length == 0 ? Armory.Settings.Settings.DefaultJournalModel : model.Trim(),
        });
        return IdentifyJournal();
    }

    /// <summary>
    /// Write up one evening. Guarded against a second press while one is in
    /// flight: two entries for the same evening is one of them wasted, and a
    /// local model is busy for long enough that somebody will press again.
    /// </summary>
    public async Task WriteEntry(SessionId id)
    {
        // Already in flight, usually because somebody pressed a card's own
        // button and then asked for everything. Both of these keep the queue
        // moving rather than stalling it silently with entries left in it;
        // each id appears once, so this cannot loop.
        if (writing.Contains(id))
        {
            await WriteNext();
            return;
        }
        var session = (await Sessions()).FirstOrDefault(session => session.Id == id);
        if (session is null)
        {
            await WriteNext();
            return;
        }

        writing.Add(id);
        Changed?.Invoke();
        var claude = Settings.JournalBackend == JournalBackend.ClaudeCode;
        var written = claude
            ? (await run(Journal.Compose(Settings.JournalModel, session.Digest())))
                .Match(Journal.ParseComposed, why => new Outcome<Written>.Unusable(why))
            : (await JournalHttp.Fetch(Journal.Write(Settings.JournalServer, session.Digest()))) switch
            {
                Outcome<Response>.Found found => Journal.ParseWritten(found.Value.Body),
                Outcome<Response>.Empty => new Outcome<Written>.Empty(),
                Outcome<Response>.Unchanged => new Outcome<Written>.Unchanged(),
                Outcome<Response>.Unusable unusable => new Outcome<Written>.Unusable(unusable.Why),
                Outcome<Response>.Stale stale => new Outcome<Written>.Stale(stale.Why),
                _ => new Outcome<Written>.Empty(),
            };
        writing.Remove(id);

        switch (written)
        {
            case Outcome<Written>.Found found:
                var entry = new Entry
                {
                    Session = id,
                    Title = found.Value.Title,
                    Body = found.Value.Body,
                    // Claude Code's result names the model that wrote. A
                    // llama-server's does not: its completion echoes whatever
                    // the request asked for, so /props's answer stands in.
                    Model = claude ? found.Value.Model : JournalModel ?? found.Value.Model,
                    WrittenAt = clock.GetUtcNow(),
                };
                var saved = await Store.On(store => store.SaveEntry(entry));
                if (!saved.IsOk)
                {
                    Toasted?.Invoke($"Could not keep the entry: {saved.Error}");
                }
                Changed?.Invoke();
                await WriteNext();
                break;
            // A model that answered with nothing is not a failure to report
            // as one, but it is also not an entry.
            case Outcome<Written>.Empty or Outcome<Written>.Unchanged:
                Toasted?.Invoke("The entry came back empty");
                AbandonQueue();
                break;
            case Outcome<Written>.Unusable unusable:
                Toasted?.Invoke($"No entry written: {unusable.Why}");
                AbandonQueue();
                break;
            case Outcome<Written>.Stale stale:
                Toasted?.Invoke($"No entry written: {stale.Why}");
                AbandonQueue();
                break;
        }
    }

    /// <summary>
    /// Stop a run of entries after one of them failed. A server that is not
    /// running fails identically thirty times in a row, and thirty toasts
    /// saying so is worse than one.
    /// </summary>
    public void AbandonQueue()
    {
        var abandoned = queued.Count;
        queued.Clear();
        if (abandoned > 0)
        {
            Toasted?.Invoke($"Stopped — {abandoned} evenings left unwritten");
        }
        Changed?.Invoke();
    }

    /// <summary>
    /// Write up everything that has not been written yet. Deliberately
    /// sequential rather than a fan-out: each entry starts the next, so
    /// pressing it once on a fresh install writes the oldest, then the next,
    /// and any failure stops the queue rather than repeating itself thirty
    /// times. Returns false when no model is answering, so the page can open
    /// the setup.
    /// </summary>
    public async Task<bool> WriteAll()
    {
        if (!JournalReady)
        {
            Toasted?.Invoke(Settings.JournalBackend == JournalBackend.ClaudeCode
                ? "Claude Code is not signed in — run claude, sign in, then test it in Journal Setup"
                : "No model is answering — check the address in Journal Setup");
            return false;
        }
        var entries = await Entries();
        var pending = (await Sessions())
            .Where(session => session.Digest().IsWorthWriting())
            .Select(session => session.Id)
            .Where(id => !entries.ContainsKey(id))
            .ToList();
        if (pending.Count == 0)
        {
            Toasted?.Invoke("Every evening is already written up");
            return true;
        }
        Toasted?.Invoke($"Writing {pending.Count} entries, one at a time");
        queued.Clear();
        queued.AddRange(pending);
        await WriteNext();
        return true;
    }

    /// <summary>The oldest queued evening first: `Sessions` is newest first, and the queue pops from the end.</summary>
    private async Task WriteNext()
    {
        if (queued.Count == 0)
        {
            return;
        }
        var id = queued[^1];
        queued.RemoveAt(queued.Count - 1);
        await WriteEntry(id);
    }

    /// <summary>
    /// Forget an evening: the record of it and anything written about it.
    /// The only thing here that cannot be undone by syncing again, because
    /// the addon keeps its last forty sessions and this one may have fallen
    /// off the end.
    /// </summary>
    public async Task ForgetSession(SessionId id)
    {
        var forgotten = await Store.On(store => store.ForgetSession(id));
        if (!forgotten.IsOk)
        {
            Toasted?.Invoke($"Could not forget the evening: {forgotten.Error}");
        }
        Changed?.Invoke();
    }
}
