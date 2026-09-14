using Armory.Blizzard;
using Armory.Market;

namespace Armory.Client.Shell;

/// <summary>
/// The market, as rows. The GTK preview hands the market page its quotes,
/// crafts and resale figures ready-made; here the page computes them from
/// the store, so the sample seeds what those computations read — a
/// commodity snapshot, price series for the watched items, the recipe
/// books' reagents and outputs, and caged pets on a watched realm — and the
/// figures come out the same way they do for a real account.
/// </summary>
public static partial class Sample
{
    public const long EmeraldDream = 61;
    public const long Mannoroth = 13;

    /// <summary>The first hour of price history; nine days of it follow.</summary>
    private static readonly DateTimeOffset MarketFrom = At("2026-09-04T09:00:00Z");

    public static IReadOnlyList<(long Id, string Name)> WatchedRealms { get; } = [(EmeraldDream, "Emerald Dream"), (Mannoroth, "Mannoroth")];

    public static IReadOnlyList<(long Id, string Name)> WatchedItems { get; } = [(197_794, "Mycobloom"), (210_930, "Crystalline Powder")];

    /// <summary>Names for the browse table. Bismuth and Crystalline Powder are two items; the GTK sample's quotes gave one id both names.</summary>
    public static IReadOnlyList<(long Id, string Name)> ItemNames { get; } =
    [
        (197_794, "Mycobloom"),
        (211_880, "Algari Mana Potion"),
        (210_930, "Crystalline Powder"),
        (208_766, "Weavercloth"),
        (210_796, "Bismuth"),
        (212_283, "Flask of Alchemical Chaos"),
        (210_797, "Crystalline Powder (Quality 2)"),
        (219_500, "Weavercloth Bandage"),
    ];

    /// <summary>
    /// A realm's commodity market, as the browser sees it. One entry has not
    /// been named yet, because that is what most of the market looks like for
    /// the first few syncs and the page has to survive it.
    /// </summary>
    public static List<Depth> Listed()
    {
        (long ItemId, long Cheapest, long Quantity, long Listings)[] rows =
        [
            (197_794, 37_400, 4_120, 96),
            (211_880, 118_000, 1_880, 44),
            (210_930, 21_000, 12_400, 210),
            (208_766, 80_200, 940, 31),
            (210_796, 9_400, 44_100, 512),
            (212_283, 1_180_000, 310, 28),
            (210_797, 61_000, 1_900, 40),
            (219_873, 500, 4, 2),
        ];
        return rows.Select(row => new Depth
        {
            ItemId = row.ItemId,
            Cheapest = row.Cheapest,
            Quantity = row.Quantity,
            Listings = row.Listings,
            Tenth = row.Cheapest + row.Cheapest / 8,
            Median = row.Cheapest + row.Cheapest / 3,
        }).ToList();
    }

    /// <summary>
    /// Somechar's recipe book. The flask's second reagent has two quality
    /// tiers, one of them in the Warband bank, which is what puts a
    /// "held" line under the craft. The alchemist stone's output is listed
    /// nowhere, so it counts as unmeasured rather than as cheap.
    /// </summary>
    public static RecipeBooks Recipes() => new()
    {
        [Somechar] =
        [
            new Recipe
            {
                Id = 371_637,
                Name = "Flask of Alchemical Chaos",
                Output = 212_283,
                Makes = 1,
                Reagents = [new Reagent { Quantity = 6, Tiers = [197_794] }, new Reagent { Quantity = 2, Tiers = [210_930, 210_797] }],
            },
            new Recipe
            {
                Id = 370_582,
                Name = "Algari Mana Potion",
                Output = 211_880,
                Makes = 5,
                Reagents = [new Reagent { Quantity = 3, Tiers = [197_794] }],
            },
            new Recipe
            {
                Id = 391_012,
                Name = "Sanctified Alchemist Stone",
                Output = 213_000,
                Makes = 1,
                Reagents = [new Reagent { Quantity = 1, Tiers = [210_930] }],
            },
        ],
        [Velkurai] =
        [
            new Recipe { Id = 446_020, Name = "Weavercloth Bandage", Output = 219_500, Makes = 1, Reagents = [new Reagent { Quantity = 2, Tiers = [208_766] }] },
        ],
    };

    /// <summary>
    /// Price history, one book per day for nine days: what <c>record_prices</c>
    /// would have written from nine hourly dumps that moved. Quantities fall
    /// between samples, because a fall is the only evidence anything sold.
    /// </summary>
    public static List<(long Realm, DateTimeOffset At, List<Depth> Book)> PriceBooks()
    {
        var books = new List<(long, DateTimeOffset, List<Depth>)>();
        for (var day = 0; day < 10; day++)
        {
            var at = MarketFrom.AddDays(day);
            // Region-wide commodities: the watched items and the recipes' reagents and outputs.
            books.Add((0, at,
            [
                Commodity(197_794, 54_000 + day * 800, 4_800 - day * 70),
                Commodity(210_930, 21_000 - day * 300, 12_400 - day * 40),
                Commodity(210_797, 61_000, 1_900 - day * 10),
                Commodity(211_880, 118_000 + (day % 3) * 4_000, 2_600 - day * 80),
                Commodity(212_283, 1_180_000 + (day % 2) * 60_000, 380 - day * 12),
                Commodity(208_766, 80_200 - day * 500, 940 + (day % 2 == 0 ? 20 : -30)),
                Commodity(219_500, 4_100 + day * 50, 900 - day * 30),
            ]));
            // Emerald Dream: the gear-side watch on Crystalline Powder, and the caged pets.
            books.Add((EmeraldDream, at,
            [
                Commodity(210_930, 1_240_000 - day * 6_000, 40 - day),
                Pet(1, 3, 8_400_000, 6 - day % 3),
                Pet(2, 1, 900_000, 3 - day % 2),
                Pet(2, 4, 22_000_000, 1),
                Pet(3, 2, 310_000, 4),
            ]));
            // Mannoroth: one reading only, on the first day.
            if (day == 0)
            {
                books.Add((Mannoroth, at, [Commodity(210_930, 980_000, 12)]));
            }
        }
        return books;
    }

    private static Depth Commodity(long itemId, long cheapest, long quantity) => new()
    {
        ItemId = itemId,
        Cheapest = cheapest,
        Quantity = quantity,
        Listings = Math.Max(quantity / 40, 1),
        Tenth = cheapest + cheapest / 8,
        Median = cheapest + cheapest / 3,
    };

    /// <summary>A caged pet: item 82800, every one of them, told apart by the species and quality in the variant.</summary>
    private static Depth Pet(long species, long quality, long price, long quantity) => new()
    {
        ItemId = Listing.CagedPet,
        Variant = new Listing { ItemId = Listing.CagedPet, PetSpecies = species, PetQuality = quality }.Series(),
        Cheapest = price,
        Quantity = quantity,
        Listings = quantity,
        Tenth = price,
        Median = price,
    };
}
