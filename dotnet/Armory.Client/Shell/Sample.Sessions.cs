using Armory.Chronicle;
using Armory.Roster;
using Armory.Tally;

namespace Armory.Client.Shell;

/// <summary>
/// The evenings, and what a lifetime of them adds up to. Three sessions: a
/// full one, a quiet one, and a raid night. The quiet one is deliberate.
/// Most sessions are, and a page that only ever gets previewed with a
/// two-hour raid night hides the layout question that actually matters —
/// what a card looks like when there is almost nothing in it.
/// </summary>
public static partial class Sample
{
    private static Moment Moment(long seconds, Happening what) => new() { At = seconds, What = what };

    public static List<Session> Sessions() => [Busy(), Quiet(), RaidNight()];

    /// <summary>An evening in Nagrand: a quest with the game's own text, a level, a death and what caused it, a drop, a mount, a sale, and a gold ledger.</summary>
    private static Session Busy() => new()
    {
        Character = Somechar,
        DisplayName = "Somechar",
        RealmName = "Emerald Dream",
        Class = "Druid",
        Race = "Tauren",
        Faction = Faction.Horde,
        StartedAt = At("2026-08-03T19:04:00Z"),
        EndedAt = At("2026-08-03T21:38:00Z"),
        StartLevel = 70,
        EndLevel = 71,
        StartMoney = 118_204_500,
        EndMoney = 118_108_500,
        StartItemLevel = 602,
        EndItemLevel = 606,
        Moments =
        [
            Moment(0, new Happening.Arrived("Orgrimmar", "The Drag", null)),
            Moment(420, new Happening.Arrived("Nagrand", "Halaa", null)),
            Moment(460, new Happening.Accepted("Hero of the Mag'har", "Garrosh has not left his tent since his father's name was spoken. Someone must go to him.")),
            Moment(1_980, new Happening.Completed(9_923, "Hero of the Mag'har", "You have given him back his father. Whatever comes of it now, the Mag'har will sing of this day.")),
            Moment(1_980, new Happening.Paid(9_923, 84_500, 12_400)),
            Moment(2_100, new Happening.Levelled(71, "Nagrand")),
            Moment(3_600, new Happening.Fought("Durn the Hungerer", false)),
            Moment(3_900, new Happening.Died("Nagrand", "Halaa", "Durn the Hungerer")),
            Moment(4_500, new Happening.Felled("Durn the Hungerer")),
            Moment(4_560, new Happening.Looted(32_458, "Collar of Cho'gall", 4)),
            Moment(5_400, new Happening.Acquired(Acquisition.Mount, "Talbuk Doe")),
            Moment(6_000, new Happening.Sold("Auction successful: Mycobloom", 3_745_800)),
            Moment(7_200, new Happening.Alongside("Velkurai")),
            Moment(1_100, new Happening.Coin(Purpose.Quest, 1_640_000, true)),
            Moment(1_200, new Happening.Coin(Purpose.Loot, 812_000, true)),
            Moment(4_400, new Happening.Coin(Purpose.Bid, 2_400_000, false)),
            Moment(5_100, new Happening.Coin(Purpose.Repair, 148_000, false)),
            Moment(5_300, new Happening.Crafted(371_637, "Flask of Alchemical Chaos")),
            Moment(5_400, new Happening.Crafted(371_637, "Flask of Alchemical Chaos")),
            Moment(400, new Happening.Campaign("Hero of the Mag'har", "Garrosh Hellscream leads the last uncorrupted orcs of Nagrand, and does not yet know what his father did.")),
        ],
        Risen = [new Risen("The Consortium", 7)],
        Travelled = 41_288,
        LongestFight = 664,
    };

    /// <summary>Fifty minutes, one quest, nothing else.</summary>
    private static Session Quiet() => new()
    {
        Character = Aeltor,
        DisplayName = "Aeltor",
        RealmName = "Mannoroth",
        Class = "Warrior",
        Race = "Orc",
        Faction = Faction.Horde,
        StartedAt = At("2026-08-02T22:10:00Z"),
        EndedAt = At("2026-08-02T23:02:00Z"),
        StartLevel = 80,
        EndLevel = 80,
        StartMoney = 44_120_000,
        EndMoney = 41_980_000,
        StartItemLevel = 641,
        EndItemLevel = 641,
        Moments =
        [
            Moment(0, new Happening.Arrived("Dornogal", null, null)),
            Moment(1_500, new Happening.Arrived("The Ringing Deeps", "Taelloch", null)),
            Moment(2_400, new Happening.Completed(82_311, "A Weight Off My Chest", null)),
        ],
        Travelled = 3_120,
        LongestFight = 0,
    };

