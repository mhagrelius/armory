using System.Globalization;
using System.Text;
using Armory.Roster;

namespace Armory.Chronicle;

/// <summary>Somewhere the character was, and for how long.</summary>
public sealed record Stop
{
    public string Zone { get; init; } = "";

    /// <summary>The subzones passed through while there, in order and deduplicated.</summary>
    public List<string> Within { get; init; } = [];

    /// <summary>Seconds spent before moving on.</summary>
    public long Stayed { get; set; }
}

/// <summary>Where a death happened, and what did it. Null for a fall, a drowning or a death nothing was blamed for.</summary>
public sealed record Death(string Zone, string? Subzone, string? To);

/// <summary>A keystone run.</summary>
public sealed record Keystone(string Dungeon, int Level, bool InTime, int Upgrades, long Seconds);

/// <summary>
/// A piece of gear that was actually an upgrade, and where it came from. The
/// source is a join over the evening's own record, bounded to a pull, and
/// absent whenever that cannot be said.
/// </summary>
public sealed record Upgrade(string Name, int ItemLevel, int Gained, string? From);

/// <summary>A quest, as the evening will remember it.</summary>
public sealed record Quest
{
    public long Id { get; init; }

    public string Title { get; init; } = "";

    /// <summary>The premise, from accepting it.</summary>
    public string? Premise { get; init; }

    /// <summary>What the turn-in said. The story, in the game's own words.</summary>
    public string? Story { get; init; }

    public long Money { get; set; }
}

/// <summary>A screenshot the addon asked for, matched to the file the client wrote.</summary>
public sealed record Picture(string Subject, DateTimeOffset TakenAt, string Path);

public enum Reading
{
    Quest,
    Zone,
    Achievement,
    Watch,
}

public static class ReadingExtensions
{
    public static string Label(this Reading reading) => reading switch
    {
        Reading.Quest => "Quest",
        Reading.Zone => "Zone",
        Reading.Achievement => "Achievement",
        _ => "Watch",
    };
}

/// <summary>Somewhere worth sending a person, and what sort of place it is.</summary>
public sealed record Link(string Label, Reading Sort, string Url);

/// <summary>A written-up evening.</summary>
public sealed record Entry
{
    public required SessionId Session { get; init; }

    public string Title { get; init; } = "";

    /// <summary>Markdown, as the model wrote it.</summary>
    public string Body { get; init; } = "";

    /// <summary>Which model wrote it: a journal that spans years will span several.</summary>
    public string Model { get; init; } = "";

    public DateTimeOffset WrittenAt { get; init; }
}

/// <summary>
/// One evening, rolled up. This is the whole feature standing on its own:
/// everything here is drawn on a card whether or not anybody ever writes
/// prose about it. Every list is deduplicated and ordered, because a journal
/// entry is a narrative and the inputs arrive as a sequence.
/// </summary>
public sealed record Digest
{
    /// <summary>How long an evening has to be, with nothing else to show for it, before it counts as an evening at all.</summary>
    private const long Idle = 15 * 60;

    /// <summary>How long after the addon asked for a screenshot the file may appear, in seconds. Generous in one direction only.</summary>
    private const long Shutter = 15;

    public required CharacterKey Character { get; init; }
    public string DisplayName { get; init; } = "";
    public string RealmName { get; init; } = "";
    public string Class { get; init; } = "";
    public string Race { get; init; } = "";
    public Faction Faction { get; init; }
    public DateTimeOffset StartedAt { get; init; }
    public DateTimeOffset EndedAt { get; init; }

    public List<Stop> Route { get; init; } = [];

    /// <summary>The storylines the evening's quests belonged to, in the order they were first touched.</summary>
    public List<(string Name, string? Summary)> Campaigns { get; init; } = [];
    public List<Quest> Quests { get; init; } = [];

    /// <summary>Quests accepted and not turned in.</summary>
    public List<string> TakenUp { get; init; } = [];
    public List<(int Level, string Zone)> Levels { get; init; } = [];
    public List<Death> Deaths { get; init; } = [];
    public List<string> Felled { get; init; } = [];

