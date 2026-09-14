using System.Globalization;
using System.Text;
using System.Text.Json;
using Armory.Addon;
using Armory.Blizzard;
using Armory.Collections;
using Armory.Provenance;
using Armory.Roster;
using Armory.Run;
using Armory.Tally;

namespace Armory.Client.Shell;

/// <summary>
/// Made-up data for the offscreen preview: the port of the <c>sample_*</c>
/// builders in <c>examples/preview.rs</c>. The states worth a picture are the
/// ones that are awkward to reach on demand — a roster nobody has enrolled
/// from, a run part-way through with goals in every bucket, an evening with
/// almost nothing in it — and none of them is on any real machine when it is
/// wanted. Nothing here touches a window, so it can be seeded into a store in
/// a test as well as under the preview.
/// </summary>
/// <remarks>
/// The GTK preview hands each page its sample directly. The pages here read
/// the store, so the sample is written through the store's own writers
/// (<see cref="Seed"/>) and, for the planner, handed to the account as the
/// addon's read (<see cref="Dump"/>) — the goal rows carry no evaluation, so
/// a run with progress bars has to be planned rather than stored.
/// </remarks>
public static partial class Sample
{
    public static readonly CharacterKey Somechar = new("emerald-dream", "Somechar");
    public static readonly CharacterKey Atulak = new("emerald-dream", "Atulak");
    public static readonly CharacterKey Velkurai = new("emerald-dream", "Velkurai");
    public static readonly CharacterKey Aeltor = new("mannoroth", "Aeltor");
    public static readonly CharacterKey Ulahae = new("thrall", "Ulahae");

    /// <summary>When the run began. Everything completed after it is the run's; everything before it belongs to whoever earned it.</summary>
    public static readonly DateTimeOffset BaselineAt = At("2026-06-01T00:00:00Z");

    public static DateTimeOffset At(string text) => DateTimeOffset.Parse(text, CultureInfo.InvariantCulture, DateTimeStyles.AssumeUniversal);

    // -- the roster ---------------------------------------------------------------

    private static Character Character(string realm, string name, int level, string race, string @class) => new()
    {
        Key = new CharacterKey(Slug.RealmSlug(realm), name),
        Id = 1,
        RealmId = 2,
        DisplayName = name,
        RealmName = realm,
        Level = level,
        Class = @class,
        Race = race,
        Faction = Faction.Horde,
        WowAccountId = 1,
    };

    public static Armory.Roster.Roster Roster() => new(
    [
        Character("Emerald Dream", "Somechar", 80, "Tauren", "Druid"),
        Character("Emerald Dream", "Atulak", 80, "Orc", "Shaman"),
        Character("Emerald Dream", "Velkurai", 71, "Troll", "Mage"),
        Character("Mannoroth", "Aeltor", 80, "Orc", "Warrior"),
        Character("Mannoroth", "Silentbeef", 62, "Tauren", "Warrior"),
        Character("Dalaran", "Moodivh", 80, "Tauren", "Priest"),
        Character("Thrall", "Ulahae", 45, "Undead", "Warlock"),
    ]);

    public static Cohort Cohort() => new([Somechar, Velkurai, Ulahae]);

    private static Profession Profession(string name) => new()
    {
        Name = name,
        Tier = $"Khaz Algar {name}",
        Skill = 84,
        MaxSkill = 100,
        IsPrimary = true,
        Specialisations = [new Specialisation("Potion Mastery", true), new Specialisation("Phial Mastery", false)],
        Knowledge = 412,
    };