    /// <summary>
    /// A keystone and half a raid, recent enough for the run page's fortnight
    /// strip: what the world said unbidden and what a vendor said when asked,
    /// the weather turning, the world tier, a boss wiped on and then killed,
    /// a rare, a cutscene, a gear upgrade joined to what dropped it, and an
    /// achievement.
    /// </summary>
    private static Session RaidNight() => new()
    {
        Character = Velkurai,
        DisplayName = "Velkurai",
        RealmName = "Emerald Dream",
        Class = "Mage",
        Race = "Troll",
        Faction = Faction.Horde,
        StartedAt = At("2026-09-12T19:31:00Z"),
        EndedAt = At("2026-09-12T22:58:00Z"),
        StartLevel = 71,
        EndLevel = 71,
        StartMoney = 4_821_000_000,
        EndMoney = 4_836_400_000,
        StartItemLevel = 571,
        EndItemLevel = 578,
        Moments =
        [
            Moment(0, new Happening.Arrived("Dornogal", "The Coreway", 2339)),
            Moment(30, new Happening.WorldTier("Heroic")),
            Moment(90, new Happening.Told("Auditor Balwurz", "The Coreway is open. Mind the drop; the elevator is not a suggestion.")),
            Moment(180, new Happening.Gave("Auditor Balwurz", 84_210, 226_701)),
            Moment(240, new Happening.Accepted("The Deeps Below", "Something has been knocking at the bottom of the Ringing Deeps, and the Machine Speakers have stopped answering.")),
            Moment(400, new Happening.Flew("Dornogal")),
            Moment(620, new Happening.Arrived("The Ringing Deeps", "Gundargaz", 2214)),
            Moment(700, new Happening.Weather("Rain", "The Ringing Deeps")),
            Moment(760, new Happening.Said("Speaker Brinthe", "Turn back, traveller. The earth here remembers what it was made for.")),
            Moment(900, new Happening.Entered("Darkflame Cleft", "party", 5)),
            Moment(960, new Happening.Alongside("Bramblefoot")),
            Moment(960, new Happening.Alongside("Sarrun")),
            Moment(960, new Happening.Alongside("Tessuya")),
            Moment(2_400, new Happening.Keystone("Darkflame Cleft", 10, true, 2, 1_412)),
            Moment(2_460, new Happening.Looted(221_130, "Wick's Lead Curtain", 4)),
            Moment(2_470, new Happening.Equipped("Wick's Lead Curtain", 597, 26)),
            Moment(2_500, new Happening.Coin(Purpose.Loot, 1_120_000, true)),
            Moment(3_000, new Happening.Entered("Liberation of Undermine", "raid", 20)),
            Moment(3_300, new Happening.Fought("Vexie and the Geargrinders", false)),
            Moment(3_300, new Happening.Wiped("Vexie and the Geargrinders", 31)),
            Moment(3_310, new Happening.Died("Liberation of Undermine", "The Hub", "Vexie and the Geargrinders")),
            Moment(3_900, new Happening.Fought("Vexie and the Geargrinders", false)),
            Moment(3_900, new Happening.Wiped("Vexie and the Geargrinders", 4)),
            Moment(4_500, new Happening.Fought("Vexie and the Geargrinders", true)),
            Moment(4_500, new Happening.Felled("Vexie and the Geargrinders")),
            Moment(4_520, new Happening.Said("Vexie", "Fine! Keep the bike! It never ran right anyway!")),
            Moment(4_560, new Happening.Earned(41_223, "Undermine It")),
            Moment(4_800, new Happening.Cutscene("Liberation of Undermine", 1_120)),
            Moment(6_000, new Happening.Fought("Cauldron of Carnage", false)),
            Moment(6_000, new Happening.Wiped("Cauldron of Carnage", 62)),
            Moment(7_200, new Happening.Arrived("Undermine", "The Incontinental Hotel", 2346)),
            Moment(7_300, new Happening.Rare("Scrapbeak", "rare-elite")),
            Moment(7_320, new Happening.Felled("Scrapbeak")),
            Moment(7_400, new Happening.Practised("Tailoring", 87)),
            Moment(7_460, new Happening.Learned("Pattern: Weavercloth Bandage")),
            Moment(7_500, new Happening.Appearance("Cartel-Issue Cloak")),
            Moment(7_600, new Happening.Expired("Auction expired: Weavercloth")),
            Moment(7_700, new Happening.Coin(Purpose.Sale, 14_280_000, true)),
            Moment(7_800, new Happening.Coin(Purpose.Deposit, 60_000, false)),
            Moment(8_000, new Happening.Pictured("boss", "Vexie and the Geargrinders")),
            Moment(9_000, new Happening.Scenario("Fungal Folly", "Tier 8")),
            Moment(12_000, new Happening.Coin(Purpose.Repair, 340_000, false)),
        ],
        Risen = [new Risen("The Cartels of Undermine", 12)],
        Travelled = 28_910,
        LongestFight = 412,
    };

