using Armory.Addon;
using Armory.Chronicle;

namespace Armory.Client.Shell;

/// <summary>Everything the addon has written: the account file, and one per character.</summary>
public sealed record Dump
{
    public required Collected Collected { get; init; }

    public List<CollectedCharacter> Characters { get; init; } = [];

    /// <summary>
    /// Play sessions, from every character's file. The chronicle is a second
    /// saved variable of the same addon, so it lands in the same per-character
    /// file rather than a file of its own, which is why reading it costs
    /// nothing extra here and why the one watch already in place covers it.
    /// </summary>
    public List<Session> Sessions { get; init; } = [];
}

/// <summary>
/// Watching the collector addon's file.
/// </summary>
/// <remarks>
/// WoW writes SavedVariables at logout or <c>/reload</c> and at no other
/// time, so there is nothing to poll. The settle delay is not decoration: the
/// client writes the file in pieces and a watcher reports each write, so
/// reading on the first event reliably gets a truncated table, which the
/// parser correctly refuses, producing an error about a healthy addon.
/// </remarks>
public sealed class AddonWatch : IDisposable
{
    /// <summary>The addon's folder name, which is also its SavedVariables file name.</summary>
    public const string Addon = "Armory_Collector";

    /// <summary>How long to wait after the last write before reading. Long past the end of a logout's burst of appends.</summary>
    public static readonly TimeSpan Settle = TimeSpan.FromSeconds(2);

    private readonly FileSystemWatcher watcher;
    private readonly string wow;
    private readonly string account;
    private readonly Action<Result<Dump, ReadError>> deliver;
    private readonly TimeSpan settle;
    private CancellationTokenSource? pending;

    private AddonWatch(string wow, string account, Action<Result<Dump, ReadError>> deliver, TimeSpan settle, FileSystemWatcher watcher)
    {
        this.wow = wow;
        this.account = account;
        this.deliver = deliver;
        this.settle = settle;
        this.watcher = watcher;
    }

    /// <summary>
    /// Watch the account file, delivering whenever it settles. Delivers once
    /// immediately if the file is already there, so an application that
    /// starts after the game has quit does not wait for a write that already
    /// happened.
    /// </summary>
    public static Result<AddonWatch, string> Open(string wow, string account, Action<Result<Dump, ReadError>> deliver, TimeSpan? settle = null)
    {
        var path = AccountFile(wow, account);
        var directory = Path.GetDirectoryName(path);
        if (directory is null)
        {
            return Result<AddonWatch, string>.Err("no directory to watch");
        }
        try
        {
            Directory.CreateDirectory(directory);
            var watcher = new FileSystemWatcher(directory, Path.GetFileName(path))
            {
                NotifyFilter = NotifyFilters.LastWrite | NotifyFilters.Size | NotifyFilters.FileName | NotifyFilters.CreationTime,
            };
            var watch = new AddonWatch(wow, account, deliver, settle ?? Settle, watcher);
            watcher.Changed += (_, _) => watch.Touched();
            watcher.Created += (_, _) => watch.Touched();
            watcher.Renamed += (_, _) => watch.Touched();
            watcher.EnableRaisingEvents = true;
            if (File.Exists(path))
            {
                deliver(ReadAll(wow, account));
            }
            return Result<AddonWatch, string>.Ok(watch);
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or ArgumentException)
        {
            return Result<AddonWatch, string>.Err(error.Message);
        }
    }

    private void Touched()
    {
        // Every kind of change ends in the file being different, and the
        // settle below collapses the burst either way.
        var mine = new CancellationTokenSource();
        var previous = Interlocked.Exchange(ref pending, mine);
        previous?.Cancel();
        previous?.Dispose();
        Task.Delay(settle, mine.Token).ContinueWith(
            waited =>
            {
                if (!waited.IsCanceled)
                {
                    deliver(ReadAll(wow, account));
                }
            },
            TaskScheduler.Default);
    }

    /// <summary>Where the collector writes, given an install and an account.</summary>
    public static string AccountFile(string wow, string account) => Files.AccountSavedVariables(wow, account, Addon);

    /// <summary>
    /// Read the account file and every per-character file beside it. They
    /// are all written by the same logout, so one watch on the account file
    /// is enough to know the rest have changed. A character file that will
    /// not parse is skipped rather than failing the lot: losing one character
    /// off the roster is a much better outcome than losing the roster. The
    /// same goes one level down: a file whose collector half is unreadable
    /// may still have a readable chronicle in it, and there is no reason to
    /// throw away somebody's evenings over an unrelated table.
    /// </summary>
    public static Result<Dump, ReadError> ReadAll(string wow, string account)
    {
        var read = ReadAccount(AccountFile(wow, account));
        if (!read.IsOk)
        {
            return Result<Dump, ReadError>.Err(read.Error);
        }
        var characters = new List<CollectedCharacter>();
        var sessions = new List<Session>();
        foreach (var file in Files.CharacterFiles(wow, account, Addon))
        {
            byte[] bytes;
            try
            {
                bytes = File.ReadAllBytes(file);
            }
            catch (IOException)
            {
                continue;
            }
            catch (UnauthorizedAccessException)
            {
                continue;
            }
            var character = Collector.ReadCharacter(bytes);
            if (character.IsOk)
            {
                characters.Add(character.Value);
            }
            var recorded = ChronicleReader.Read(bytes);
            if (recorded.IsOk)
            {
                sessions.AddRange(recorded.Value);
            }
        }
        return Result<Dump, ReadError>.Ok(new Dump { Collected = read.Value, Characters = characters, Sessions = sessions });
    }

    /// <summary>Read and parse the collector's account-wide file.</summary>
    public static Result<Collected, ReadError> ReadAccount(string path)
    {
        try
        {
            return Collector.Read(File.ReadAllBytes(path));
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException)
        {
            return Result<Collected, ReadError>.Err(new ReadError.Unparsable($"could not read the file: {error.Message}"));
        }
    }

    /// <summary>Where the client writes screenshots.</summary>
    public static string ScreenshotDirectory(string wow) => Path.Combine(wow, "Screenshots");

    /// <summary>
    /// Every screenshot the client has written since <paramref name="since"/>,
    /// with the time it landed. The join the addon cannot make itself: no API
    /// reports the filename, so the addon records when it fired and this
    /// supplies the files by modification time.
    /// </summary>
    public static List<(DateTimeOffset At, string Path)> ScreenshotsSince(string wow, DateTimeOffset since)
    {
        var directory = ScreenshotDirectory(wow);
        if (!Directory.Exists(directory))
        {
            return [];
        }
        var shots = new List<(DateTimeOffset, string)>();
        foreach (var file in Directory.EnumerateFiles(directory))
        {
            var extension = Path.GetExtension(file);
            if (!(extension.Equals(".jpg", StringComparison.OrdinalIgnoreCase) || extension.Equals(".jpeg", StringComparison.OrdinalIgnoreCase)
                || extension.Equals(".png", StringComparison.OrdinalIgnoreCase) || extension.Equals(".tga", StringComparison.OrdinalIgnoreCase)))
            {
                continue;
            }
            var modified = new DateTimeOffset(File.GetLastWriteTimeUtc(file), TimeSpan.Zero);
            if (modified >= since)
            {
                shots.Add((modified, file));
            }
        }
        shots.Sort((a, b) => a.Item1.CompareTo(b.Item1));
        return shots;
    }

    public void Dispose()
    {
        watcher.EnableRaisingEvents = false;
        watcher.Dispose();
        var last = Interlocked.Exchange(ref pending, null);
        last?.Cancel();
        last?.Dispose();
    }
}