    /// <summary>
    /// Detail for the enrolled three only. The fan-out fetches nobody else, so
    /// the unenrolled rows have nothing to show and must not look broken for
    /// it. Somechar carries everything the character page draws — the GTK
    /// preview's <c>sample_life</c> — including the two facts that arrive
    /// from different sources: gear from the API, raids from the addon.
    /// </summary>
    public static Dictionary<CharacterKey, Detail> Details() => new()
    {
        [Somechar] = new Detail
        {
            ItemLevel = 642,
            EquippedItemLevel = 639,
            Spec = "Restoration",
            Guild = "Dream Team",
            Money = 91_234_560_000,
            MythicRating = 2418,
            AchievementPoints = 28_940,
            LastLogin = At("2026-08-05T01:22:00Z"),
            Professions = [Profession("Alchemy"), Profession("Herbalism")],
            Equipment = Equipment(),
            Raids =
            [
                new RaidTier
                {
                    Name = "Nerub-ar Palace",
                    Expansion = "The War Within",
                    Difficulties = [new RaidDifficulty { Name = "Heroic", Defeated = 8, Total = 8, LastKill = new Kill("Queen Ansurek", At("2026-02-11T22:10:00Z")) }],
                },
                new RaidTier
                {
                    Name = "Liberation of Undermine",
                    Expansion = "The War Within",
                    Difficulties =
                    [
                        new RaidDifficulty { Name = "Normal", Defeated = 8, Total = 8, LastKill = new Kill("Mug'Zee", At("2026-07-30T22:41:00Z")) },
                        new RaidDifficulty { Name = "Heroic", Defeated = 2, Total = 8, LastKill = new Kill("Vexie", At("2026-07-30T21:02:00Z")) },
                    ],
                },
            ],
            RaidLocks = [new RaidLock { Name = "Liberation of Undermine", Difficulty = "Heroic", Defeated = 2, Total = 8 }],
            Vault =
            [
                new VaultSlot { Row = VaultRow.Raids, Index = 1, Threshold = 2, Progress = 2, Level = 3, LevelName = "Heroic" },
                new VaultSlot { Row = VaultRow.Raids, Index = 2, Threshold = 4, Progress = 2, Level = 0 },
                new VaultSlot { Row = VaultRow.Dungeons, Index = 1, Threshold = 1, Progress = 1, Level = 10 },
                new VaultSlot { Row = VaultRow.Dungeons, Index = 2, Threshold = 4, Progress = 3, Level = 0 },
                new VaultSlot { Row = VaultRow.World, Index = 1, Threshold = 2, Progress = 2, Level = 8 },
            ],
            VaultReady = true,
        },
        [Velkurai] = new Detail
        {
            ItemLevel = 571,
            Spec = "Frost",
            Money = 4_821_000_000,
            Professions = [Profession("Tailoring")],
        },
        [Ulahae] = new Detail
        {
            ItemLevel = 112,
            Spec = "Affliction",
            Money = 123_000_000,
        },
    };

    private static List<Equipped> Equipment()
    {
        (string Slot, string Name, int? Level)[] worn =
        [
            ("NECK", "Amulet of Earthen Binding", 627),
            ("WRIST", "Coilfang Cuffs", 636),
            ("FEET", "Treads of the Mag'har", 639),
            ("WAIST", "Girdle of the Windrider", 639),
            ("FINGER_1", "Band of Oshu'gun", 642),
            ("HANDS", "Grips of Distant Thunder", 642),
            ("BACK", "Drape of the Kurenai", 645),
            ("TRINKET_1", "Spiritcaller's Totem", 645),
            ("LEGS", "Leggings of the Broken", 645),
            ("FINGER_2", "Signet of the Warsong", 649),
            ("HEAD", "Crown of the Dreamer", 649),
            ("SHOULDER", "Mantle of Deep Roots", 649),
            ("CHEST", "Robes of the Emerald Wake", 652),
            ("TRINKET_2", "Ephemeral Bloom", 652),
            ("MAIN_HAND", "Staff of the Wild Heart", 658),
            ("TABARD", "Tabard of the Kurenai", null),
        ];
        var names = Equipped.Slots.ToDictionary(slot => slot.Slot, slot => slot.Name);
        return worn.Select(piece => new Equipped
        {
            Slot = piece.Slot,
            SlotName = names.GetValueOrDefault(piece.Slot) ?? "Tabard",
            Name = piece.Name,
            Level = piece.Level,
        }).ToList();
    }

