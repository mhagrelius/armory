using System.Globalization;
using System.Text.Json.Nodes;
using Armory.Roster;
using Armory.Run;

namespace Armory.Blizzard;

/// <summary>One faction, as a person would read it. The planner wants an id and a number; a page wants the words.</summary>
public sealed record FactionStanding
{
    public long Faction { get; init; }

    public string Name { get; init; } = "";

    /// <summary>What Blizzard calls the tier: Exalted, or a renown level's name.</summary>
    public string Tier { get; init; } = "";

    /// <summary>Progress within the tier, and what it takes to leave it. Both zero for a standing with no further to go.</summary>
    public long Value { get; init; }

    public long Max { get; init; }

    /// <summary>Renown level, for the factions that use renown instead of tiers.</summary>
    public long Renown { get; init; }

    /// <summary>Whether Warbands handed this to the character rather than the character earning it.</summary>
    public bool Inherited { get; init; }

    /// <summary>How far through the current tier, if the tier has a width. Null at the top: a full bar looks like one at 99%.</summary>
    public double? Fraction() => Max > 0 ? Math.Clamp(Value / (double)Max, 0.0, 1.0) : null;
}

/// <summary>A faction's standing, and whether it can be trusted as this character's own.</summary>
public sealed record Reputations
{
    public Dictionary<long, long> Standings { get; init; } = [];

    /// <summary>Factions whose standing is account-wide, and therefore may have been earned by anybody.</summary>
    public HashSet<long> Inherited { get; init; } = [];

    /// <summary>The same standings with their names and tiers, for showing.</summary>
    public List<FactionStanding> Detail { get; init; } = [];
}

/// <summary>
/// <c>/profile/...</c>: this account, and what its characters have done.
/// Everything here is a snapshot written when a character logs out. Every
/// endpoint honours <c>If-Modified-Since</c>, so a character who has not
/// played since the last sync answers 304 with no body.
/// </summary>
public static class Profile
{
    private const SourceId Source = SourceId.BlizzardProfile;

    /// <summary>Every character on the account, across every realm and every licence.</summary>
    public static Request Account(Region region) => Request.Get(Source, Api.Url(region, Namespace.Profile, "/profile/user/wow"));

    private static string CharacterPath(CharacterKey key, string suffix) => $"/profile/wow/character/{key.RealmSlug}/{key.Name}{suffix}";

    private static Request Character(Region region, CharacterKey key, string suffix) =>
        Request.Get(Source, Api.Url(region, Namespace.Profile, CharacterPath(key, suffix)));

    /// <summary>The character summary: item level, spec, guild, last logout.</summary>
    public static Request Summary(Region region, CharacterKey key) => Character(region, key, "");

    public static Request Professions(Region region, CharacterKey key) => Character(region, key, "/professions");

    public static Request MythicKeystone(Region region, CharacterKey key) => Character(region, key, "/mythic-keystone-profile");

    public static Request DungeonEncounters(Region region, CharacterKey key) => Character(region, key, "/encounters/dungeons");

    public static Request RaidEncounters(Region region, CharacterKey key) => Character(region, key, "/encounters/raids");

    public static Request Equipment(Region region, CharacterKey key) => Character(region, key, "/equipment");

    /// <summary>Gold. The one endpoint that takes numeric ids, and the only one that knows about money.</summary>
    public static Request ProtectedCharacter(Region region, Character character) =>
        Request.Get(Source, Api.Url(region, Namespace.Profile, $"/profile/user/wow/protected-character/{character.ProtectedId()}"));

    public static Request Achievements(Region region, CharacterKey key) => Character(region, key, "/achievements");

    public static Request Statistics(Region region, CharacterKey key) => Character(region, key, "/achievements/statistics");

    /// <summary>Every quest this character has completed. Genuinely per character.</summary>
    public static Request CompletedQuests(Region region, CharacterKey key) => Character(region, key, "/quests/completed");

    public static Request ReputationsOf(Region region, CharacterKey key) => Character(region, key, "/reputations");

