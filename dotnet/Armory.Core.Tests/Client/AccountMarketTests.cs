using System.Net;
using System.Text;
using Armory.Addon;
using Armory.Blizzard;
using Armory.Client.Shell;
using Armory.Collections;
using Armory.Market;
using Armory.Roster;
using Xunit;

namespace Armory.Tests.Client;

/// <summary>The market half of the orchestrator, against a real store and a Blizzard answered in-process.</summary>
public sealed class AccountMarketTests : IDisposable
{
    private static readonly CharacterKey Somechar = new("emerald-dream", "Somechar");
    private static readonly Token Signed = new() { Access = "abc", ExpiresIn = 86_400 };

    private readonly string directory = Directory.CreateTempSubdirectory("armory-market-").FullName;
    private readonly List<Account> accounts = [];

    public void Dispose()
    {
        foreach (var account in accounts)
        {
            account.Dispose();
        }
        try
        {
            Directory.Delete(directory, recursive: true);
        }
        catch (IOException)
        {
        }
    }

    private Account Make(HttpTests.Answering? blizzard = null)
    {
        var store = Armory.Store.Store.InMemory();
        store.SetMachine("one");
        var account = new Account(
            new StoreWorker(store),
            new MemorySecrets(),
            Path.Combine(directory, "settings.json"),
            work => work().GetAwaiter().GetResult(),
            handler: blizzard);
        accounts.Add(account);
        return account;
    }

    private static HttpResponseMessage Json(string body) => new(HttpStatusCode.OK) { Content = new StringContent(body, Encoding.UTF8, "application/json") };

    /// <summary>A Blizzard that answers each path with a canned body and 404 for the rest.</summary>
    private static HttpTests.Answering Blizzard(params (string Path, string Body)[] routes) => new(seen =>
    {
        foreach (var (path, body) in routes)
        {
            if (seen.Url.Contains(path, StringComparison.Ordinal))
            {
                return Json(body);
            }
        }
        return new HttpResponseMessage(HttpStatusCode.NotFound);
    });

    [Fact(DisplayName = "a market sync records the token, the whole snapshot, and a history for what is watched")]
    public async Task A_market_sync_records_the_token_the_snapshot_and_a_history_for_what_is_watched()
    {
        var blizzard = Blizzard(
            ("/data/wow/token/index", """{"price":2345600}"""),
            ("/data/wow/auctions/commodities", """{"auctions":[{"id":1,"item":{"id":197794},"quantity":20,"unit_price":56523,"time_left":"SHORT"},{"id":2,"item":{"id":2770},"quantity":400,"unit_price":300,"time_left":"LONG"}]}"""));
        var account = Make(blizzard);
        account.HoldToken(Signed);
        await account.WatchItem(197_794, "Mycobloom");

        await account.SyncMarket(Signed, account.Generation);

        Assert.Equal(2_345_600, account.TokenPrice);
        var view = await account.MarketNow();
        // The whole market is browsable, watched or not.
        Assert.Equal(2, view.Market.Count);
        Assert.Contains(view.Market, listed => listed.ItemId == 2770);
        // Only the watched item has a history, and it is region-wide.
        var quote = Assert.Single(view.Quotes);
        Assert.Equal(197_794, quote.ItemId);
        Assert.Equal(0, quote.Realm);
        Assert.Equal(56_523, quote.Latest!.Value.Price);
        Assert.Empty(await account.Store.On(store => store.PriceHistory(0, 2770).Value));
    }

    [Fact(DisplayName = "a_second_identical_write_enqueues_nothing (record_snapshot)")]
    public async Task A_second_identical_snapshot_enqueues_nothing()
    {
        var account = Make();
        List<Listing> listings = [new() { ItemId = 197_794, UnitPrice = 56_523, Quantity = 20 }, new() { ItemId = 2770, UnitPrice = 300, Quantity = 400 }];
        await account.Record(0, listings, new HashSet<long> { 197_794 });
        var queued = await account.Store.On(store => store.Queued().Value.Sum(scope => scope.Count));
        Assert.True(queued > 0, "the first snapshot travels");

        await account.Record(0, listings, new HashSet<long> { 197_794 });
        Assert.Equal(queued, await account.Store.On(store => store.Queued().Value.Sum(scope => scope.Count)));
    }