    // -- the collections ----------------------------------------------------------

    /// <summary>
    /// A handful of each kind. The mounts carry real boss names, because the
    /// journal's sentence is what a drop is joined to the account's own pull
    /// count by — see <see cref="Counters.AttemptsAt"/>. The pets are
    /// tradeable and held in spares, which is what <c>worth_selling</c>
    /// needs; the toys and decor are addressed by item, one of them with the
    /// guessed stand-in id the link rules refuse.
    /// </summary>
    public static List<Collectible> Collection()
    {
        (long Id, string Name, Source Source, string Whence)[] mounts =
        [
            (6, "Reins of the Onyxian Drake", Source.Drop, "Onyxia, Onyxia's Lair"),
            (7, "Swift Zulian Tiger", Source.Drop, "High Priest Thekal, Zul'Gurub"),
            (8, "Reins of the Grand Black War Mammoth", Source.Vendor, "Sold by Mei Francis, Dalaran"),
            (9, "Ashes of Al'ar", Source.Drop, "Kael'thas Sunstrider, Tempest Keep"),
            (10, "Mimiron's Head", Source.Drop, "Yogg-Saron, Ulduar"),
            (11, "Invincible's Reins", Source.Drop, "The Lich King, Icecrown Citadel"),
            (12, "Reins of the Raven Lord", Source.Unknown, ""),
            (13, "Fiery Warhorse's Reins", Source.Unknown, ""),
        ];
        var entries = mounts.Select(mount => new Collectible
        {
            Kind = Kind.Mount,
            Id = mount.Id,
            Name = mount.Name,
            Source = mount.Source,
            // What the in-game journal gives and the web API does not.
            Description = mount.Whence.Length > 0 ? $"{mount.Source.Label()}: {mount.Whence}" : null,
            LinkId = mount.Id * 100,
        }).ToList();

        (long Species, string Name, Source Source, string Whence, long Creature)[] pets =
        [
            (1, "Nether Faerie Dragon", Source.Drop, "Nether Faerie Dragon, Feralas", 41_046),
            (2, "Sprite Darter Hatchling", Source.Drop, "Sprite Darter, Feralas", 5_785),
            (3, "Tiny Crimson Whelpling", Source.Drop, "Crimson Whelp, Wetlands", 1_042),
            (4, "Anubisath Idol", Source.Drop, "Emperor Vek'lor, Temple of Ahn'Qiraj", 15_275),
        ];
        entries.AddRange(pets.Select(pet => new Collectible
        {
            Kind = Kind.Pet,
            Id = pet.Species,
            Name = pet.Name,
            Source = pet.Source,
            Description = $"{pet.Source.Label()}: {pet.Whence}",
            LinkId = pet.Creature,
            Tradeable = true,
        }));

        (long Id, string Name, Source Source, long Item)[] toys =
        [
            (301, "Kang's Bindstone", Source.Drop, 86_571),
            (302, "Blazing Wings", Source.Quest, 86_589),
            // The collection index's stand-in: no known item, so no link and no icon.
            (303, "Foam Sword Rack", Source.Vendor, 303),
        ];
        entries.AddRange(toys.Select(toy => new Collectible
        {
            Kind = Kind.Toy,
            Id = toy.Id,
            Name = toy.Name,
            Source = toy.Source,
            Description = $"{toy.Source.Label()}: somewhere in Pandaria",
            LinkId = toy.Item,
        }));

        (long Id, string Name, Source Source, long Item)[] decor =
        [
            (401, "Kul Tiran Armchair", Source.Vendor, 234_001),
            (402, "Orcish War Banner", Source.Achievement, 234_002),
            (403, "Dornogal Lantern", Source.Quest, 234_003),
        ];
        entries.AddRange(decor.Select(piece => new Collectible
        {
            Kind = Kind.Decor,
            Id = piece.Id,
            Name = piece.Name,
            Source = piece.Source,
            Description = $"{piece.Source.Label()}: Housing",
            LinkId = piece.Item,
        }));
        return entries;
    }