    /// <summary>Read the account index into a roster.</summary>
    public static Outcome<Roster.Roster> ParseAccount(byte[] body)
    {
        var parsed = Outcomes.ParseJson<Roster.Roster>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        // `wow_accounts` is plural because one Battle.net login can hold
        // several WoW licences.
        if (parsed.Value.At("wow_accounts") is not JsonArray accounts)
        {
            return new Outcome<Roster.Roster>.Stale(new Reason.Malformed("the account profile carried no wow_accounts"));
        }
        var characters = new List<Character>();
        foreach (var account in accounts)
        {
            var wowAccountId = account.At("id").Int() ?? 0;
            foreach (var entry in account.At("characters").Items())
            {
                if (ReadCharacter(entry, wowAccountId) is { } character)
                {
                    characters.Add(character);
                }
            }
        }
        return Outcomes.OfCollection(characters).Map(list => new Roster.Roster(list));
    }

    private static Character? ReadCharacter(JsonNode? entry, long wowAccountId)
    {
        if (entry.At("name").Str() is not { } displayName || entry.At("realm") is not { } realm || realm.At("name").Str() is not { } realmName)
        {
            return null;
        }
        // The response carries the slug, but a locale that spells the realm
        // differently has been known to omit it.
        var realmSlug = realm.At("slug").Str() ?? Slug.RealmSlug(realmName);
        return new Character
        {
            Key = new CharacterKey(realmSlug, displayName),
            Id = entry.At("id").Int() ?? 0,
            RealmId = realm.At("id").Int() ?? 0,
            DisplayName = displayName,
            RealmName = realmName,
            Level = (int)(entry.At("level").Int() ?? 0),
            Class = entry.At("playable_class").Named(),
            Race = entry.At("playable_race").Named(),
            Faction = entry.At("faction").At("type").Str() is { } code ? FactionExtensions.FactionFromType(code) : Faction.Neutral,
            WowAccountId = wowAccountId,
        };
    }

    /// <summary>Read a character's achievements. An entry appears for anything with any progress, complete or not.</summary>
    public static Outcome<List<AchievementProgress>> ParseAchievements(byte[] body)
    {
        var parsed = Outcomes.ParseJson<List<AchievementProgress>>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        if (parsed.Value.At("achievements") is not JsonArray list)
        {
            return new Outcome<List<AchievementProgress>>.Stale(new Reason.Malformed("the achievements response carried no achievements list"));
        }
        var progress = new List<AchievementProgress>();
        foreach (var entry in list)
        {
            if (entry.At("achievement").At("id").Int() is not { } id)
            {
                continue;
            }
            progress.Add(new AchievementProgress
            {
                Id = id,
                CompletedAt = Millis(entry.At("completed_timestamp").Int()),
                Criteria = entry.At("criteria") is { } criteria ? ReadCriterion(criteria) : null,
            });
        }
        return Outcomes.OfCollection(progress);
    }

    /// <summary>
    /// Read one node of a criteria tree. The kind is left Unknown on purpose:
    /// the profile says how far along the account is and never what the
    /// criterion measures. Inventing it here would be inventing it from nothing.
    /// </summary>
    private static Criterion? ReadCriterion(JsonNode value)
    {
        if (value.At("id").Int() is not { } id)
        {
            return null;
        }
        return new Criterion
        {
            Id = id,
            Kind = CriterionKind.Unknown,
            Required = value.At("amount").Int() ?? 0,
            Children = value.At("child_criteria").Items().Select(child => child is null ? null : ReadCriterion(child)).OfType<Criterion>().ToList(),
        };
    }

    /// <summary>Blizzard stamps in milliseconds since the epoch.</summary>
    internal static DateTimeOffset? Millis(long? stamp) =>
        stamp is { } ms && ms >= -62135596800000 && ms <= 253402300799999 ? DateTimeOffset.FromUnixTimeMilliseconds(ms) : null;