    [Fact(DisplayName = "worth making and worth selling are joined from the store")]
    public async Task Worth_making_and_worth_selling_are_joined_from_the_store()
    {
        var account = Make();
        var potion = new Recipe { Id = 2, Name = "Ordinary Potion", Output = 200, Makes = 1, Reagents = [new Reagent { Quantity = 1, Tiers = [901] }] };
        var books = new RecipeBooks { [Somechar] = [potion] };
        var collected = new Collected
        {
            Recipes = books,
            PetsHeld = { [1] = 3 },
            Collectibles = { new Collectible { Kind = Kind.Pet, Id = 1, Name = "Sprite Darter", LinkId = 1, Tradeable = true } },
        };
        await account.Collected(Result<Dump, ReadError>.Ok(new Dump
        {
            Collected = collected,
            Characters = [new CollectedCharacter { Character = new Character { Key = Somechar, DisplayName = "Somechar", RealmName = "Emerald Dream", Level = 80, Class = "Druid", Race = "Tauren", Faction = Faction.Horde } }],
        }));
        await account.Store.On(store => store.WatchRealm(61, "Emerald Dream"));

        // Two snapshots an hour apart, with stock falling: the falls are the sales.
        var watched = new HashSet<long>();
        List<Listing> before = [new() { ItemId = 901, UnitPrice = 100, Quantity = 500 }, new() { ItemId = 200, UnitPrice = 5_000, Quantity = 40 }];
        List<Listing> after = [new() { ItemId = 901, UnitPrice = 100, Quantity = 480 }, new() { ItemId = 200, UnitPrice = 5_000, Quantity = 22 }];
        List<Listing> pets = [new() { ItemId = Listing.CagedPet, UnitPrice = 9_000, Quantity = 2, PetSpecies = 1, PetQuality = 3 }];
        List<Listing> petsLater = [new() { ItemId = Listing.CagedPet, UnitPrice = 9_000, Quantity = 1, PetSpecies = 1, PetQuality = 3 }];
        await account.Record(0, before, watched);
        await account.Record(61, pets, watched);
        var later = DateTimeOffset.UtcNow.AddHours(1);
        await account.Store.On(store => store.RecordPrices(0, Auctions.DepthOf(after), later));
        await account.Store.On(store => store.RecordPrices(61, Auctions.DepthOf(petsLater), later));

        var view = await account.MarketNow();
        var flip = Assert.Single(view.Crafting.Worth);
        Assert.Equal("Ordinary Potion", flip.Name);
        Assert.Equal("Somechar", flip.ByName);
        Assert.Equal(100, flip.Cost);
        Assert.True(flip.Margin > 0);

        var spare = Assert.Single(view.Resale);
        Assert.Equal("Sprite Darter", spare.Name);
        Assert.Equal(2, spare.Spare);
        Assert.Equal("Emerald Dream", spare.RealmName);
        Assert.Equal(9_000, spare.Floor);
    }

    [Fact(DisplayName = "the realm list comes from the cache when held and needs a sign-in when not")]
    public async Task The_realm_list_comes_from_the_cache_when_held_and_needs_a_sign_in_when_not()
    {
        var index = """{"realms":[{"id":61,"name":"Mannoroth","slug":"mannoroth"},{"id":1567,"name":"Emerald Dream","slug":"emerald-dream"}]}""";
        var blizzard = Blizzard(("/data/wow/realm/index", index));
        var account = Make(blizzard);

        var signedOut = await account.RealmChoices();
        Assert.False(signedOut.IsOk);
        Assert.Contains("Sign in", signedOut.Error, StringComparison.Ordinal);

        account.HoldToken(Signed);
        var fetched = await account.RealmChoices();
        Assert.True(fetched.IsOk);
        Assert.Equal(2, fetched.Value.Realms.Count);
        Assert.Single(blizzard.Requests);

        // Held now, so a signed-out relaunch still has the list.
        account.HoldToken(null);
        var cached = await account.RealmChoices();
        Assert.True(cached.IsOk);
        Assert.Single(blizzard.Requests);
    }