    /// <summary>What the account already has. Two mounts, three pets, one toy, one piece of decor.</summary>
    public static HashSet<(Kind Kind, long Id)> Owned() =>
    [
        (Kind.Mount, 6), (Kind.Mount, 8),
        (Kind.Pet, 1), (Kind.Pet, 2), (Kind.Pet, 3),
        (Kind.Toy, 301),
        (Kind.Decor, 401),
    ];

    /// <summary>Copies held per species. More than one is a spare, which is what makes a pet worth selling.</summary>
    public static Dictionary<long, long> PetsHeld() => new() { [1] = 3, [2] = 2, [3] = 5 };

    /// <summary>
    /// Pulls this account has made, as the addon counts them. Joined to a
    /// collectible by the sentence its journal gives — see
    /// <see cref="Counters.AttemptsAt"/>.
    /// </summary>
    public static List<Tally.Tally> Attempts() =>
    [
        new() { Kind = Counting.Attempt, Key = "Kael'thas Sunstrider", Label = "Kael'thas Sunstrider", Count = 58 },
        new() { Kind = Counting.Attempt, Key = "The Lich King", Label = "The Lich King", Count = 31 },
        new() { Kind = Counting.Attempt, Key = "Yogg-Saron", Label = "Yogg-Saron", Count = 14 },
    ];

    /// <summary>
    /// Drop chances as an installed Rarity would supply them, keyed by the
    /// mount's summoning spell — which is the sample's <c>LinkId</c>.
    /// </summary>
    public static List<Chance> Chances() =>
    [
        new() { Name = "Ashes of Al'ar", SpellId = 900, OneIn = 20 },
        new() { Name = "Invincible's Reins", SpellId = 1100, OneIn = 100 },
        new() { Name = "Mimiron's Head", SpellId = 1000, OneIn = 3000 },
    ];

    /// <summary>
    /// The same odds as a Rarity database file, in the shape
    /// <see cref="Rarity.Parse"/> reads. The account holds no setter for its
    /// odds — they are read from the install the settings name, once — so the
    /// preview installs this under a throwaway game folder and points the
    /// settings at it, which is the seam the real thing uses.
    /// </summary>
    public static string RarityLua()
    {
        var lua = new StringBuilder("local addonName, addonTable = ...\nlocal M = {}\n\n");
        foreach (var chance in Chances())
        {
            lua.Append(CultureInfo.InvariantCulture, $"\t[\"{chance.Name}\"] = {{\n\t\tspellId = {chance.SpellId},\n\t\tchance = {chance.OneIn},\n\t}},\n");
        }
        return lua.ToString();
    }

    /// <summary>Write the Rarity fixture where <see cref="Rarity.Read"/> looks for it under a game folder.</summary>
    public static void InstallRarity(string wowPath)
    {
        var database = Path.Combine(Files.AddonDirectory(wowPath, "Rarity"), "DB");
        Directory.CreateDirectory(database);
        File.WriteAllText(Path.Combine(database, "Mounts.lua"), RarityLua());
    }

    // -- reputations and provenance -----------------------------------------------

