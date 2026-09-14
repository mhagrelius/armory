using System.Net;
using Armory.Client.Blizzard;
using Armory.Client.Images;
using Armory.Tests.Store;
using Xunit;

namespace Armory.Tests.Client;

/// <summary>Ported from <c>src/ui/images.rs</c>: the cache policy, and the fetch-once behaviour that had no test there.</summary>
public sealed class ImagesTests : IDisposable
{
    private readonly string directory = Directory.CreateTempSubdirectory("armory-images-").FullName;

    public void Dispose()
    {
        try
        {
            Directory.Delete(directory, recursive: true);
        }
        catch (IOException)
        {
        }
    }

    private static readonly byte[] Pixel = [0, 0, 0, 255];

    [Fact(DisplayName = "the_cache_forgets_the_least_recently_looked_at")]
    public void The_cache_forgets_the_least_recently_looked_at()
    {
        // Sized in entries rather than bytes, so the eviction order is the
        // whole of the policy and worth pinning down.
        var lru = new Lru<(string, int), byte[]>(2);
        static (string, int) Key(string name) => (name, 96);

        lru.Put(Key("a"), Pixel);
        lru.Put(Key("b"), Pixel);
        // Touching "a" makes "b" the oldest.
        Assert.NotNull(lru.Get(Key("a")));
        lru.Put(Key("c"), Pixel);

        Assert.True(lru.Get(Key("b")) is null, "the untouched one goes");
        Assert.NotNull(lru.Get(Key("a")));
        Assert.NotNull(lru.Get(Key("c")));
    }

    [Fact(DisplayName = "one_url_at_two_sizes_is_two_textures")]
    public void One_url_at_two_sizes_is_two_textures()
    {
        // A grid wants ninety-six pixels and a dialog wants three hundred.
        // Keying on the URL alone would hand the dialog the thumbnail.
        var lru = new Lru<(string, int), byte[]>(4);
        lru.Put(("https://render/x.jpg", 96), Pixel);
        Assert.NotNull(lru.Get(("https://render/x.jpg", 96)));
        Assert.Null(lru.Get(("https://render/x.jpg", 320)));
    }

    [Fact(DisplayName = "a url is fetched once, kept on disk, and served from disk on the next launch")]
    public async Task A_url_is_fetched_once_and_kept_on_disk()
    {
        var served = 0;
        var server = new HttpTests.Answering(_ =>
        {
            served++;
            return new HttpResponseMessage(HttpStatusCode.OK) { Content = new ByteArrayContent(Pixel) };
        });
        using var http = new Http(server);
        var images = new Images(http, directory);

        const string url = "https://render.worldofwarcraft.com/us/npcs/zoom/creature-display-2404.jpg";
        Assert.Equal(Pixel, await images.Load(url));
        Assert.Equal(Pixel, await images.Load(url));
        Assert.Equal(1, served);
        Assert.True(File.Exists(images.PathOf(url)));

        // A second launch: the same directory, a fresh process.
        var again = new Images(http, directory);
        Assert.Equal(Pixel, await again.Load(url));
        Assert.Equal(1, served);
    }

    [Fact(DisplayName = "art the service refuses is no picture, and is not asked for again this session")]
    public async Task Art_the_service_refuses_is_no_picture()
    {
        var server = new HttpTests.Answering(_ => new HttpResponseMessage(HttpStatusCode.Forbidden));
        using var http = new Http(server);
        var images = new Images(http, directory);

        const string url = "https://render.worldofwarcraft.com/us/npcs/zoom/creature-display-1.jpg";
        Assert.Null(await images.Load(url));
        Assert.Null(await images.Load(url));
        Assert.Single(server.Requests);
    }

    [Fact(DisplayName = "bytes that will not decode are dropped from disk rather than kept or retried")]
    public async Task Bytes_that_will_not_decode_are_dropped_from_disk()
    {
        var server = new HttpTests.Answering(_ => new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent("<html>maintenance</html>") });
        using var http = new Http(server);
        var images = new Images(http, directory);

        const string url = "https://render.worldofwarcraft.com/us/icons/56/inv_misc_questionmark.jpg";
        Assert.NotNull(await images.Load(url));
        Assert.True(File.Exists(images.PathOf(url)));

        // The window tried to decode it and could not.
        images.Refuse(url);
        Assert.False(File.Exists(images.PathOf(url)));
        Assert.Null(await images.Load(url));
        Assert.Single(server.Requests);
    }

    [Fact(DisplayName = "the sweep drops what is a month old and keeps what is not")]
    public void The_sweep_drops_what_is_a_month_old()
    {
        var clock = new FixedClock();
        using var http = new Http(new HttpTests.Answering(_ => new HttpResponseMessage(HttpStatusCode.OK)), clock);
        var images = new Images(http, directory, clock);

        var old = images.PathOf("https://render/old.jpg");
        var fresh = images.PathOf("https://render/fresh.jpg");
        File.WriteAllBytes(old, Pixel);
        File.WriteAllBytes(fresh, Pixel);
        File.SetLastWriteTimeUtc(old, (clock.GetUtcNow() - Images.MaxAge - TimeSpan.FromDays(1)).UtcDateTime);
        File.SetLastWriteTimeUtc(fresh, clock.GetUtcNow().UtcDateTime);

        Assert.Equal(1, images.Purge());
        Assert.False(File.Exists(old));
        Assert.True(File.Exists(fresh));
    }
}