    /// <summary>Bosses wiped on and never killed, with the best pull where the client said.</summary>
    public List<string> LostTo { get; init; } = [];
    public List<string> Rares { get; init; } = [];
    public List<(string Name, string Kind)> Instances { get; init; } = [];
    public List<string> WorldTiers { get; init; } = [];
    public List<string> Weather { get; init; } = [];
    public List<Keystone> Keystones { get; init; } = [];
    public List<string> Scenarios { get; init; } = [];
    public List<(long Id, string Name)> Achievements { get; init; } = [];
    public List<(Acquisition Kind, string Name)> Acquired { get; init; } = [];
    public List<(long Item, string Name, int Quality)> Loot { get; init; } = [];
    public List<(string Subject, long Money)> Sales { get; init; } = [];
    public List<string> Companions { get; init; } = [];
    public List<(string Profession, int Skill)> Practised { get; init; } = [];

    /// <summary>Gear upgrades, best first.</summary>
    public List<Upgrade> Equipped { get; init; } = [];
    public List<string> Appearances { get; init; } = [];
    public List<string> Learned { get; init; } = [];
    public List<(string Who, string Line)> Overheard { get; init; } = [];
    public List<string> Expired { get; init; } = [];
    public List<(string Zone, long? Movie)> Cutscenes { get; init; } = [];
    public List<(string Who, string Line)> Told { get; init; } = [];
    public List<(string Who, long Given)> Questgivers { get; init; } = [];

    /// <summary>Where money came from, largest first. The ledger is the only set of books.</summary>
    public List<(Purpose Purpose, long Amount)> Income { get; init; } = [];
    public List<(Purpose Purpose, long Amount)> Spending { get; init; } = [];
    public List<(string Name, long Made)> Crafted { get; init; } = [];
    public long Flights { get; init; }
    public List<(long At, string Subject)> Shots { get; init; } = [];
    public List<Risen> Risen { get; init; } = [];
    public long Travelled { get; init; }
    public long LongestFight { get; init; }
    public int StartLevel { get; init; }
    public int EndLevel { get; init; }
    public int StartItemLevel { get; init; }
    public int EndItemLevel { get; init; }

    /// <summary>End minus start, in copper. Negative is a shopping trip.</summary>
    public long Purse { get; init; }
    public long QuestIncome { get; init; }
    public long SaleIncome { get; init; }

    public SessionId Id => new(Character, StartedAt);

    public TimeSpan Duration => EndedAt - StartedAt;

    /// <summary>
    /// Whether this evening is worth putting in front of somebody. Logging in
    /// to post an auction is not a chapter, but an hour wandering three zones
    /// is still an evening.
    /// </summary>
    public bool IsWorthWriting()
    {
        if (Quests.Count > 0 || Levels.Count > 0 || Felled.Count > 0 || LostTo.Count > 0 || Rares.Count > 0
            || Keystones.Count > 0 || Scenarios.Count > 0 || Achievements.Count > 0 || Acquired.Count > 0
            || Loot.Count > 0 || Sales.Count > 0 || Appearances.Count > 0 || Learned.Count > 0
            || Crafted.Count > 0 || Risen.Count > 0)
        {
            return true;
        }
        return (long)Duration.TotalSeconds >= Idle && Route.Count > 1;
    }

    /// <summary>The one line a card leads with before there is any prose.</summary>
    public string Headline()
    {
        var parts = new List<string>();
        // A keystone leads, because "+18 Halls of Atonement" is what the
        // evening was and the zone list is where it happened.
        if (Keystones.Count > 0)
        {
            parts.Add(string.Create(CultureInfo.InvariantCulture, $"+{Keystones[0].Level} {Keystones[0].Dungeon}"));
        }
        else if (Route.Count == 1)
        {
            parts.Add(Route[0].Zone);
        }
        else if (Route.Count > 1)
        {
            parts.Add(string.Create(CultureInfo.InvariantCulture, $"{Route[0].Zone} and {Route.Count - 1} more"));
        }
        // Two campaigns in an evening is not a headline, it is a list.
        if (Campaigns.Count == 1)
        {
            parts.Add(Campaigns[0].Name);
        }
        if (Quests.Count > 0)
        {
            parts.Add(Prose.Plural(Quests.Count, "quest", "quests"));
        }
        if (Levels.Count > 0)
        {
            parts.Add(string.Create(CultureInfo.InvariantCulture, $"level {Levels[^1].Level}"));
        }
        if (Felled.Count > 0)
        {
            parts.Add(Prose.Plural(Felled.Count, "boss", "bosses"));
        }
        if (Acquired.Count > 0)
        {
            parts.Add(Prose.Plural(Acquired.Count, "new thing", "new things"));
        }
        if (parts.Count == 0)
        {
            parts.Add("a quiet hour");
        }
        return string.Join(" · ", parts);
    }