    /// <summary>Read the completed-quest id list. A character who has completed nothing genuinely has no <c>quests</c> key.</summary>
    public static Outcome<HashSet<long>> ParseCompletedQuests(byte[] body)
    {
        var parsed = Outcomes.ParseJson<HashSet<long>>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        if (parsed.Value.At("quests") is not JsonArray list)
        {
            return new Outcome<HashSet<long>>.Empty();
        }
        var quests = list.Select(quest => quest.At("id").Int()).OfType<long>().ToHashSet();
        return quests.Count == 0 ? new Outcome<HashSet<long>>.Empty() : new Outcome<HashSet<long>>.Found(quests);
    }

    /// <summary>Read the per-character statistics into a flat id-to-value map.</summary>
    public static Outcome<Dictionary<long, double>> ParseStatistics(byte[] body)
    {
        var parsed = Outcomes.ParseJson<Dictionary<long, double>>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        if (parsed.Value.At("categories") is not JsonArray categories)
        {
            return new Outcome<Dictionary<long, double>>.Stale(new Reason.Malformed("the statistics response carried no categories"));
        }
        var statistics = new Dictionary<long, double>();
        foreach (var category in categories)
        {
            CollectStatistics(category, statistics);
        }
        return statistics.Count == 0 ? new Outcome<Dictionary<long, double>>.Empty() : new Outcome<Dictionary<long, double>>.Found(statistics);
    }

    private static void CollectStatistics(JsonNode? value, Dictionary<long, double> into)
    {
        foreach (var statistic in value.At("statistics").Items())
        {
            if (statistic.At("id").Int() is { } id)
            {
                into[id] = statistic.At("quantity").Num() ?? 0.0;
            }
        }
        foreach (var child in value.At("sub_categories").Items())
        {
            CollectStatistics(child, into);
        }
    }

    /// <summary>
    /// Read reputations, marking the ones Warbands made account-wide. The
    /// API offers no flag, so the honest heuristic: a standing on a character
    /// too low to have earned it is inherited, and reported rather than counted.
    /// </summary>
    public static Outcome<Reputations> ParseReputations(byte[] body, int characterLevel)
    {
        var parsed = Outcomes.ParseJson<Reputations>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        if (parsed.Value.At("reputations") is not JsonArray list)
        {
            return new Outcome<Reputations>.Stale(new Reason.Malformed("the reputations response carried no reputations list"));
        }
        var reputations = new Reputations();
        foreach (var entry in list)
        {
            var faction = entry.At("faction");
            if (faction.At("id").Int() is not { } id)
            {
                continue;
            }
            var standing = entry.At("standing");
            var raw = standing.At("raw").Int() ?? 0;
            var renown = standing.At("renown_level").Int() ?? 0;
            reputations.Standings[id] = raw;
            // A character below the level cap carrying renown could not have
            // earned it themselves. Levelled characters are left alone.
            var isInherited = renown > 0 && characterLevel < 70;
            if (isInherited)
            {
                reputations.Inherited.Add(id);
            }
            reputations.Detail.Add(new FactionStanding
            {
                Faction = id,
                Name = faction.Named(),
                Tier = standing.At("name").Str() ?? (renown > 0 ? string.Create(CultureInfo.InvariantCulture, $"Renown {renown}") : ""),
                Value = standing.At("value").Int() ?? 0,
                Max = standing.At("max").Int() ?? 0,
                Renown = renown,
                Inherited = isInherited,
            });
        }
        if (reputations.Standings.Count == 0)
        {
            return new Outcome<Reputations>.Empty();
        }
        reputations.Detail.Sort((a, b) => string.CompareOrdinal(a.Name, b.Name));
        return new Outcome<Reputations>.Found(reputations);
    }

    /// <summary>Read the character summary into the detail fields it supplies.</summary>
    public static Outcome<Detail> ParseSummary(byte[] body)
    {
        var parsed = Outcomes.ParseJson<Detail>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        var value = parsed.Value;
        // `id` is the one field every summary has.
        if (value.At("id").Int() is null)
        {
            return new Outcome<Detail>.Stale(new Reason.Malformed("the character summary carried no id"));
        }
        return new Outcome<Detail>.Found(new Detail
        {
            ItemLevel = (int?)value.At("average_item_level").Int(),
            EquippedItemLevel = (int?)value.At("equipped_item_level").Int(),
            Spec = value.At("active_spec").At("name").Str(),
            Guild = value.At("guild").At("name").Str(),
            AchievementPoints = value.At("achievement_points").Int(),
            LastLogin = Millis(value.At("last_login_timestamp").Int()),
        });
    }