    [Fact(DisplayName = "watching a realm watches the auction house it trades in, not the realm's own id")]
    public async Task Watching_a_realm_watches_the_auction_house_it_trades_in()
    {
        var blizzard = Blizzard(("/data/wow/realm/terenas", """{"id":1567,"name":"Terenas","connected_realm":{"href":"https://us.api.blizzard.com/data/wow/connected-realm/61?namespace=dynamic-us"}}"""));
        var account = Make(blizzard);
        var toasts = new List<string>();
        account.Toasted += toasts.Add;

        await account.WatchRealm(new Realm { Id = 1567, Name = "Terenas", Slug = "terenas" });
        Assert.Contains(toasts, toast => toast.Contains("Sign in", StringComparison.Ordinal));
        Assert.Empty(await account.Store.On(store => store.WatchedRealmList().Value));

        account.HoldToken(Signed);
        await account.WatchRealm(new Realm { Id = 1567, Name = "Terenas", Slug = "terenas" });
        var realm = Assert.Single(await account.Store.On(store => store.WatchedRealmList().Value));
        Assert.Equal((61L, "Terenas"), realm);
        Assert.Contains(toasts, toast => toast.StartsWith("Watching Terenas", StringComparison.Ordinal));
    }

    [Fact(DisplayName = "an item search is Blizzard's, and empty without a sign-in")]
    public async Task An_item_search_is_blizzards_and_empty_without_a_sign_in()
    {
        var blizzard = Blizzard(("/data/wow/search/item", """{"page":1,"results":[{"data":{"id":197794,"name":{"en_US":"Mycobloom","de_DE":"Pilzbluete"}}}]}"""));
        var account = Make(blizzard);
        Assert.Empty(await account.SearchItems("myco"));
        Assert.Empty(blizzard.Requests);

        account.HoldToken(Signed);
        var found = Assert.Single(await account.SearchItems("myco"));
        Assert.Equal((197_794L, "Mycobloom"), found);

        await account.WatchItem(found.Id, found.Name);
        await account.UnwatchItem(found.Id);
        Assert.Empty(await account.Store.On(store => store.WatchedItems().Value));
    }

    [Fact(DisplayName = "the art cache fetches a render once and a tile's URL is the display id")]
    public async Task The_art_cache_fetches_a_render_once()
    {
        var bytes = new byte[] { 1, 2, 3, 4 };
        var render = new HttpTests.Answering(_ => new HttpResponseMessage(HttpStatusCode.OK) { Content = new ByteArrayContent(bytes) });
        var account = Make(render);
        // Never the real cache: a picture left behind by a previous run would answer from disk and prove nothing.
        account.ArtDirectory = Path.Combine(directory, "cache");
        var mount = new Collectible { Kind = Kind.Mount, Id = 337, Name = "Rivendare's Deathcharger", LinkId = 17_481, Display = 10_995 };
        var url = account.ArtUrl(mount);
        Assert.Equal("https://render.worldofwarcraft.com/us/npcs/zoom/creature-display-10995.jpg", url);

        Assert.Equal(bytes, await account.Art.Load(url!));
        Assert.Equal(bytes, await account.Art.Load(url!));
        Assert.Single(render.Requests);

        // A toy's icon arrives by a media call; until one has, there is no picture and no guessed one.
        var toy = new Collectible { Kind = Kind.Toy, Id = 9, Name = "Kang's Bindstone", LinkId = 9 };
        Assert.Null(account.ArtUrl(toy));
        account.ToyArt[86_571] = "https://render.worldofwarcraft.com/us/icons/56/inv_misc_bindstone.jpg";
        Assert.NotNull(account.ArtUrl(toy with { LinkId = 86_571 }));
    }

    [Fact(DisplayName = "the odds are read from an installed Rarity once per install path")]
    public void The_odds_are_read_from_an_installed_rarity_once_per_install_path()
    {
        var wow = Path.Combine(directory, "_retail_");
        var db = Path.Combine(wow, "Interface", "AddOns", "Rarity", "DB");
        Directory.CreateDirectory(db);
        File.WriteAllText(Path.Combine(db, "Mounts.lua"), """
            local L = LibStub("AceLocale-3.0"):GetLocale("Rarity")
            Rarity.ItemDB.mounts = {
             ["Deathcharger's Reins"] = {
              cat = MOUNTS,
              type = MOUNT,
              method = NPC,
              name = L["Deathcharger's Reins"],
              spellId = 17481,
              itemId = 13335,
              npcs = { 45412 },
              chance = 100, -- Blind guess
             },
            }
            """);
        var account = Make();
        account.SaveSettings(account.Settings with { WowPath = wow });
        account.RefreshChances();
        var mount = new Collectible { Kind = Kind.Mount, Id = 337, Name = "Rivendare's Deathcharger", LinkId = 17_481 };
        Assert.Equal(100, account.Chances.OneIn(mount));
    }
}