    /// <summary>
    /// Match the addon's screenshot moments to the files the client wrote.
    /// The addon cannot know the filename, so the join is by time: a file
    /// within the shutter window after a recorded moment is that moment's
    /// picture, and a file earlier than the moment is somebody pressing
    /// Print Screen themselves.
    /// </summary>
    public List<Picture> Pictures(IEnumerable<(DateTimeOffset When, string Path)> taken)
    {
        var files = taken.ToList();
        var pictures = new List<Picture>();
        foreach (var (at, subject) in Shots)
        {
            var asked = StartedAt + TimeSpan.FromSeconds(at);
            var file = files
                .Where(file => file.When >= asked && file.When - asked <= TimeSpan.FromSeconds(Shutter))
                .OrderBy(file => file.When)
                .Select(file => ((DateTimeOffset, string)?)file)
                .FirstOrDefault();
            if (file is { } found)
            {
                pictures.Add(new Picture(subject, found.Item1, found.Item2));
            }
        }
        return pictures;
    }

    /// <summary>
    /// Where to read more, for the things this evening touched. Links, never
    /// fetches: Wowhead's terms forbid automated access and the wiki
    /// disallows its API for every user agent. The video channels are search
    /// URLs on purpose, because a link to a playlist rots.
    /// </summary>
    public List<Link> FurtherReading()
    {
        var links = new List<Link>();
        foreach (var quest in Quests.Take(8))
        {
            links.Add(new Link(quest.Title, Reading.Quest, string.Create(CultureInfo.InvariantCulture, $"https://www.wowhead.com/quest={quest.Id}")));
        }
        foreach (var stop in Route.Take(4))
        {
            links.Add(new Link(stop.Zone, Reading.Zone, $"https://warcraft.wiki.gg/wiki/{WikiTitle(stop.Zone)}"));
            links.Add(new Link($"{stop.Zone} lore — Nobbel87", Reading.Watch, $"https://www.youtube.com/@Nobbel87/search?query={Encode(stop.Zone)}"));
            links.Add(new Link($"{stop.Zone} — The Karazhan Library", Reading.Watch, $"https://www.youtube.com/@TheKarazhanLibrary/search?query={Encode(stop.Zone)}"));
        }
        foreach (var (id, name) in Achievements.Take(4))
        {
            links.Add(new Link(name, Reading.Achievement, string.Create(CultureInfo.InvariantCulture, $"https://www.wowhead.com/achievement={id}")));
        }
        return links;
    }

    /// <summary>A zone name as a wiki article title: spaces become underscores.</summary>
    private static string WikiTitle(string zone) => Encode(zone.Replace(' ', '_'));

    /// <summary>Percent-encode everything that is not safe in a URL path or query.</summary>
    internal static string Encode(string text)
    {
        var builder = new StringBuilder(text.Length);
        foreach (var b in Encoding.UTF8.GetBytes(text))
        {
            if (b is (>= (byte)'A' and <= (byte)'Z') or (>= (byte)'a' and <= (byte)'z') or (>= (byte)'0' and <= (byte)'9') or (byte)'-' or (byte)'_' or (byte)'.' or (byte)'~')
            {
                builder.Append((char)b);
            }
            else
            {
                builder.Append(CultureInfo.InvariantCulture, $"%{b:X2}");
            }
        }
        return builder.ToString();
    }
}

/// <summary>Rolling a session up into a digest.</summary>
public static class Digesting
{
    /// <summary>How long before a drop something may have died and still be credited with it, in seconds. Wide enough to survive a full bag, narrow enough that the previous pull is never the answer.</summary>
    private const long Spoils = 30;

    /// <summary>What dropped a piece of gear, if this evening can say: the item was looted tonight, and something with a name died within the window before it.</summary>
    private static string? WhereItCameFrom(Session session, string item)
    {
        long? looted = null;
        foreach (var moment in session.Moments)
        {
            if (moment.What is Happening.Looted loot && loot.Name == item)
            {
                looted = moment.At;
                break;
            }
        }
        if (looted is not { } at)
        {
            return null;
        }
        string? from = null;
        foreach (var moment in session.Moments)
        {
            if (moment.At > at || at - moment.At > Spoils)
            {
                continue;
            }
            switch (moment.What)
            {
                case Happening.Felled felled:
                    from = felled.Name;
                    break;
                case Happening.Rare rare:
                    from = rare.Name;
                    break;
                default:
                    break;
            }
        }
        return from;
    }