    /// <summary>Read professions. Only the current expansion's tier is kept; tiers arrive oldest-first.</summary>
    public static Outcome<List<Profession>> ParseProfessions(byte[] body)
    {
        var parsed = Outcomes.ParseJson<List<Profession>>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        var professions = new List<Profession>();
        foreach (var (key, isPrimary) in new[] { ("primaries", true), ("secondaries", false) })
        {
            foreach (var entry in parsed.Value.At(key).Items())
            {
                if (entry.At("profession").At("name").Str() is not { } name)
                {
                    continue;
                }
                var tier = entry.At("tiers").Items().LastOrDefault();
                professions.Add(new Profession
                {
                    Name = name,
                    Tier = tier.At("tier").At("name").Str(),
                    Skill = (int?)tier.At("skill_points").Int(),
                    MaxSkill = (int?)tier.At("max_skill_points").Int(),
                    IsPrimary = isPrimary,
                    // The API knows nothing about trees or knowledge; the addon does.
                    Specialisations = [],
                    Knowledge = 0,
                });
            }
        }
        return Outcomes.OfCollection(professions);
    }

    /// <summary>Read the current season's Mythic+ rating. No keys this season is Empty, not a rating of zero.</summary>
    public static Outcome<long> ParseMythicKeystone(byte[] body)
    {
        var parsed = Outcomes.ParseJson<long>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        return parsed.Value.At("current_mythic_rating").At("rating").Num() is { } rating
            ? new Outcome<long>.Found((long)rating)
            : new Outcome<long>.Empty();
    }

    /// <summary>Read gold out of the protected-character response.</summary>
    public static Outcome<long> ParseProtected(byte[] body)
    {
        var parsed = Outcomes.ParseJson<long>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        return parsed.Value.At("money").Int() is { } money
            ? new Outcome<long>.Found(money)
            : new Outcome<long>.Stale(new Reason.Malformed("the protected character carried no money"));
    }

    /// <summary>Read the encounter ids a character has completed. A character who has cleared nothing has no <c>expansions</c> key.</summary>
    public static Outcome<HashSet<long>> ParseEncounters(byte[] body)
    {
        var parsed = Outcomes.ParseJson<HashSet<long>>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        if (parsed.Value.At("expansions") is not JsonArray expansions)
        {
            return new Outcome<HashSet<long>>.Empty();
        }
        var encounters = new HashSet<long>();
        foreach (var expansion in expansions)
        {
            CollectEncounters(expansion, encounters);
        }
        return encounters.Count == 0 ? new Outcome<HashSet<long>>.Empty() : new Outcome<HashSet<long>>.Found(encounters);
    }

    private static void CollectEncounters(JsonNode? value, HashSet<long> into)
    {
        // The nesting is expansion, instances, modes, progress, encounters,
        // and Blizzard has changed it before, so this walks whichever of the
        // known keys is present in whichever form.
        foreach (var key in new[] { "instances", "modes", "progress", "encounters", "instance" })
        {
            var child = value.At(key);
            if (child is JsonArray list)
            {
                foreach (var entry in list)
                {
                    CollectEncounters(entry, into);
                }
            }
            else if (child is not null)
            {
                CollectEncounters(child, into);
            }
        }
        if (value.At("encounter").At("id").Int() is { } id)
        {
            // `completed_count` present and zero is listed but never killed.
            // Absent means the response did not break it down.
            var killed = value.At("completed_count").Int() ?? 1;
            if (killed > 0)
            {
                into.Add(id);
            }
        }
    }