    /// <summary>One of the evenings written up, the others not.</summary>
    public static Dictionary<SessionId, Entry> Entries(IReadOnlyList<Session> sessions)
    {
        if (sessions.Count == 0)
        {
            return [];
        }
        var first = sessions[0];
        return new Dictionary<SessionId, Entry>
        {
            [first.Id] = new Entry
            {
                Session = first.Id,
                Title = "What the Mag'har Sing",
                Body = "I went to Halaa meaning only to clear the road, and came back with a name I will not put down again for a while.\n\n"
                    + "Garrosh would not look at me at first. He has his father's shoulders and none of his father's certainty, and when I told him what "
                    + "Hellscream had done at the end — not the drinking, the other thing, the thing the orcs of this world still owe him for — he stood "
                    + "there so long I thought I had broken him. The Mag'har will sing of it, someone said afterwards. Perhaps. They were singing before "
                    + "I reached the tent flap.\n\n"
                    + "Durn caught us in the open on the way back and I did not get up from it. Velkurai dragged me to the spirit healer and said nothing "
                    + "about it, which I appreciated more than I said. We went again an hour later and the gronn went down like a felled tree. The talbuk "
                    + "that has been following me since dusk seems to have decided the matter is settled.",
                Model = "qwen3-30b-a3b-instruct",
                WrittenAt = At("2026-08-04T08:12:00Z"),
            },
        };
    }

    /// <summary>
    /// A lifetime of counters for the character whose evenings these are: the
    /// GTK preview's <c>sample_tallies</c> and <c>sample_life</c> folded
    /// together, since the character page here reads the same table the
    /// chronicle does. The account's pull counts (<see cref="Attempts"/>) are
    /// filed under the same character.
    /// </summary>
    public static Tallies Tallies()
    {
        (Counting Kind, string Key, string Label, long Count)[] counted =
        [
            (Counting.Recipe, "Flask of Alchemical Chaos", "Flask of Alchemical Chaos", 412),
            (Counting.Recipe, "Algari Mana Potion", "Algari Mana Potion", 188),
            (Counting.Recipe, "Sanctified Alchemist Stone", "Sanctified Alchemist Stone", 9),
            (Counting.Companion, "Velkurai", "Velkurai", 34),
            (Counting.Companion, "Bramblefoot", "Bramblefoot", 41),
            (Counting.Companion, "Sarrun", "Sarrun", 12),
            (Counting.Companion, "Tessuya", "Tessuya", 11),
            (Counting.Victory, "Durn the Hungerer", "Durn the Hungerer", 6),
            (Counting.Victory, "Queen Ansurek", "Queen Ansurek", 22),
            (Counting.Victory, "Mug'Zee", "Mug'Zee", 9),
            // Keyed by UiMapID, which is what the addon records and what the
            // zone corpus joins on — 107 is Outland's Nagrand, not Draenor's.
            (Counting.Zone, "107", "Nagrand", 68_400),
            (Counting.Zone, "85", "Orgrimmar", 9_120),
            (Counting.Zone, "1970", "Zaralek Cavern", 61 * 3600),
            (Counting.Zone, "2022", "The Waking Shores", 38 * 3600),
            (Counting.Zone, "1527", "Uldum", 24 * 3600),
            (Counting.Zone, "84", "Stormwind City", 12 * 3600),
            (Counting.Killer, "Durn the Hungerer", "Durn the Hungerer", 4),
            (Counting.Killer, "Vexie", "Vexie", 61),
            (Counting.Distance, "walked", "Walked", 5_918_400),
            (Counting.Flight, "Nagrand", "Nagrand", 61),
            (Counting.Flight, "Valdrakken", "Valdrakken", 214),
            (Counting.Delve, "Tier 11", "Tier 11", 40),
            (Counting.Delve, "Tier 8", "Tier 8", 82),
            (Counting.Questgiver, "Khadgar", "Khadgar", 96),
            (Counting.Rare, "Scrapbeak", "Scrapbeak", 3),
        ];
        var tallies = new Tallies
        {
            [Somechar] = counted.Select(tally => new Tally.Tally { Kind = tally.Kind, Key = tally.Key, Label = tally.Label, Count = tally.Count })
                .Concat(Attempts())
                .ToList(),
            [Velkurai] =
            [
                new Tally.Tally { Kind = Counting.Zone, Key = "2214", Label = "The Ringing Deeps", Count = 14 * 3600 },
            ],
        };
        return tallies;
    }
}