    /// <summary>Roll the evening up.</summary>
    public static Digest Digest(this Session session)
    {
        var route = new List<Stop>();
        var campaigns = new List<(string, string?)>();
        var quests = new List<Quest>();
        var premises = new List<(string Title, string? Premise)>();
        var levels = new List<(int, string)>();
        var deaths = new List<Death>();
        var felled = new List<string>();
        var lostTo = new List<string>();
        var bestPull = new List<(string Name, int Best)>();
        var rares = new List<string>();
        var instances = new List<(string, string)>();
        var worldTiers = new List<string>();
        var weather = new List<string>();
        var keystones = new List<Keystone>();
        var scenarios = new List<string>();
        var achievements = new List<(long, string)>();
        var acquired = new List<(Acquisition, string)>();
        var loot = new List<(long, string, int)>();
        var sales = new List<(string, long)>();
        var practised = new List<(string Profession, int Skill)>();
        var equipped = new List<Upgrade>();
        var appearances = new List<string>();
        var learned = new List<string>();
        var overheard = new List<(string, string)>();
        var expired = new List<string>();
        var cutscenes = new List<(string, long?)>();
        var told = new List<(string, string)>();
        var givers = new SortedDictionary<string, long>(StringComparer.Ordinal);
        var shots = new List<(long, string)>();
        var income = new SortedDictionary<Purpose, long>();
        var spending = new SortedDictionary<Purpose, long>();
        var crafted = new SortedDictionary<string, long>(StringComparer.Ordinal);
        long flights = 0;
        var companions = new SortedSet<string>(StringComparer.Ordinal);
        long entered = 0;

        foreach (var moment in session.Moments)
        {
            switch (moment.What)
            {
                case Happening.Arrived arrived:
                    if (route.Count > 0)
                    {
                        var last = route[^1];
                        // Still in the same zone: a subzone crossing, which is
                        // detail on this stop rather than a new one.
                        if (last.Zone == arrived.Zone)
                        {
                            if (arrived.Subzone is { } subzone && !last.Within.Contains(subzone))
                            {
                                last.Within.Add(subzone);
                            }
                            continue;
                        }
                        // Close the stop being left, so the route carries
                        // dwell times and not only an order.
                        last.Stayed = Math.Max(moment.At - entered, 0);
                    }
                    entered = moment.At;
                    route.Add(new Stop { Zone = arrived.Zone, Within = arrived.Subzone is { } within ? [within] : [] });
                    break;
                case Happening.Accepted accepted:
                    premises.Add((accepted.Title, accepted.Premise));
                    break;
                case Happening.Completed completed:
                    // The premise came from accepting it, matched by title
                    // because the accept event has no id.
                    quests.Add(new Quest
                    {
                        Id = completed.Quest,
                        Title = completed.Title,
                        Premise = premises.FirstOrDefault(taken => taken.Title == completed.Title).Premise,
                        Story = completed.Story,
                        Money = 0,
                    });
                    break;
                case Happening.Paid paid:
                    // Itemisation only. The money is the ledger's.
                    for (var index = quests.Count - 1; index >= 0; index--)
                    {
                        if (quests[index].Id == paid.Quest)
                        {
                            quests[index].Money = paid.Money;
                            break;
                        }
                    }
                    break;
                case Happening.Campaign campaign:
                    if (!campaigns.Any(seen => seen.Item1 == campaign.Name))
                    {
                        campaigns.Add((campaign.Name, campaign.Summary));
                    }
                    break;
                case Happening.Levelled levelled:
                    levels.Add((levelled.Level, levelled.Zone));
                    break;
                case Happening.Died died:
                    deaths.Add(new Death(died.Zone, died.Subzone, died.To));
                    break;
                case Happening.Entered instance:
                    if (!instances.Contains((instance.Name, instance.Kind)))
                    {
                        instances.Add((instance.Name, instance.Kind));
                    }
                    break;
                case Happening.WorldTier tier:
                    if (!worldTiers.Contains(tier.Tier))
                    {
                        worldTiers.Add(tier.Tier);
                    }
                    break;
                case Happening.Weather turned:
                    {
                        var said = turned.Zone is { } over ? $"{turned.Kind} over {over}" : turned.Kind;
                        if (!weather.Contains(said))
                        {
                            weather.Add(said);
                        }
                        break;
                    }
                case Happening.Keystone key:
                    keystones.Add(new Keystone(key.Dungeon, key.Level, key.InTime, key.Upgrades, key.Seconds));
                    break;
                case Happening.Scenario scenario:
                    {
                        var said = scenario.Tier is { } tier ? $"{scenario.Name} ({tier})" : scenario.Name;
                        if (!scenarios.Contains(said))
                        {
                            scenarios.Add(said);
                        }
                        break;
                    }
                case Happening.Rare rare:
                    if (!rares.Contains(rare.Name))
                    {
                        rares.Add(rare.Name);
                    }
                    break;
                case Happening.Practised skill:
                    {
                        // The best reached, not every step.
                        var index = practised.FindIndex(seen => seen.Profession == skill.Profession);
                        if (index >= 0)
                        {
                            practised[index] = (skill.Profession, Math.Max(practised[index].Skill, skill.Skill));
                        }
                        else
                        {
                            practised.Add((skill.Profession, skill.Skill));
                        }
                        break;
                    }
                case Happening.Equipped gear:
                    equipped.Add(new Upgrade(gear.Name, gear.ItemLevel, gear.Gained, null));
                    break;
                case Happening.Said said:
                    overheard.Add((said.Who, said.Line));
                    break;
                case Happening.Gave gave:
                    givers[gave.Who] = givers.GetValueOrDefault(gave.Who) + 1;
                    break;
                case Happening.Told spoken:
                    told.Add((spoken.Who, spoken.Line));
                    break;
                case Happening.Cutscene cutscene:
                    cutscenes.Add((cutscene.Zone, cutscene.Movie));
                    break;
                case Happening.Expired unsold:
                    if (!expired.Contains(unsold.What))
                    {
                        expired.Add(unsold.What);
                    }
                    break;
                case Happening.Learned recipe:
                    if (!learned.Contains(recipe.Name))
                    {
                        learned.Add(recipe.Name);
                    }
                    break;
                case Happening.Appearance appearance:
                    if (!appearances.Contains(appearance.Name))
                    {
                        appearances.Add(appearance.Name);
                    }
                    break;
                case Happening.Coin coin:
                    {
                        var book = coin.Incoming ? income : spending;
                        book[coin.Purpose] = book.GetValueOrDefault(coin.Purpose) + coin.Amount;
                        break;
                    }
                case Happening.Crafted made:
                    crafted[made.Name] = crafted.GetValueOrDefault(made.Name) + 1;
                    break;
                case Happening.Flew:
                    flights++;
                    break;
                case Happening.Pictured shot:
                    shots.Add((moment.At, shot.Subject.Length == 0 ? shot.What : $"{shot.What}: {shot.Subject}"));
                    break;
                case Happening.Felled boss:
                    if (!felled.Contains(boss.Name))
                    {
                        felled.Add(boss.Name);
                    }
                    break;
                case Happening.Fought fought:
                    if (fought.Won)
                    {
                        if (!felled.Contains(fought.Name))
                        {
                            felled.Add(fought.Name);
                        }
                    }
                    else if (!lostTo.Contains(fought.Name))
                    {
                        lostTo.Add(fought.Name);
                    }
                    break;
                case Happening.Wiped wiped:
                    {
                        var index = bestPull.FindIndex(seen => seen.Name == wiped.Name);
                        if (index >= 0)
                        {
                            bestPull[index] = (wiped.Name, Math.Min(bestPull[index].Best, wiped.Remaining));
                        }
                        else
                        {
                            bestPull.Add((wiped.Name, wiped.Remaining));
                        }
                        break;
                    }
                case Happening.Earned earned:
                    achievements.Add((earned.Achievement, earned.Name));
                    break;
                case Happening.Acquired got:
                    acquired.Add((got.Kind, got.Name));
                    break;
                case Happening.Looted looted:
                    loot.Add((looted.Item, looted.Name, looted.Quality));
                    break;
                case Happening.Sold sold:
                    sales.Add((sold.Subject, sold.Money));
                    break;
                case Happening.Alongside with:
                    companions.Add(with.Name);
                    break;
                default:
                    break;
            }
        }

        // The last stop ran to the end of the session.
        if (route.Count > 0)
        {
            route[^1].Stayed = Math.Max((long)Math.Max(session.Duration.TotalSeconds, 0) - entered, 0);
        }

        // A boss that was both won and lost against was, on balance, killed.
        lostTo.RemoveAll(felled.Contains);
        // The best pull is attached after that, so the name the removal
        // matched on is the one the addon wrote and not one with a number in it.
        for (var index = 0; index < lostTo.Count; index++)
        {
            var name = lostTo[index];
            var pull = bestPull.FindIndex(seen => seen.Name == name);
            if (pull >= 0)
            {
                lostTo[index] = string.Create(CultureInfo.InvariantCulture, $"{name} (down to {bestPull[pull].Best}%)");
            }
        }
        // A rare that also came through as a boss kill is one thing, not two.
        rares.RemoveAll(felled.Contains);
        // The upgrade that mattered is the biggest jump, so it leads.
        var upgrades = equipped
            .Select(upgrade => upgrade with { From = WhereItCameFrom(session, upgrade.Name) })
            .OrderByDescending(upgrade => upgrade.Gained)
            .ToList();

        // Biggest first in both books, because the question a person asks of
        // a ledger is "where did it mostly go".
        var incomeBook = income.Select(pair => (pair.Key, pair.Value)).OrderByDescending(pair => pair.Value).ToList();
        var spendingBook = spending.Select(pair => (pair.Key, pair.Value)).OrderByDescending(pair => pair.Value).ToList();
        var craftedList = crafted.Select(pair => (pair.Key, pair.Value)).OrderByDescending(pair => pair.Value).ToList();
        var questgivers = givers.Select(pair => (pair.Key, pair.Value)).OrderByDescending(pair => pair.Value).ToList();

        // Both read off the one set of books rather than summed a second time
        // from the events that itemise them.
        static long Of(List<(Purpose Purpose, long Amount)> book, Purpose want) =>
            book.FirstOrDefault(entry => entry.Purpose == want).Amount;
        var questIncome = Of(incomeBook, Purpose.Quest);
        var saleIncome = Of(incomeBook, Purpose.Sale);

        var turnedIn = quests.Select(quest => quest.Title).ToHashSet(StringComparer.Ordinal);
        var takenUp = premises.Select(premise => premise.Title).Where(title => !turnedIn.Contains(title)).ToSortedSet().ToList();

        return new Digest
        {
            Character = session.Character,
            DisplayName = session.DisplayName,
            RealmName = session.RealmName,
            Class = session.Class,
            Race = session.Race,
            Faction = session.Faction,
            StartedAt = session.StartedAt,
            EndedAt = session.EndedAt,
            Route = route,
            Campaigns = campaigns,
            Quests = quests,
            TakenUp = takenUp,
            Levels = levels,
            Deaths = deaths,
            Felled = felled,
            LostTo = lostTo,
            Rares = rares,
            Instances = instances,
            WorldTiers = worldTiers,
            Weather = weather,
            Keystones = keystones,
            Scenarios = scenarios,
            Achievements = achievements,
            Acquired = acquired,
            Loot = loot,
            Sales = sales,
            Companions = companions.ToList(),
            Practised = practised,
            Equipped = upgrades,
            Appearances = appearances,
            Learned = learned,
            Overheard = overheard,
            Expired = expired,
            Cutscenes = cutscenes,
            Told = told,
            Questgivers = questgivers,
            Income = incomeBook,
            Spending = spendingBook,
            Crafted = craftedList,
            Flights = flights,
            Shots = shots,
            Risen = session.Risen.ToList(),
            Travelled = session.Travelled,
            LongestFight = session.LongestFight,
            StartLevel = session.StartLevel,
            EndLevel = session.EndLevel,
            StartItemLevel = session.StartItemLevel,
            EndItemLevel = session.EndItemLevel,
            Purse = session.EndMoney - session.StartMoney,
            QuestIncome = questIncome,
            SaleIncome = saleIncome,
        };
    }

    private static SortedSet<string> ToSortedSet(this IEnumerable<string> items) => new(items, StringComparer.Ordinal);
}
