using System.Text.Json;
using Armory.Roster;
using Xunit;

namespace Armory.Tests.Roster;

/// <summary>Ported from <c>core/src/character.rs</c>.</summary>
public sealed class CharacterTests
{
    internal static Character Someone(string realm, string name) => new()
    {
        Key = new CharacterKey(Armory.Blizzard.Slug.RealmSlug(realm), name),
        Id = 1,
        RealmId = 2,
        DisplayName = name,
        RealmName = realm,
        Level = 80,
        Class = "Druid",
        Race = "Tauren",
        Faction = Faction.Horde,
        WowAccountId = 1,
    };

    [Fact(DisplayName = "a_key_lowercases_the_name_because_the_endpoints_do")]
    public void A_key_lowercases_the_name_because_the_endpoints_do()
    {
        Assert.Equal("somechar", new CharacterKey("emerald-dream", "Somechar").Name);
    }

    [Fact(DisplayName = "a_key_reads_back_as_a_name_when_it_reaches_prose")]
    public void A_key_reads_back_as_a_name_when_it_reaches_prose()
    {
        Assert.Equal("Aeltor", new CharacterKey("mannoroth", "Aeltor").DisplayName());
        Assert.Equal("", new CharacterKey("x", "").DisplayName());
    }

    [Fact(DisplayName = "a_roster_sorts_by_realm_then_name")]
    public void A_roster_sorts_by_realm_then_name()
    {
        var roster = new Armory.Roster.Roster([Someone("Thrall", "Ulahae"), Someone("Emerald Dream", "Velkurai"), Someone("Emerald Dream", "Atulak")]);
        Assert.Equal(["Atulak", "Velkurai", "Ulahae"], roster.Characters.Select(character => character.DisplayName));
    }

    [Fact(DisplayName = "realms_are_deduplicated_because_each_one_is_an_auction_house")]
    public void Realms_are_deduplicated_because_each_one_is_an_auction_house()
    {
        var roster = new Armory.Roster.Roster([Someone("Emerald Dream", "Atulak"), Someone("Emerald Dream", "Velkurai"), Someone("Mannoroth", "Aeltor")]);
        Assert.Equal([("emerald-dream", "Emerald Dream"), ("mannoroth", "Mannoroth")], roster.Realms());
    }

    [Fact(DisplayName = "the_protected_endpoint_wants_the_numeric_pair")]
    public void The_protected_endpoint_wants_the_numeric_pair()
    {
        var character = Someone("Dalaran", "Moodivh") with { RealmId = 3684, Id = 12345 };
        Assert.Equal("3684-12345", character.ProtectedId());
    }

    [Fact(DisplayName = "absorbing_the_addons_answer_keeps_what_only_the_api_knows")]
    public void Absorbing_the_addons_answer_keeps_what_only_the_api_knows()
    {
        // This was a real wipe. The addon's Detail has no Mythic+ rating, no
        // renown, no achievement points and no lifetime raiding.
        var held = new Detail
        {
            ItemLevel = 639,
            MythicRating = 2418,
            Renown = 80,
            AchievementPoints = 28_940,
            Raids = [new RaidTier { Name = "Liberation of Undermine", Expansion = "The War Within" }],
        };

        var merged = held.Absorb(new Detail
        {
            ItemLevel = 641,
            Money = 1_234_500,
            RaidLocks = [new RaidLock { Name = "Liberation of Undermine", Difficulty = "Heroic", Defeated = 2, Total = 8 }],
        });

        Assert.Equal(641, merged.ItemLevel);
        Assert.Equal(1_234_500, merged.Money);
        Assert.Equal(2418, merged.MythicRating);
        Assert.Equal(80, merged.Renown);
        Assert.Equal(28_940, merged.AchievementPoints);
        Assert.NotNull(merged.Raids);
        Assert.NotNull(merged.RaidLocks);
    }

