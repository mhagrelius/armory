using Armory.Blizzard;
using Armory.Client.Blizzard;
using Armory.Client.Images;
using Armory.Collections;

namespace Armory.Client.Shell;

/// <summary>
/// The pictures, and the odds. The image cache has its own client because
/// the render service is not an API host: no token, no namespace, no quota,
/// and image traffic in a sync's rate bucket would make a scrolling grid
/// slow the sync down. The odds are read out of an installed Rarity, never
/// shipped and never fetched.
/// </summary>
public sealed partial class Account
{
    private Images.Images? art;
    private Http? artHttp;
    private string? chancesFrom;

    /// <summary>Where the pictures are kept. The application's cache directory; a test points it at a temporary one before the first picture is asked for.</summary>
    public string ArtDirectory { get; set; } = Paths.CachePath;

    /// <summary>Blizzard's art, fetched once and kept under the cache directory.</summary>
    public Images.Images Art => art ??= new Images.Images(artHttp ??= new Http(handler), ArtDirectory);

    /// <summary>
    /// Drop chances from the Rarity addon installed beside the collector,
    /// or none. <c>chance = 100</c> means one in a hundred, and the figures
    /// are estimates read off Wowhead by Rarity's authors.
    /// </summary>
    public Chances Chances { get; private set; } = new([]);

    /// <summary>
    /// Re-read the odds from the install the settings name. Once per path:
    /// the files change only when Rarity updates, and scanning its database
    /// on every launch of the collection page would be a second of disk for
    /// nothing.
    /// </summary>
    public void RefreshChances()
    {
        var wow = Settings.WowPath;
        if (wow is null || wow == chancesFrom)
        {
            return;
        }
        chancesFrom = wow;
        Chances = Rarity.Read(wow);
    }

    /// <summary>
    /// The picture for one collectible, or none. A mount or a pet is a
    /// creature render addressed by its display id, which costs no request;
    /// a toy or a piece of decor is an item icon that arrives one media
    /// call at a time.
    /// </summary>
    public string? ArtUrl(Collectible entry) => entry.Kind switch
    {
        Kind.Mount or Kind.Pet => entry.Display is { } display ? Media.CreatureRender(Settings.Region, display) : null,
        _ => entry.KnownItemId() is { } item && ToyArt.TryGetValue(item, out var url) ? url : null,
    };

    /// <summary>Sweep the image cache on the way out, the same horizon as the store's.</summary>
    public void PurgeArt()
    {
        if (art is not null)
        {
            art.Purge();
        }
        else if (Directory.Exists(ArtDirectory))
        {
            // Never opened this session, but a previous one may have left a
            // month of files behind.
            new Images.Images(artHttp ??= new Http(handler), ArtDirectory).Purge();
        }
    }
}
