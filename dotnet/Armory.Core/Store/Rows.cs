namespace Armory.Store;

// The schema, one class a table, named as the Rust store names them so a row
// on the wire is positional over the same columns on both ends. Integers are
// `long` throughout because SQLite's are; booleans are integers because the
// triggers and the upserts compare them as values and the wire carries them
// as numbers. The comments on what each table is for are in `ArmoryContext`.

public sealed class CharacterRow
{
    public required string RealmSlug { get; set; }
    public required string Name { get; set; }
    public long CharacterId { get; set; }
    public long RealmId { get; set; }
    public string DisplayName { get; set; } = "";
    public string RealmName { get; set; } = "";
    public long Level { get; set; }
    public string Class { get; set; } = "";
    public string Race { get; set; } = "";
    public string Faction { get; set; } = "";
    public long WowAccountId { get; set; }
}

public sealed class EnrolmentRow
{
    public required string RealmSlug { get; set; }
    public required string Name { get; set; }
}

public sealed class DetailRow
{
    public required string RealmSlug { get; set; }
    public required string Name { get; set; }
    public string Json { get; set; } = "{}";
    public string FetchedAt { get; set; } = "";
}

public sealed class AttributionRow
{
    public long AchievementId { get; set; }
    public string RealmSlug { get; set; } = "";
    public string Name { get; set; } = "";
}

public sealed class CurrencyRow
{
    public required string RealmSlug { get; set; }
    public required string Name { get; set; }
    public long CurrencyId { get; set; }
    public long Amount { get; set; }
}

public sealed class EarnedReputationRow
{
    public required string RealmSlug { get; set; }
    public required string Name { get; set; }
    public long FactionId { get; set; }
    public long Points { get; set; }
    public long Renown { get; set; }
    public long RenownSeen { get; set; }
    public long AccountWide { get; set; }
}

public sealed class EarnedCurrencyRow
{
    public required string RealmSlug { get; set; }
    public required string Name { get; set; }
    public long CurrencyId { get; set; }
    public long Gained { get; set; }
    public long Earned { get; set; }
    public long TracksEarned { get; set; }
    public long AccountWide { get; set; }
    public long Transferable { get; set; }
}

public sealed class TallyRow
{
    public required string RealmSlug { get; set; }
    public required string Name { get; set; }
    public required string Kind { get; set; }
    public required string Key { get; set; }
    public long Count { get; set; }
    public string Label { get; set; } = "";
}

public sealed class RecipeRow
{
    public required string RealmSlug { get; set; }
    public required string Name { get; set; }
    public long RecipeId { get; set; }
    public string Recipe { get; set; } = "";
    public long OutputId { get; set; }
    public long Makes { get; set; }
}

public sealed class RecipeReagentRow
{
    public required string RealmSlug { get; set; }
    public required string Name { get; set; }
    public long RecipeId { get; set; }
    public long Slot { get; set; }
    public long Quantity { get; set; }
    public string Tiers { get; set; } = "";
}

public sealed class InstanceRow
{
    public long Id { get; set; }
    public string Name { get; set; } = "";
    public long? Map { get; set; }
    public string Description { get; set; } = "";
    public string Expansion { get; set; } = "";
    public string Encounters { get; set; } = "";
}

public sealed class EncounterRow
{
    public long Id { get; set; }
    public string Name { get; set; } = "";
    public string Description { get; set; } = "";
    public string Loot { get; set; } = "";
}

public sealed class CriterionRow
{
    public long CriterionId { get; set; }
    public string Kind { get; set; } = "";
}

public sealed class WarbandItemRow
{
    public long ItemId { get; set; }
    public long Count { get; set; }
}

public sealed class PetHeldRow
{
    public long SpeciesId { get; set; }
    public long Count { get; set; }
}

public sealed class GoalRow
{
    public long RunId { get; set; }
    public long AchievementId { get; set; }
    public string Standing { get; set; } = "";
    public string Bucket { get; set; } = "";
    public string? Attestation { get; set; }
}

public sealed class RunRow
{
    public long Id { get; set; }
    public string Name { get; set; } = "";
    public string Baseline { get; set; } = "";
    public string Cohort { get; set; } = "";
    public long IsCurrent { get; set; }
    public string Key { get; set; } = "";
}

public sealed class CollectibleRow
{
    public required string Kind { get; set; }
    public long Id { get; set; }
    public string Json { get; set; } = "{}";
    public long Owned { get; set; }
}

public sealed class AchievementRow
{
    public long Id { get; set; }
    public string Name { get; set; } = "";
    public string Category { get; set; } = "";
    public long Points { get; set; }
    public string Description { get; set; } = "";
    public long Unrepeatable { get; set; }
}

public sealed class PriceRow
{
    public long Realm { get; set; }
    public long ItemId { get; set; }
    public required string Variant { get; set; }
    public long UnitPrice { get; set; }
    public long Quantity { get; set; }
    public required string SeenAt { get; set; }
    public long Listings { get; set; }
    public long Tenth { get; set; }
    public long Median { get; set; }
}

public sealed class SnapshotRow
{
    public long Realm { get; set; }
    public long ItemId { get; set; }
    public required string Variant { get; set; }
    public long Cheapest { get; set; }
    public long Quantity { get; set; }
    public long Listings { get; set; }
    public long Tenth { get; set; }
    public long Median { get; set; }
    public string SeenAt { get; set; } = "";
}

public sealed class ItemRow
{
    public long ItemId { get; set; }
    public string Name { get; set; } = "";
    public long Sellable { get; set; } = 1;
    public string Quality { get; set; } = "";
}

public sealed class WatchedRow
{
    public long ItemId { get; set; }
    public string Name { get; set; } = "";
}

public sealed class WatchedRealmRow
{
    public long RealmId { get; set; }
    public string Name { get; set; } = "";
}

public sealed class SessionRow
{
    public required string RealmSlug { get; set; }
    public required string Name { get; set; }
    public required string StartedAt { get; set; }
    public string EndedAt { get; set; } = "";
    public string Json { get; set; } = "{}";
}

public sealed class EntryRow
{
    public required string RealmSlug { get; set; }
    public required string Name { get; set; }
    public required string StartedAt { get; set; }
    public string Title { get; set; } = "";
    public string Body { get; set; } = "";
    public string Model { get; set; } = "";
    public string WrittenAt { get; set; } = "";
}

public sealed class ResponseRow
{
    public required string Url { get; set; }
    public byte[] Body { get; set; } = [];
    public string? LastModified { get; set; }
    public string FetchedAt { get; set; } = "";
}

public sealed class ForgottenRow
{
    public required string RealmSlug { get; set; }
    public required string Name { get; set; }
    public required string StartedAt { get; set; }
    public string At { get; set; } = "";
}

/// <summary>
/// Which rows have moved, and in what order. One entry a row rather than one
/// an edit: a write deletes the entry it finds and inserts a new one, so the
/// log stays the size of the data. On a client this is an outbox that
/// empties; on the server it is a log that is kept.
/// </summary>
public sealed class ChangeRow
{
    public long Seq { get; set; }
    public required string Scope { get; set; }
    public required string Key { get; set; }
    public long Gone { get; set; }
    public string At { get; set; } = "";
    public string Machine { get; set; } = "";
}

/// <summary>Cursors, the recording flag and this installation's id.</summary>
public sealed class SyncStateRow
{
    public required string Name { get; set; }
    public string Value { get; set; } = "";
}