    /// <summary>
    /// Standings for the enrolled three, which are the characters the account
    /// holds reputations for. The parser marks renown on a character below the
    /// level cap as inherited, so the fresh alt carrying somebody else's
    /// standings — the case the page exists to show — is Ulahae at 45.
    /// </summary>
    public static Dictionary<CharacterKey, List<FactionStanding>> Standings()
    {
        (long Id, string Name, string Tier, long Value, long Max, long Renown)[] factions =
        [
            (2570, "Dream Wardens", "Renown 20", 0, 0, 20),
            (2507, "Dragonscale Expedition", "Renown 25", 1400, 2500, 25),
            (69, "Darnassus", "Revered", 5000, 21000, 0),
            (2605, "The Assembly of the Deeps", "Renown 12", 800, 2500, 12),
            (1090, "Kirin Tor", "Exalted", 0, 0, 0),
        ];
        var standings = new Dictionary<CharacterKey, List<FactionStanding>>();
        foreach (var character in Cohort().MembersOf(Roster()))
        {
            standings[character.Key] = factions.Select(faction => new FactionStanding
            {
                Faction = faction.Id,
                Name = faction.Name,
                Tier = faction.Tier,
                Value = faction.Value,
                Max = faction.Max,
                Renown = faction.Renown,
                Inherited = faction.Renown > 0 && character.Level < 70,
            }).ToList();
        }
        return standings;
    }

    /// <summary>
    /// A reputations response in Blizzard's shape, so the standings reach the
    /// account the way real ones do: out of the response cache, through
    /// <see cref="Profile.ParseReputations"/>.
    /// </summary>
    public static byte[] ReputationsBody(IEnumerable<FactionStanding> standings)
    {
        var reputations = standings.Select(standing => new Dictionary<string, object>
        {
            ["faction"] = new Dictionary<string, object> { ["id"] = standing.Faction, ["name"] = standing.Name },
            ["standing"] = new Dictionary<string, object>
            {
                ["raw"] = standing.Renown > 0 ? standing.Renown * 2500 + standing.Value : standing.Value,
                ["value"] = standing.Value,
                ["max"] = standing.Max,
                ["name"] = standing.Tier,
                ["renown_level"] = standing.Renown,
            },
        }).ToList();
        return JsonSerializer.SerializeToUtf8Bytes(new Dictionary<string, object> { ["reputations"] = reputations });
    }

    /// <summary>
    /// What one character has personally earned, so the page can be looked at
    /// in the state it exists for: an inherited standing that somebody is
    /// grinding anyway. Ulahae, because that is who <see cref="Standings"/>
    /// gives inherited standings to; the two have to agree or the row the
    /// feature exists for never appears. Somechar carries the currency half,
    /// with one answer of each origin for the Warband summary.
    /// </summary>
    public static Dictionary<CharacterKey, Earned> Provenance() => new()
    {
        [Ulahae] = new Earned
        {
            Reputation =
            {
                // Halfway to Exalted by their own hand, with a faction the
                // account maxed out years ago.
                [69] = new EarnedReputation { Points = 12_000, AccountWide = true },
                // And a renown faction, counted in levels: nine of the
                // account's twenty-five earned here.
                [2507] = new EarnedReputation { Points = 4_200, Renown = 9, RenownSeen = 25, AccountWide = true },
            },
        },
        [Somechar] = new Earned
        {
            Currency =
            {
                [3008] = new EarnedCurrency { Gained = 1_900, Earned = 1_900, TracksEarned = true, AccountWide = true, Transferable = true },
                [2245] = new EarnedCurrency { Gained = 640, Earned = 120, TracksEarned = true, AccountWide = true, Transferable = true },
                [2815] = new EarnedCurrency { Gained = 45, Earned = 0, TracksEarned = false, AccountWide = true, Transferable = true },
                [1166] = new EarnedCurrency { Gained = 0, Earned = 0, AccountWide = false },
            },
        },
    };

    public static Dictionary<CharacterKey, Dictionary<long, long>> Currencies() => new()
    {
        [Somechar] = new() { [3008] = 1_900, [2245] = 640, [2815] = 45, [1166] = 2_140 },
        [Velkurai] = new() { [3008] = 210, [2245] = 88 },
    };

    public static Dictionary<long, long> WarbandBank() => new() { [210_797] = 600, [197_794] = 240, [208_766] = 40 };

    // -- the run --------------------------------------------------------------------