    [Fact(DisplayName = "a_profession_keeps_the_half_the_other_source_wrote")]
    public void A_profession_keeps_the_half_the_other_source_wrote()
    {
        var held = new Detail
        {
            Professions = [new Profession { Name = "Alchemy", Tier = "Khaz Algar Alchemy", Skill = 100, MaxSkill = 100, IsPrimary = true }],
        };
        var merged = held.Absorb(new Detail
        {
            Professions =
            [
                new Profession
                {
                    Name = "Alchemy", Skill = 100, MaxSkill = 100, IsPrimary = true,
                    Specialisations = [new Specialisation("Potion Mastery", true)], Knowledge = 42,
                },
            ],
        });
        var alchemy = merged.Professions[0];
        Assert.Equal("Khaz Algar Alchemy", alchemy.Tier);
        Assert.Equal(42, alchemy.Knowledge);
        Assert.Single(alchemy.Specialisations);
    }

    [Fact(DisplayName = "the_last_kill_is_the_latest_across_every_difficulty")]
    public void The_last_kill_is_the_latest_across_every_difficulty()
    {
        var tier = new RaidTier
        {
            Name = "Liberation of Undermine",
            Expansion = "The War Within",
            Difficulties =
            [
                new RaidDifficulty { Name = "Normal", Defeated = 8, Total = 8, LastKill = new Kill("Mug'Zee", DateTimeOffset.FromUnixTimeSeconds(1_753_800)) },
                new RaidDifficulty { Name = "Heroic", Defeated = 2, Total = 8, LastKill = new Kill("Vexie", DateTimeOffset.FromUnixTimeSeconds(1_754_000)) },
            ],
        };
        var (boss, _, difficulty) = tier.LastKill()!.Value;
        Assert.Equal("Vexie", boss);
        Assert.Equal("Heroic", difficulty);
    }

    [Fact(DisplayName = "a detail's JSON is the shape the Rust writes")]
    public void A_details_json_is_the_shape_the_rust_writes()
    {
        // `detail.json` travels between machines, so both implementations read
        // each other's. This is the Rust's serialisation of the same record.
        const string rust = """{"item_level":641,"equipped_item_level":null,"spec":"Restoration","guild":null,"money":12,"achievement_points":null,"last_login":"2026-09-10T20:14:00Z","professions":[{"name":"Alchemy","tier":null,"skill":100,"max_skill":100,"is_primary":true,"specialisations":[["Potion Mastery",true]],"knowledge":42}],"mythic_rating":null,"renown":null,"equipment":null,"raids":[{"name":"Undermine","expansion":"TWW","difficulties":[{"name":"Heroic","defeated":2,"total":8,"last_kill":["Vexie","2026-09-01T00:00:00Z"]}]}],"raid_locks":null,"vault":[{"row":"Dungeons","index":1,"threshold":1,"progress":1,"level":12,"level_name":""},{"row":{"Other":9},"index":1,"threshold":1,"progress":0,"level":0,"level_name":""}],"vault_ready":true}""";
        var detail = JsonSerializer.Deserialize<Detail>(rust)!;
        Assert.Equal("Restoration", detail.Spec);
        Assert.Equal(new Specialisation("Potion Mastery", true), detail.Professions[0].Specialisations[0]);
        Assert.Equal("Vexie", detail.Raids![0].Difficulties[0].LastKill!.Boss);
        Assert.Equal(VaultRow.Dungeons, detail.Vault![0].Row);
        Assert.Equal(new VaultRow(VaultRowKind.Other, 9), detail.Vault[1].Row);
        Assert.Equal("+12", detail.Vault[0].Reward());
        Assert.Equal("Row 9", detail.Vault[1].Row.Label());

        var again = JsonSerializer.Deserialize<Detail>(JsonSerializer.Serialize(detail))!;
        Assert.Equal(detail, again with { Professions = detail.Professions, Raids = detail.Raids, Vault = detail.Vault });
        Assert.Contains("\"last_kill\":[\"Vexie\",\"2026-09-01T00:00:00Z\"]", JsonSerializer.Serialize(detail), StringComparison.Ordinal);
    }
}