    /// <summary>
    /// Read what a character is wearing. Only what is worn: an empty slot is
    /// simply absent, and this does not invent a row for it. A cosmetic slot
    /// has no level at all, never a zero.
    /// </summary>
    public static Outcome<List<Equipped>> ParseEquipment(byte[] body)
    {
        var parsed = Outcomes.ParseJson<List<Equipped>>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        if (parsed.Value.At("equipped_items") is not JsonArray items)
        {
            return new Outcome<List<Equipped>>.Stale(new Reason.Malformed("the equipment response carried no equipped_items"));
        }
        var worn = new List<Equipped>();
        foreach (var item in items)
        {
            if (item.At("slot") is not { } slot || slot.At("type").Str() is not { } slotType || item.At("name").Str() is not { } name)
            {
                continue;
            }
            worn.Add(new Equipped
            {
                Slot = slotType,
                SlotName = slot.At("name").Str() ?? "",
                Name = name,
                Level = (int?)item.At("level").At("value").Int(),
            });
        }
        // A naked character is a real answer, and Empty is what tells it
        // from a parser that stopped understanding the response.
        return Outcomes.OfCollection(worn);
    }

    /// <summary>Read raid progress, one instance at a time. A difficulty never entered is absent rather than nought-of-eight.</summary>
    public static Outcome<List<RaidTier>> ParseRaids(byte[] body)
    {
        var parsed = Outcomes.ParseJson<List<RaidTier>>(Source, body);
        if (!parsed.IsOk)
        {
            return parsed.Error;
        }
        if (parsed.Value.At("expansions") is not JsonArray expansions)
        {
            return new Outcome<List<RaidTier>>.Empty();
        }
        var tiers = new List<RaidTier>();
        foreach (var expansion in expansions)
        {
            var name = expansion.At("expansion").Named();
            foreach (var instance in expansion.At("instances").Items())
            {
                if (ReadRaid(instance, name) is { } tier)
                {
                    tiers.Add(tier);
                }
            }
        }
        return Outcomes.OfCollection(tiers);
    }

    private static RaidTier? ReadRaid(JsonNode? instance, string expansion)
    {
        if (instance.At("instance").At("name").Str() is not { } name || instance.At("modes") is not JsonArray modes)
        {
            return null;
        }
        var difficulties = new List<RaidDifficulty>();
        foreach (var mode in modes)
        {
            if (mode.At("progress") is not { } progress
                || mode.At("difficulty").At("name").Str() is not { } difficulty
                || progress.At("completed_count").Int() is not { } defeated
                || progress.At("total_count").Int() is not { } total)
            {
                continue;
            }
            // The last kill is a per-encounter stamp, so the raid's is the latest.
            Kill? lastKill = null;
            foreach (var encounter in progress.At("encounters").Items())
            {
                if ((encounter.At("completed_count").Int() ?? 0) <= 0
                    || encounter.At("encounter").At("name").Str() is not { } boss
                    || Millis(encounter.At("last_kill_timestamp").Int()) is not { } at)
                {
                    continue;
                }
                if (lastKill is null || at > lastKill.At)
                {
                    lastKill = new Kill(boss, at);
                }
            }
            difficulties.Add(new RaidDifficulty { Name = difficulty, Defeated = (int)defeated, Total = (int)total, LastKill = lastKill });
        }
        return difficulties.Count == 0 ? null : new RaidTier { Name = name, Expansion = expansion, Difficulties = difficulties };
    }

    /// <summary>The highest renown across the account-wide major factions. A fact about the account, never run progress.</summary>
    public static long? HighestRenown(JsonNode reputations)
    {
        long? best = null;
        foreach (var entry in reputations.At("reputations").Items())
        {
            if (entry.At("standing").At("renown_level").Int() is { } renown && (best is null || renown > best))
            {
                best = renown;
            }
        }
        return best;
    }

    /// <summary>Assemble what a character's own data can answer. Earned reputations and the run's achievements are the planner's to fill in.</summary>
    public static PrimaryData PrimaryOf(HashSet<long> quests, Dictionary<long, double> statistics, Reputations reputations, HashSet<long> encounters) => new()
    {
        Quests = quests,
        Statistics = statistics,
        Encounters = encounters,
        Reputations = reputations.Standings,
        InheritedReputations = reputations.Inherited,
    };
}