    private const long EarnedFrom = 0;
    private const long EarnedTo = 37;
    private const long ObservableFrom = 100;
    private const long ObservableTo = 118;
    private const long AttestableFrom = 200;
    private const long AttestableTo = 209;
    private const long UnrepeatableFrom = 300;
    private const long UnrepeatableTo = 307;
    private const long ByHandFrom = 307;
    private const long ByHandTo = 310;
    private const long UnearnedFrom = 400;
    private const long UnearnedTo = 406;
    private const long CohortFrom = 500;
    private const long CohortTo = 504;

    /// <summary>The whole population of goal ids, by bucket. Every builder that names a goal draws on this.</summary>
    private static IEnumerable<long> Range(long from, long to) => Enumerable.Range(0, (int)(to - from)).Select(offset => from + offset);

    /// <summary>
    /// Real achievement names, so the list reads the way it will in the app
    /// rather than as a column of ids. Feats of Strength for the goals the
    /// planner is to exclude.
    /// </summary>
    public static Dictionary<long, Achievement> Catalogue()
    {
        string[] quests =
        [
            "Loremaster of Kalimdor", "Explore Dustwallow Marsh", "The Deadmines", "Classic Dungeonmaster", "Ambassador of the Horde",
            "Loremaster of Eastern Kingdoms", "Explore Northrend", "Nagrand Slam", "Into the Wild Blue Yonder", "On the Blade's Edge",
            "Hellfire Ramparts", "Terror of Terokkar", "Zangarmarsh", "Shattrath Divided", "Might of Kalimdor",
            "The Loremaster", "Explore Outland", "Bloody Rare",
        ];
        string[] holidays =
        [
            "Brewmaster", "To Honor One's Elders", "Fool For Love", "Hallowed Be Thy Name", "The Winter Veil Gourmet",
            "Flame Warden", "Merrymaker", "Noble Gardener", "Spirit of Competition",
        ];
        string[] feats =
        [
            "Realm First! Level 80", "Tabard of the Protector", "Vampire Hunter", "Big Blizzard Bear", "Old School Ride",
            "Insane in the Membrane", "Competitor's Tabard",
        ];
        string[] unearned = ["Long Strange Trip", "Glory of the Hero", "Glory of the Delver", "The Keystone Master", "Sha of Anger", "Going Down?"];
        string[] cohort = ["Level 80", "Explore Isle of Dorn", "Exalted with the Council of Dornogal", "Dungeon Hero"];
        string[] earned =
        [
            "Level 10", "Level 20", "Level 30", "Level 40", "Level 50", "Level 60", "Level 70", "Sightseeing", "Well Read", "Fast and Furious",
            "Explore Hallowfall", "Explore Azj-Kahet", "Explore the Ringing Deeps", "Stormrider", "Home Turf", "Safe Passage",
            "Well-Travelled", "Skyriding Glyph Hunter", "Delve Diver", "Bountiful Delves", "Nerubian Cartel", "Rocket Ride",
            "Twice the Fun", "Got My Mind on My Money", "Undermine It", "Big Shot", "Cartel Chief", "Cauldron Hopper",
            "Making a Splash", "Rare Catch", "Glory of the Undermine Raider", "Ahead of the Curve", "Cutting Edge",
            "Adventurer of Hallowfall", "Adventurer of Azj-Kahet", "Adventurer of the Ringing Deeps", "Adventurer of Isle of Dorn",
        ];

        var catalogue = new Dictionary<long, Achievement>();
        void Fill(long from, IEnumerable<string> names, string category, bool unrepeatable = false)
        {
            var id = from;
            foreach (var name in names)
            {
                catalogue[id] = new Achievement { Id = id, Name = name, Category = category, Points = 10, IsUnrepeatable = unrepeatable };
                id++;
            }
        }
        Fill(EarnedFrom, earned, "Exploration");
        Fill(ObservableFrom, quests, "Quests");
        Fill(AttestableFrom, holidays, "World Events");
        Fill(UnrepeatableFrom, feats, "Feats of Strength", unrepeatable: true);
        Fill(ByHandFrom, ["Master of Anniversary Events", "Timewalking Crusader", "Emissary of War"], "Dungeons & Raids");
        Fill(UnearnedFrom, unearned, "Dungeons & Raids");
        Fill(CohortFrom, cohort, "Character");
        return catalogue;
    }

