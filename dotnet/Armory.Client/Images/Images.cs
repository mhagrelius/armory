using System.Security.Cryptography;
using System.Text;
using Armory.Blizzard;
using Armory.Client.Blizzard;

namespace Armory.Client.Images;

/// <summary>
/// Blizzard's art, fetched once and kept small.
/// </summary>
/// <remarks>
/// <para>Every picture in the application comes from
/// <c>render.worldofwarcraft.com</c>. That host is not an API host: it takes
/// no token, answers no namespace and does not spend the client's request
/// quota, which is why a collection of sixteen hundred mounts can be
/// illustrated at all. <see cref="Media"/> decides <i>which</i> URL; this
/// fetches it.</para>
/// <para><b>Kept on disk.</b> The bytes go under the cache directory, keyed by
/// a hash of the URL, so a second launch draws the same grid without touching
/// the network. <see cref="Purge"/> sweeps anything a month old, the same
/// horizon the store keeps for API responses.</para>
/// <para><b>Asked for once.</b> A URL already in flight collects further
/// callers rather than starting a second request, and one that has answered
/// 403, which is what the service says for art that was never published, is
/// not asked again this session.</para>
/// <para>What comes back is bytes rather than a decoded picture: decoding at
/// the size a cell draws is the window's business, and the window keeps its
/// own decoded pictures in an <see cref="Lru{TKey, TValue}"/> keyed by URL
/// and size, because the same render wanted small in a grid and large in a
/// dialog is two different pictures.</para>
/// </remarks>
public sealed class Images
{
    /// <summary>How many fetched bodies to keep in memory. A few hundred renders at forty kilobytes is a handful of megabytes.</summary>
    public const int CacheSize = 512;

    /// <summary>How long a cached image file lives: the same thirty days the store keeps API responses for, because one horizon is easier to reason about than two.</summary>
    public static readonly TimeSpan MaxAge = TimeSpan.FromDays(30);

    private readonly Http http;
    private readonly string directory;
    private readonly TimeProvider clock;
    private readonly object gate = new();
    private readonly Lru<string, byte[]> cache = new(CacheSize);
    private readonly Dictionary<string, Task<byte[]?>> pending = [];
    private readonly HashSet<string> refused = [];

    /// <summary>
    /// Its own <see cref="Http"/>, because the render service is not an API
    /// host and image traffic in the same rate bucket as a sync would make a
    /// scrolling grid slow the sync down.
    /// </summary>
    public Images(Http http, string directory, TimeProvider? clock = null)
    {
        this.http = http;
        this.directory = directory;
        this.clock = clock ?? TimeProvider.System;
        try
        {
            Directory.CreateDirectory(directory);
        }
        catch (IOException)
        {
        }
        catch (UnauthorizedAccessException)
        {
        }
    }

    /// <summary>Where the bytes for one URL live. A URL is not a filename: it carries slashes, query strings and, for a portrait, a hash longer than some filesystems allow.</summary>
    public string PathOf(string url) =>
        Path.Combine(directory, Convert.ToHexStringLower(SHA256.HashData(Encoding.UTF8.GetBytes(url))) + ".img");

    /// <summary>
    /// The bytes for <paramref name="url"/>, now or when they arrive, or null
    /// for a picture that cannot be had. Never a failure: the caller leaves
    /// whatever it was showing in place, which for every caller is a
    /// placeholder that already reads correctly.
    /// </summary>
    public Task<byte[]?> Load(string url)
    {
        lock (gate)
        {
            if (cache.Get(url) is { } held)
            {
                return Task.FromResult<byte[]?>(held);
            }
            if (refused.Contains(url))
            {
                return Task.FromResult<byte[]?>(null);
            }
            // Already asked for. Join the queue rather than sending a second
            // request for the same bytes: a grid binding forty cells against
            // the same placeholder would otherwise send forty.
            if (pending.TryGetValue(url, out var inFlight))
            {
                return inFlight;
            }
            var task = Fetch(url);
            if (!task.IsCompleted)
            {
                pending[url] = task;
            }
            return task;
        }
    }

