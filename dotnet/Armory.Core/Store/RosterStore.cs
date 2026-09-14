using System.Text.Json;
using Armory.Roster;
using Microsoft.EntityFrameworkCore;

namespace Armory.Store;

/// <summary>The roster, the cohort and each character's detail.</summary>
public sealed partial class Store
{
    /// <summary>
    /// Replace the roster wholesale. Characters are deleted and transferred,
    /// so a merge would leave ghosts behind that go on settling goals nothing
    /// can account for.
    /// </summary>
    public Result<Unit, StoreError> SaveRoster(Roster.Roster roster) => Work(() =>
    {
        var rows = roster.Characters.Select(character => new CharacterRow
        {
            RealmSlug = character.Key.RealmSlug,
            Name = character.Key.Name,
            CharacterId = character.Id,
            RealmId = character.RealmId,
            DisplayName = character.DisplayName,
            RealmName = character.RealmName,
            Level = character.Level,
            Class = character.Class,
            Race = character.Race,
            Faction = character.Faction.Label(),
            WowAccountId = character.WowAccountId,
        }).ToList();
        Reconcile(Context.Characters, rows, row => (row.RealmSlug, row.Name), (held, fresh) =>
        {
            held.CharacterId = fresh.CharacterId;
            held.RealmId = fresh.RealmId;
            held.DisplayName = fresh.DisplayName;
            held.RealmName = fresh.RealmName;
            held.Level = fresh.Level;
            held.Class = fresh.Class;
            held.Race = fresh.Race;
            held.Faction = fresh.Faction;
            held.WowAccountId = fresh.WowAccountId;
        });
    });

    public Result<Roster.Roster, StoreError> RosterHeld() => Work(() =>
        new Roster.Roster(Context.Characters.AsNoTracking().AsEnumerable().Select(row => new Character
        {
            Key = new CharacterKey(row.RealmSlug, row.Name),
            Id = row.CharacterId,
            RealmId = row.RealmId,
            DisplayName = row.DisplayName,
            RealmName = row.RealmName,
            Level = (int)row.Level,
            Class = row.Class,
            Race = row.Race,
            Faction = row.Faction switch
            {
                "Alliance" => Faction.Alliance,
                "Horde" => Faction.Horde,
                _ => Faction.Neutral,
            },
            WowAccountId = row.WowAccountId,
        })));

    public Result<Unit, StoreError> SaveCohort(Cohort cohort) => Work(() =>
    {
        var rows = cohort.Keys.Select(key => new EnrolmentRow { RealmSlug = key.RealmSlug, Name = key.Name }).ToList();
        Reconcile(Context.Enrolments, rows, row => (row.RealmSlug, row.Name), (_, _) => { });
    });

    public Result<Cohort, StoreError> CohortHeld() => Work(() =>
        new Cohort(Context.Enrolments.AsNoTracking().AsEnumerable().Select(row => new CharacterKey(row.RealmSlug, row.Name))));

    /// <summary>Save one character's detail.</summary>
    public Result<Unit, StoreError> SaveDetail(CharacterKey key, Detail detail) => Work(() =>
    {
        var json = JsonSerializer.Serialize(detail);
        var held = Context.Details.Find(key.RealmSlug, key.Name);
        if (held is null)
        {
            Context.Details.Add(new DetailRow { RealmSlug = key.RealmSlug, Name = key.Name, Json = json, FetchedAt = Now() });
        }
        else
        {
            held.Json = json;
            held.FetchedAt = Now();
        }
    });

    /// <summary>
    /// Every character's detail, keyed for joining onto the roster. A row
    /// whose JSON no longer parses is dropped rather than failing the read:
    /// the detail is refetchable.
    /// </summary>
    public Result<Dictionary<CharacterKey, Detail>, StoreError> Details() => Work(() =>
    {
        var result = new Dictionary<CharacterKey, Detail>();
        foreach (var row in Context.Details.AsNoTracking())
        {
            try
            {
                if (JsonSerializer.Deserialize<Detail>(row.Json) is { } detail)
                {
                    result[new CharacterKey(row.RealmSlug, row.Name)] = detail;
                }
            }
            catch (JsonException)
            {
                // Refetchable, and not worth the whole roster's detail.
            }
        }
        return result;
    });
}