    /// <summary>A poisoned, measurable goal's ten criteria, all quests. Progress is spread across the range so the ranking has something to do.</summary>
    private static IEnumerable<(long Criterion, long Quest)> QuestCriteria(long achievement) =>
        Enumerable.Range(0, 10).Select(k => (achievement * 100 + k, 70_000 + achievement * 10 + k));

    /// <summary>How many of a goal's ten quests Somechar has turned in: <c>10 - id % 10</c>, as in the GTK sample.</summary>
    private static int QuestsDone(long achievement) => (int)(10 - achievement % 10);

    /// <summary>
    /// The run as the store holds it: goals in every bucket and standing.
    /// Settled ones earned since the baseline; poisoned ones that are
    /// measurable and part-way there; poisoned ones nothing measures, one of
    /// them marked done by hand; Feats of Strength outside the denominator;
    /// three a person excluded; unearned ones; and ones the cohort earned.
    /// The evaluations are not stored — <see cref="Dump"/> is what makes the
    /// planner put them back.
    /// </summary>
    public static Run.Run Run()
    {
        Standing Poisoned() => new Standing.Poisoned(Aeltor);
        var goals = new List<Goal>();
        goals.AddRange(Range(EarnedFrom, EarnedTo).Select(id => new Goal { AchievementId = id, Standing = new Standing.EarnedDuringRun(At("2026-07-14T00:00:00Z")) }));
        goals.AddRange(Range(ObservableFrom, ObservableTo).Select(id => new Goal
        {
            AchievementId = id,
            Standing = Poisoned(),
            Nearest = Somechar,
            Evaluation = new Evaluation(QuestsDone(id), 10, true, false),
        }));
        goals.AddRange(Range(AttestableFrom, AttestableTo).Select(id => new Goal
        {
            AchievementId = id,
            Standing = Poisoned(),
            Bucket = new Bucket.Attestable(),
            Attestation = id == AttestableFrom ? new Attestation { Character = Somechar, At = At("2026-07-20T00:00:00Z") } : null,
        }));
        goals.AddRange(Range(UnrepeatableFrom, UnrepeatableTo).Select(id => new Goal { AchievementId = id, Standing = Poisoned(), Bucket = new Bucket.Excluded(Exclusion.Unrepeatable) }));
        goals.AddRange(Range(ByHandFrom, ByHandTo).Select(id => new Goal { AchievementId = id, Standing = Poisoned(), Bucket = new Bucket.Excluded(Exclusion.ByHand) }));
        goals.AddRange(Range(UnearnedFrom, UnearnedTo).Select(id => new Goal { AchievementId = id }));
        goals.AddRange(Range(CohortFrom, CohortTo).Select(id => new Goal { AchievementId = id, Standing = new Standing.EarnedByCohort(Velkurai) }));
        return new Run.Run
        {
            Name = "Fresh start",
            Baseline = new Baseline { TakenAt = BaselineAt },
            Cohort = Cohort(),
            Goals = goals,
        };
    }

    // -- the addon's read -----------------------------------------------------------