    private async Task<byte[]?> Fetch(string url)
    {
        // On disk from a previous launch.
        var path = PathOf(url);
        if (File.Exists(path))
        {
            byte[]? kept = null;
            try
            {
                kept = await File.ReadAllBytesAsync(path).ConfigureAwait(false);
            }
            catch (IOException)
            {
            }
            catch (UnauthorizedAccessException)
            {
            }
            if (kept is { Length: > 0 })
            {
                return Finish(url, kept, write: false);
            }
        }

        var outcome = await http.Fetch(Request.Get(SourceId.BlizzardGameData, url)).ConfigureAwait(false);
        if (outcome is Outcome<Response>.Found found && found.Value.Body.Length > 0)
        {
            return Finish(url, found.Value.Body, write: true);
        }
        // Anything else is art that is not there. The render service answers
        // 403 for a texture that was never published, which the client reads
        // as a privacy refusal: true of the API, not of a CDN, and either way
        // the answer is the same, no picture.
        lock (gate)
        {
            refused.Add(url);
            pending.Remove(url);
        }
        return null;
    }

    /// <summary>Cache, write through, and answer everyone waiting.</summary>
    private byte[] Finish(string url, byte[] bytes, bool write)
    {
        if (write)
        {
            try
            {
                File.WriteAllBytes(PathOf(url), bytes);
            }
            catch (IOException)
            {
            }
            catch (UnauthorizedAccessException)
            {
            }
        }
        lock (gate)
        {
            cache.Put(url, bytes);
            pending.Remove(url);
        }
        return bytes;
    }

    /// <summary>
    /// Bytes that turned out not to decode are not worth keeping or
    /// retrying. A truncated file from an interrupted launch lands here, and
    /// deleting it is what lets the next launch fetch it properly.
    /// </summary>
    public void Refuse(string url)
    {
        lock (gate)
        {
            refused.Add(url);
            cache.Remove(url);
        }
        try
        {
            File.Delete(PathOf(url));
        }
        catch (IOException)
        {
        }
        catch (UnauthorizedAccessException)
        {
        }
    }

    /// <summary>Drop cached art nobody has looked at in a month. Returns how many files went. Called at shutdown alongside the store's own sweep.</summary>
    public int Purge()
    {
        if (!Directory.Exists(directory))
        {
            return 0;
        }
        var now = clock.GetUtcNow();
        var removed = 0;
        foreach (var path in Directory.EnumerateFiles(directory))
        {
            try
            {
                if (now - File.GetLastWriteTimeUtc(path) > MaxAge)
                {
                    File.Delete(path);
                    removed++;
                }
            }
            catch (IOException)
            {
            }
            catch (UnauthorizedAccessException)
            {
            }
        }
        return removed;
    }
}

/// <summary>A least-recently-used map, sized in entries.</summary>
public sealed class Lru<TKey, TValue>
    where TKey : notnull
    where TValue : class
{
    private readonly int limit;
    private readonly Dictionary<TKey, LinkedListNode<(TKey Key, TValue Value)>> entries = [];
    private readonly LinkedList<(TKey Key, TValue Value)> order = new();

    public Lru(int limit)
    {
        this.limit = limit;
    }

    public int Count => entries.Count;

    public TValue? Get(TKey key)
    {
        if (!entries.TryGetValue(key, out var node))
        {
            return null;
        }
        order.Remove(node);
        order.AddLast(node);
        return node.Value.Value;
    }

    public void Put(TKey key, TValue value)
    {
        if (entries.TryGetValue(key, out var node))
        {
            order.Remove(node);
        }
        node = new LinkedListNode<(TKey, TValue)>((key, value));
        entries[key] = node;
        order.AddLast(node);
        while (order.Count > limit && order.First is { } oldest)
        {
            order.RemoveFirst();
            entries.Remove(oldest.Value.Key);
        }
    }

    public void Remove(TKey key)
    {
        if (entries.Remove(key, out var node))
        {
            order.Remove(node);
        }
    }
}