    /// <summary>
    /// The account file, as the collector would have written it for this
    /// account: attribution, completion and criteria trees that plan to
    /// exactly the goals in <see cref="Run"/>, and everything else the file
    /// carries — currencies, the Warband bank, the collections, provenance,
    /// the recipe books, the lifetime counters, the pets held.
    /// </summary>
    public static Collected Collected()
    {
        var collected = new Collected { WrittenAt = At("2026-09-13T23:41:00Z"), Client = new Armory.Addon.Client { Project = 1, Version = "12.0.7", Build = 65_000, Interface = 120_007 } };
        var before = At("2024-03-02T20:15:00Z");
        foreach (var id in Range(EarnedFrom, EarnedTo))
        {
            collected.EarnedBy[id] = Somechar;
            collected.Completed[id] = At("2026-07-14T00:00:00Z");
        }
        foreach (var id in Range(ObservableFrom, ObservableTo).Concat(Range(UnrepeatableFrom, UnrepeatableTo)).Concat(Range(ByHandFrom, ByHandTo)))
        {
            collected.EarnedBy[id] = Aeltor;
            collected.Completed[id] = before;
            collected.Tree[id] = QuestCriteria(id).Select(pair => pair.Criterion).ToList();
            foreach (var (criterion, quest) in QuestCriteria(id))
            {
                collected.Criteria[criterion] = CriterionKind.Quest(quest);
            }
        }
        foreach (var id in Range(AttestableFrom, AttestableTo))
        {
            collected.EarnedBy[id] = Aeltor;
            collected.Completed[id] = before;
            // A criterion the catalogue does not describe: nothing measures it.
            collected.Tree[id] = [id * 100];
        }
        foreach (var id in Range(UnearnedFrom, UnearnedTo))
        {
            collected.Tree[id] = QuestCriteria(id).Select(pair => pair.Criterion).ToList();
            foreach (var (criterion, quest) in QuestCriteria(id))
            {
                collected.Criteria[criterion] = CriterionKind.Quest(quest);
            }
        }
        foreach (var id in Range(CohortFrom, CohortTo))
        {
            collected.EarnedBy[id] = Velkurai;
            collected.Completed[id] = At("2024-11-01T19:00:00Z");
        }
        foreach (var (id, achievement) in Catalogue())
        {
            collected.Catalogue[id] = achievement;
        }
        foreach (var (key, amounts) in Currencies())
        {
            collected.Currencies[key] = amounts;
        }
        foreach (var (item, count) in WarbandBank())
        {
            collected.WarbandBank[item] = count;
        }
        collected.Collectibles.AddRange(Collection());
        collected.Owned.UnionWith(Owned());
        foreach (var (key, earned) in Provenance())
        {
            collected.Earned[key] = earned;
        }
        foreach (var (key, book) in Recipes())
        {
            collected.Recipes[key] = book;
        }
        foreach (var (key, counted) in Tallies())
        {
            collected.Tallies[key] = counted;
        }
        foreach (var (species, count) in PetsHeld())
        {
            collected.PetsHeld[species] = count;
        }
        return collected;
    }

    /// <summary>Somechar's quest log: the first <c>10 - id % 10</c> of each measurable goal's ten quests.</summary>
    public static HashSet<long> QuestsOf(CharacterKey key)
    {
        if (key != Somechar)
        {
            return [];
        }
        var quests = new HashSet<long>();
        foreach (var id in Range(ObservableFrom, ObservableTo))
        {
            quests.UnionWith(QuestCriteria(id).Take(QuestsDone(id)).Select(pair => pair.Quest));
        }
        return quests;
    }

    /// <summary>
    /// The whole read, as <see cref="AddonWatch"/> would deliver it: the
    /// account file, a file per character, and every evening. Handing this
    /// to <see cref="Account.Collected"/> after <see cref="Seed"/> is what
    /// plans the run with its evaluations, which the goal rows do not keep.
    /// </summary>
    public static Dump Dump()
    {
        var details = Details();
        return new Dump
        {
            Collected = Collected(),
            Characters = Roster().Characters.Select(character => new CollectedCharacter
            {
                Character = character,
                Detail = details.GetValueOrDefault(character.Key) ?? new Detail(),
                Quests = QuestsOf(character.Key),
            }).ToList(),
            Sessions = Sessions(),
        };
    }
}
